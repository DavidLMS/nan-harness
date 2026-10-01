#!/usr/bin/env python3
"""Closed branch-only desktop qualification matrix and report reducer."""
import argparse
import hashlib
import json
from pathlib import Path
import re

from desktop_suite import read_frozen_manifest, validated_report

APPS = ('zed-desktop', 'chatgpt-desktop', 'claude-desktop', 'hermes-desktop', 'pen-desktop')
TARGETS = (('linux', 'x86_64', 'ubuntu-24.04'), ('macos', 'aarch64', 'macos-15'),
           ('windows', 'x86_64', 'windows-2025'))
BACKENDS = {('zed-desktop', 'macos', 'aarch64'): 'native-thread-export',
            ('hermes-desktop', 'linux', 'x86_64'): 'renderer-dom'}
STEPS = {'launched', 'input-submitted', 'response-verified', 'tool-verified', 'error-recovered'}
COMMIT = re.compile(r'[0-9a-f]{40}\Z')
HASH = re.compile(r'[0-9a-f]{64}\Z')
VERSION = re.compile(r'[0-9]+(?:\.[0-9]+){2}(?:[-+][A-Za-z0-9.-]+)?\Z')


def matrix():
    return {'include': [dict(app=app, platform=platform, architecture=architecture,
                             runner=runner, backend=BACKENDS.get((app, platform, architecture), 'pending'))
                        for platform, architecture, runner in TARGETS for app in APPS]}


def cell(app, platform, architecture):
    matches = [item for item in matrix()['include']
               if (item['app'], item['platform'], item['architecture']) == (app, platform, architecture)]
    if len(matches) != 1:
        raise ValueError('target is outside the initial matrix')
    return matches[0]


def digest(path):
    path = Path(path)
    if path.is_symlink() or not path.is_file():
        raise ValueError('identity requires a regular file')
    value = hashlib.sha256()
    with path.open('rb') as source:
        for block in iter(lambda: source.read(1024 * 1024), b''):
            value.update(block)
    return value.hexdigest()


def envelope(app, platform, architecture, source_sha):
    selected = cell(app, platform, architecture)
    if not COMMIT.fullmatch(source_sha):
        raise ValueError('source identity is invalid')
    return dict(schemaVersion=1, suite='desktop-qualification', source='branch', sourceSha=source_sha,
                evidenceMode='deterministic', app=app, platform=platform, architecture=architecture,
                backend=selected['backend'], qualification='unqualified', outcome='blocked',
                reason='pending-backend' if selected['backend'] == 'pending' else 'not-run', checkerSha256=None, launcherSha256=None,
                realNanhSha256=None, frozenManifestSha256=None, preparedSha256=None,
                reportSha256=None, applicationSha256=None, upstreamRevision=None,
                upstreamArtifactSha256=None, appVersion=None, runtimeVersion=None,
                appCleanup=None, globalCleanup=None, probes=[])


def bounded_json(path, limit=1024 * 1024):
    path = Path(path)
    if path.is_symlink() or not path.is_file():
        raise ValueError('evidence must be a regular file')
    with path.open('rb') as source:
        raw = source.read(limit + 1)
    if len(raw) > limit:
        raise ValueError('evidence exceeds its bound')
    return json.loads(raw)


def reduce_report(*, app, platform, architecture, source_sha, model, frozen, prepared,
                  checker, launcher, real_nanh, report):
    result = envelope(app, platform, architecture, source_sha)
    if result['backend'] == 'pending':
        raise ValueError('native backend is not qualified for this cell')
    manifest = read_frozen_manifest(frozen, [app], platform, architecture, model)
    entry = manifest['apps'][0]
    if entry['status'] != 'frozen':
        result['reason'] = entry['reason']
        result['frozenManifestSha256'] = digest(frozen)
        return result
    result.update(checkerSha256=digest(checker), launcherSha256=digest(launcher),
                  realNanhSha256=digest(real_nanh), frozenManifestSha256=digest(frozen),
                  preparedSha256=digest(prepared), upstreamRevision=entry.get('revision'),
                  upstreamArtifactSha256=entry.get('digest', '').removeprefix('sha256:') or None)
    receipt = bounded_json(prepared)
    if (receipt.get('schemaVersion') != 2 or receipt.get('platform') != platform
            or receipt.get('architecture') != architecture
            or (receipt.get('checker') or {}).get('sha256') != result['checkerSha256']
            or (receipt.get('nanh') or {}).get('sha256') != result['launcherSha256']
            or (receipt.get('frozen') or {}).get('sha256') != result['frozenManifestSha256']
            or (receipt.get('frozen') or {}).get('model') != model):
        raise ValueError('prepared provenance differs')
    installed = [item for item in receipt.get('apps', []) if item.get('app') == app]
    if len(installed) != 1:
        raise ValueError('prepared app identity differs')
    executable = installed[0].get('executable') or {}
    application_hash = executable.get('sha256')
    if not isinstance(application_hash, str) or not HASH.fullmatch(application_hash):
        raise ValueError('installed application identity is missing')
    checked, report_digest = validated_report(Path(report), Path(checker))
    if (checked.get('schemaVersion') != 3 or checked.get('platform') != platform
            or checked.get('architecture') != architecture
            or (checked.get('model') is not None and checked.get('model') != model)
            or (checked.get('nanHarness') or {}).get('sha256') != result['launcherSha256']
            or len(checked.get('results', [])) != 1 or checked['results'][0].get('app') != app):
        raise ValueError('report identity differs from the cell')
    observed = checked['results'][0]
    if observed.get('appVersion') != entry['version']:
        raise ValueError('observed application version differs from frozen version')
    runtime = observed.get('runtimeVersion')
    if runtime is not None and (not isinstance(runtime, str) or not VERSION.fullmatch(runtime)):
        raise ValueError('runtime version is not closed')
    probes = observed.get('deterministic', [])
    public = [{key: probe[key] for key in ('status', 'reason', 'steps', 'inputMode', 'responseVerification')
               if key in probe} for probe in probes]
    result.update(reportSha256=report_digest, applicationSha256=application_hash,
                  appVersion=entry['version'], runtimeVersion=runtime,
                  appCleanup=observed.get('cleanup'), globalCleanup=checked.get('cleanup'), probes=public)
    # Semantic mechanisms are explicit, never inferred from visual/OCR success.
    semantic_pairs = {('native-clipboard-and-keyboard', 'native-thread-export')} if result['backend'] == 'native-thread-export' else {('renderer-dom-and-keyboard', 'renderer-dom')}
    accepted = (len(probes) == 3 and result['appCleanup'] == 'passed' and result['globalCleanup'] == 'passed'
                and all(probe.get('status') == 'passed' and len(probe.get('steps', [])) == 5
                        and set(probe.get('steps', [])) == STEPS
                        and (probe.get('inputMode'), probe.get('responseVerification')) in semantic_pairs for probe in probes))
    result.update(outcome='passed' if accepted else 'blocked',
                  qualification='deterministic-full' if accepted else 'unqualified',
                  reason=None if accepted else 'incomplete-acceptance')
    return result


def aggregate(directory, source_sha):
    records = [bounded_json(path) for path in Path(directory).glob('**/qualification.json')]
    expected = {(item['app'], item['platform'], item['architecture']) for item in matrix()['include']}
    observed = [(item.get('app'), item.get('platform'), item.get('architecture')) for item in records]
    if len(observed) != 15 or len(set(observed)) != 15 or set(observed) != expected:
        raise ValueError('qualification matrix is incomplete or duplicated')
    for item in records:
        template = envelope(item['app'], item['platform'], item['architecture'], source_sha)
        if item.get('qualification') not in {'unqualified', 'deterministic-full'} or item.get('outcome') not in {'blocked', 'passed'}:
            raise ValueError('qualification outcome is invalid')
        if item['qualification'] == 'deterministic-full' and (item['outcome'] != 'passed'
                or item['backend'] == 'pending' or item['appCleanup'] != 'passed'
                or item['globalCleanup'] != 'passed' or len(item['probes']) != 3
                or any(not isinstance(item[key], str) or not HASH.fullmatch(item[key]) for key in
                       ('checkerSha256', 'launcherSha256', 'realNanhSha256', 'frozenManifestSha256',
                        'preparedSha256', 'reportSha256', 'applicationSha256'))):
            raise ValueError('qualification success is missing evidence')
        if set(item) != set(template) or any(item[key] != template[key] for key in
                ('schemaVersion', 'suite', 'source', 'sourceSha', 'evidenceMode', 'backend')):
            raise ValueError('qualification provenance differs')
    return dict(schemaVersion=1, sourceSha=source_sha, qualification='deterministic-full'
                if all(item['qualification'] == 'deterministic-full' and item['outcome'] == 'passed'
                       for item in records) else 'incomplete', cells=records)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=('matrix', 'pending', 'reduce', 'aggregate'))
    for name in ('app', 'platform', 'architecture', 'source-sha', 'output', 'model', 'frozen', 'prepared',
                 'checker', 'launcher', 'real-nanh', 'report', 'directory'):
        parser.add_argument('--' + name)
    args = parser.parse_args()
    try:
        if args.command == 'matrix':
            print(json.dumps(matrix(), separators=(',', ':')))
            return
        required = (['directory', 'source_sha', 'output'] if args.command == 'aggregate' else
                    ['app', 'platform', 'architecture', 'source_sha', 'output'])
        if args.command == 'reduce':
            required += ['model', 'frozen', 'prepared', 'checker', 'launcher', 'real_nanh', 'report']
        if any(getattr(args, key) is None for key in required):
            raise ValueError('required cell evidence is missing')
        result = (aggregate(args.directory, args.source_sha) if args.command == 'aggregate' else
                  envelope(args.app, args.platform, args.architecture, args.source_sha)
                  if args.command == 'pending' else reduce_report(**{key: getattr(args, key) for key in required if key != 'output'}))
        destination = Path(args.output)
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(json.dumps(result, sort_keys=True) + '\n')
        destination.chmod(0o600)
        if args.command == 'aggregate' and result['qualification'] != 'deterministic-full':
            raise SystemExit('desktop qualification matrix remains incomplete')
    except (OSError, ValueError, TypeError, KeyError):
        raise SystemExit('desktop qualification evidence rejected') from None


if __name__ == '__main__':
    main()
