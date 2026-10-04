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


def compile_diagnostics(output):
    # Only fixed compiler categories leave this bounded private output buffer.
    # Cargo may force ANSI colors despite a non-terminal stderr destination.
    plain = re.sub(rb'\x1b\[[0-9;]{0,32}m', b'', output)
    codes = {code.decode('ascii') for code in
             re.findall(rb'(?m)^error\[(E[0-9]{4})\]:', plain)}
    known = sorted(codes & RUSTC_CODES)
    other = bool(codes - RUSTC_CODES)
    link = any(line.startswith(b'error: linking with ') and b' failed:' in line
               or re.search(rb'\b(?:fatal )?error LNK[0-9]{4}:', line) is not None
               for line in plain.splitlines())
    category = 'rustc-code' if codes else 'link-stage' if link else 'no-code'
    return dict(category=category, rustcCodes=known, otherRustcCode=other, linkStage=link)


def classify(output, succeeded):
    started = any(line.startswith(('test ' + TEST + ' ...').encode()) for line in output.splitlines())
    if len(output) > 131072:
        category = 'output-overflow'
    elif succeeded:
        category = 'passed'
    elif b'could not compile' in output:
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
               'desktop-qualification', '--lib', TEST, '--', '--exact']
    succeeded = False
    with tempfile.TemporaryFile() as private:
        try:
            result = subprocess.run(command, env=environment, stdout=private, stderr=subprocess.STDOUT,
                                    stdin=subprocess.DEVNULL, timeout=180, check=False)
            succeeded = result.returncode == 0
            private.seek(0)
            observation = classify(private.read(131073), succeeded)
        except subprocess.TimeoutExpired:
            observation = dict(fixtureStarted=False, category='driver-deadline')
        except OSError:
            observation = dict(fixtureStarted=False, category='driver-spawn-failure')
    facts = dict(schemaVersion=1, mechanism='windows-configuration-fixture-driver', diagnosticsOnly=True,
                 sourceSha=source, phase='after-installation', **observation)
    with (Path(os.environ['RUNNER_TEMP']) / 'configuration-fixture-driver.json').open('x') as output:
        json.dump(facts, output)
    return 0 if succeeded else 1


if __name__ == '__main__':
    raise SystemExit(main())
