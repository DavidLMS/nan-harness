#!/usr/bin/env python3
"""Run only the synthetic writer fixture and publish fixed diagnostic categories."""
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile

TEST = 'commands::claude_desktop::session::configuration_persist_tests::production_std_rename_lifecycle_under_precreation_leases_restores_documents'


RUSTC_CODES = frozenset(('E0061','E0277','E0282','E0308','E0382','E0425','E0432',
                         'E0433','E0455','E0463','E0502','E0514','E0599','E0603'))


# These are anchored tool signatures, never extracted diagnostic payloads.
NO_CODE_SIGNATURES = (
    ('build-script', rb"^error: failed to run custom build command for "),
    ('environment-variable', rb'^error: environment variable [^\r\n]+ not defined at compile time'),
    ('read-file', rb"^error: (?:couldn't|could not|failed to) read "),
    ('write-file', rb'^error: (?:could not|failed to) write '),
    ('remove-file', rb'^error: (?:could not|failed to) remove '),
    ('spawn-process', rb'^error: could not (?:execute|exec) process '),
    ('linker-unavailable', rb'^error: linker [^\r\n]+ not found'),
    ('metadata-file', rb'^error: failed to (?:read|write) [^\r\n]*metadata'),
    ('output-file', rb'^error: could not open [^\r\n]*output file'),
    ('archive-file', rb'^error: failed to build archive '),
    ('temporary-directory', rb"^error: (?:couldn't|could not|failed to) create (?:a )?temp(?:orary)? dir"),
    ('emit-output', rb'^error: failed to emit '),
)



def structured_diagnostics(output):
    # Cargo JSON fields stay private. Only fixed aggregate values leave this parser.
    codes=set();signals=set();levels={'error':0,'warning':0,'failure-note':0,'note':0,'help':0,'ice':0}
    messages=0;malformed=False;unknown_error=False;link=False;finished='unobserved'
    def diagnostic(value,depth=0):
        nonlocal malformed,messages,unknown_error,link
        if(depth>8 or messages>=256 or not isinstance(value,dict)):
            malformed=True;return
        level=value.get('level');message=value.get('message');code=value.get('code')
        if(type(level) is not str or level not in levels or not isinstance(message,str) or len(message)>65536):
            malformed=True;return
        messages+=1;levels[level]+=1
        parsed=None
        if code is not None:
            if not isinstance(code,dict) or not isinstance(code.get('code'),str):
                malformed=True
            elif re.fullmatch(r'E[0-9]{4}',code['code']) is not None:
                parsed=code['code'];codes.add(parsed)
        # Rustc's structured message omits rendered `error:` prefixes and ANSI.
        # Never inspect rendered, spans, filenames, package IDs or explanations.
        try:data=('error: '+message).encode('utf-8')
        except UnicodeError:malformed=True;return
        matches={name for name,pattern in NO_CODE_SIGNATURES if re.search(pattern,data) is not None}
        is_link=message.startswith('linking with ') and ' failed:' in message
        signals.update(matches);link=link or is_link
        if level in {'error','ice'} and not parsed and not matches and not is_link:
            unknown_error=True
        children=value.get('children',[])
        if not isinstance(children,list) or len(children)>64:malformed=True;return
        for child in children:diagnostic(child,depth+1)
    for line in output.splitlines():
        if not line.startswith(b'{'):continue
        if len(line)>65536:malformed=True;continue
        try:value=json.loads(line)
        except (ValueError,UnicodeError):malformed=True;continue
        if not isinstance(value,dict):malformed=True;continue
        if value.get('reason')=='compiler-message':diagnostic(value.get('message'))
        elif value.get('reason')=='build-finished':
            success=value.get('success')
            if type(success) is not bool:malformed=True
            else:finished='succeeded' if success else 'failed'
    return dict(codes=codes,signals=signals,link=link,
        facts=dict(diagnosticCount=messages,levels=levels,malformed=malformed,
                   otherErrorMessage=unknown_error,buildFinished=finished))

def compile_diagnostics(output):
    # Only fixed compiler categories leave this bounded private output buffer.
    # Cargo may force ANSI colors despite a non-terminal stderr destination.
    plain = re.sub(rb'\x1b\[[0-9;]{0,32}m', b'', output)
    codes = {code.decode('ascii') for code in
             re.findall(rb'(?m)^error\[(E[0-9]{4})\]:', plain)}
    structured = structured_diagnostics(output)
    codes.update(structured['codes'])
    known = sorted(codes & RUSTC_CODES)
    other = bool(codes - RUSTC_CODES)
    link = any(line.startswith(b'error: linking with ') and b' failed:' in line
               or re.search(rb'\b(?:fatal )?error LNK[0-9]{4}:', line) is not None
               for line in plain.splitlines())
    link = link or structured['link']
    category = 'rustc-code' if codes else 'link-stage' if link else 'no-code'
    signals = sorted({name for name, pattern in NO_CODE_SIGNATURES
                      if re.search(pattern, plain, re.MULTILINE) is not None})
    return dict(category=category, rustcCodes=known, otherRustcCode=other, linkStage=link,
                noCodeSignals=sorted(set(signals)|structured['signals']),
                structured=structured['facts'])


def classify(output, succeeded):
    structured = structured_diagnostics(output)['facts'] if len(output) <= 131072 and not succeeded else None
    started = any(line.startswith(('test ' + TEST + ' ...').encode()) for line in output.splitlines())
    if len(output) > 131072:
        category = 'output-overflow'
    elif succeeded:
        category = 'passed' if started else 'fixture-not-run'
    elif (b'could not compile' in output
          or structured['buildFinished'] == 'failed'
          or structured['levels']['error'] > 0
          or structured['levels']['ice'] > 0):
        category = 'compile-failure'
    elif b'could not execute process' in output:
        category = 'test-process-unavailable'
    elif started and b'os error 32' in output:
        category = 'fixture-panic-sharing-violation'
    elif started and b'os error 5' in output:
        category = 'fixture-panic-access-denied'
    elif started:
        category = 'fixture-failure'
    else:
        category = 'unclassified-failure'
    result = dict(fixtureStarted=started, category=category)
    if category == 'compile-failure':
        result['compileDiagnostics'] = compile_diagnostics(output)
    return result


def read_private_output(private):
    # Cargo artifact records are not diagnostics and must not crowd the bounded
    # observation. Both individual records and total scan work remain bounded.
    kept=bytearray();scanned=0
    while True:
        line=private.readline(65537)
        if not line:return bytes(kept)
        scanned+=len(line)
        if len(line)>65536 or scanned>16777216:
            return b'\n'*131073
        if line.startswith(b'{'):
            try:value=json.loads(line)
            except (ValueError,UnicodeError):value=None
            if isinstance(value,dict) and type(value.get('reason')) is str and value['reason'] in {'compiler-artifact','build-script-executed'}:
                continue
        if len(kept)+len(line)>131072:return b'\n'*131073
        kept.extend(line)


def main():
    expected = dict(GITHUB_ACTIONS='true', RUNNER_ENVIRONMENT='github-hosted', RUNNER_OS='Windows',
                    NANH_CONFIGURATION_POSTINSTALL_OBSERVATION='1')
    if any(os.environ.get(key) != value for key, value in expected.items()) or os.name != 'nt':
        raise ValueError('hosted Windows synthetic fixture required')
    source = os.environ['GITHUB_SHA']
    if re.fullmatch('[0-9a-fA-F]{40}', source) is None:
        raise ValueError('fixture commit required')
    environment = {key: value for key, value in os.environ.items()
                   if key not in {'NAN_API_KEY', 'GH_TOKEN', 'GITHUB_TOKEN'}}
    command = ['cargo', 'test', '--locked', '-p', 'nan-harness-cli', '--features',
               'desktop-qualification', '--message-format=json', '--lib', TEST, '--', '--exact']
    succeeded = False
    with tempfile.TemporaryFile() as private:
        try:
            result = subprocess.run(command, env=environment, stdout=private, stderr=subprocess.STDOUT,
                                    stdin=subprocess.DEVNULL, timeout=180, check=False)
            succeeded = result.returncode == 0
            private.seek(0)
            observation = classify(read_private_output(private), succeeded)
        except subprocess.TimeoutExpired:
            observation = dict(fixtureStarted=False, category='driver-deadline')
        except OSError:
            observation = dict(fixtureStarted=False, category='driver-spawn-failure')
    facts = dict(schemaVersion=1, mechanism='windows-configuration-fixture-driver', diagnosticsOnly=True,
                 sourceSha=source, phase='after-installation', **observation)
    with (Path(os.environ['RUNNER_TEMP']) / 'configuration-fixture-driver.json').open('x') as output:
        json.dump(facts, output)
    return 0 if observation['category']=='passed' else 1


if __name__ == '__main__':
    raise SystemExit(main())
