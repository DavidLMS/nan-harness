#!/usr/bin/env python3
"""Plan credential-free desktop maintenance; source qualification is not release evidence."""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'canary/actions'))
from desktop_qualification import matrix
from desktop_suite import read_frozen_manifest

APPS = ('zed-desktop', 'chatgpt-desktop', 'claude-desktop', 'hermes-desktop')
TARGETS = {'linux': ('x86_64', 'linux-x64'), 'macos': ('aarch64', 'macos-arm64'),
           'windows': ('x86_64', 'windows-x64')}
SHA = re.compile(r'[a-f0-9]{40}\Z')
WORKFLOWS = ('desktop-check-qualification.yml', 'desktop-check-daily.yml')
STEPS = {'launched', 'input-submitted', 'response-verified', 'tool-verified', 'error-recovered'}


def baseline(platform):
    value = json.loads((ROOT / 'canary/desktop-baselines.json').read_text())
    expected = {(c['app'], c['platform'], c['architecture']) for c in matrix(['pen-desktop'])['include']}
    entries = value['cells']
    if (value['schemaVersion'] != 1 or len(entries) != 12
            or {(c['app'], c['platform'], c['architecture']) for c in entries} != expected):
        raise ValueError('invalid inspected baseline catalog')
    result = {}
    for entry in entries:
        if (not re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+', entry['version'])
                or ('digest' in entry) == ('revision' in entry)
                or 'digest' in entry and not re.fullmatch(r'sha256:[a-f0-9]{64}', entry['digest'])
                or 'revision' in entry and not SHA.fullmatch(entry['revision'])):
            raise ValueError('invalid inspected baseline identity')
        if entry['platform'] == platform:
            result[entry['app']] = {key: value for key, value in entry.items()
                                   if key not in {'platform', 'architecture'}} | {'status': 'frozen'}
    return result


def same_release(entry, expected):
    return (entry.get('status') == 'frozen' and entry.get('app') == expected['app']
            and entry.get('version') == expected['version']
            and all(entry.get(key) == expected.get(key) for key in ('digest', 'revision')))


def accepted(receipt, expected, platform, source_sha):
    return (type(receipt) is dict and receipt.get('sourceSha') == source_sha
            and receipt.get('source') == 'branch' and receipt.get('app') == expected['app']
            and receipt.get('platform') == platform and receipt.get('architecture') == TARGETS[platform][0]
            and receipt.get('schemaVersion') == 1 and receipt.get('suite') == 'desktop-qualification'
            and receipt.get('evidenceMode') == 'deterministic'
            and all(type(receipt.get(key)) is str and re.fullmatch(r'[a-f0-9]{64}', receipt[key]) for key in
                    ('checkerSha256', 'launcherSha256', 'realNanhSha256', 'frozenManifestSha256',
                     'preparedSha256', 'reportSha256', 'applicationSha256'))
            and receipt.get('qualification') == 'deterministic-full' and receipt.get('outcome') == 'passed'
            and receipt.get('appCleanup') == receipt.get('globalCleanup') == 'passed'
            and receipt.get('appVersion') == expected['version']
            and receipt.get('upstreamArtifactSha256') == (expected.get('digest', '').removeprefix('sha256:') or None)
            and receipt.get('upstreamRevision') == expected.get('revision')
            and type(receipt.get('probes')) is list and len(receipt['probes']) == 3
            and all(type(p) is dict and p.get('status') == 'passed'
                    and type(p.get('steps')) is list and len(p['steps']) == 5 and set(p['steps']) == STEPS for p in receipt['probes']))


def plan_cell(entry, expected, platform, source_sha, receipt=None, force=False):
    if entry is None or entry.get('status') != 'frozen':
        state = 'resolution-failed'
    elif not same_release(entry, expected):
        state = 'adaptation-required'
    elif not force and accepted(receipt, expected, platform, source_sha):
        state = 'already-qualified'
    else:
        state = 'qualification-pending'
    # Only bounded versions from the validated manifest leave private staging.
    return dict(app=expected['app'], platform=platform, architecture=TARGETS[platform][0],
                baselineVersion=expected['version'], observedVersion=entry.get('version') if entry else None,
                state=state)


def command(argv, limit=4 * 1024 * 1024):
    result = subprocess.run(argv, check=True, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                            timeout=180)
    if len(result.stdout) > limit:
        raise ValueError('bounded command output exceeded')
    return result.stdout


def api(endpoint):
    return json.loads(command(['gh', 'api', endpoint]))


def read_artifact(repository, artifact, source_sha, run_id):
    run = artifact.get('workflow_run', {})
    if (artifact.get('expired') is not False or run.get('id') != run_id
            or run.get('head_sha') != source_sha or type(artifact.get('id')) is not int
            or type(artifact.get('size_in_bytes')) is not int
            or not 0 < artifact['size_in_bytes'] <= 16 * 1024 * 1024):
        raise ValueError('invalid artifact identity')
    raw = command(['gh', 'api', f"repos/{repository}/actions/artifacts/{artifact['id']}/zip"],
                  16 * 1024 * 1024)
    if len(raw) != artifact['size_in_bytes'] or 'sha256:' + hashlib.sha256(raw).hexdigest() != artifact.get('digest'):
        raise ValueError('invalid artifact digest')
    with zipfile.ZipFile(io.BytesIO(raw)) as archive:
        entries = archive.infolist()
        if len(entries) != 1 or entries[0].filename != 'qualification.json' or not 0 < entries[0].file_size <= 4 * 1024 * 1024:
            raise ValueError('invalid closed artifact')
        return json.loads(archive.read(entries[0]))


def previous(repository, branch, source_sha, fetch=api, download=read_artifact):
    """A newer failed run invalidates older successes; never fish for a passing attempt."""
    candidates = []
    for workflow in WORKFLOWS:
        data = fetch(f'repos/{repository}/actions/workflows/{workflow}/runs?per_page=30')
        candidates.extend(run for run in data['workflow_runs'] if run.get('status') == 'completed'
                          and run.get('head_sha') == source_sha and run.get('head_branch') == branch
                          and run.get('event') in {'schedule', 'workflow_dispatch'}
                          and run.get('path') == '.github/workflows/' + workflow)
    if not candidates:
        return {}
    records = {}
    names = {f"deterministic-qualification-{c['app']}-{c['platform']}": (c['app'], c['platform'])
             for c in matrix(['pen-desktop'])['include']}
    for run in sorted(candidates, key=lambda value: value['created_at'], reverse=True)[:10]:
        if run.get('conclusion') != 'success':
            break
        artifacts = []
        for page in range(1, 5):
            batch = fetch(f"repos/{repository}/actions/runs/{run['id']}/artifacts?per_page=100&page={page}")
            artifacts.extend(batch['artifacts'])
            if len(batch['artifacts']) < 100:
                break
        selected = {}
        for artifact in artifacts:
            name = artifact.get('name')
            if name not in names or names[name] in records:
                continue
            held = selected.get(name)
            if held and held['created_at'] == artifact['created_at']:
                raise ValueError('ambiguous artifact attempt')
            if not held or artifact['created_at'] > held['created_at']:
                selected[name] = artifact
        records.update({names[name]: download(repository, artifact, source_sha, run['id'])
                        for name, artifact in selected.items()})
        if len(records) == len(names):
            break
    return records


def resolve(checker, platform, directory):
    architecture, target = TARGETS[platform]
    manifest = directory / (platform + '.json')
    argv = [str(checker), 'resolve', '--target', target, '--model', 'qwen3.6',
            '--artifacts', str(directory / (platform + '-artifacts')), '--output', str(manifest)]
    for app in APPS:
        argv += ['--app', app]
    # No GitHub or provider credentials enter the resolver or staged vendor bytes.
    env = {k: v for k, v in os.environ.items() if k not in {'GH_TOKEN', 'GITHUB_TOKEN', 'NAN_API_KEY'}}
    subprocess.run(argv, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                   check=True, timeout=900)
    return read_frozen_manifest(manifest, APPS, platform, architecture, 'qwen3.6')['apps']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--checker', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--force', action='store_true')
    args = parser.parse_args()
    repository, branch, source_sha = (os.environ[k] for k in ('GITHUB_REPOSITORY', 'DEFAULT_BRANCH', 'GITHUB_SHA'))
    if not SHA.fullmatch(source_sha) or not re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', repository):
        raise ValueError('invalid repository identity')
    # Unavailable evidence causes fresh qualification, never a false cached pass.
    try:
        receipts = previous(repository, branch, source_sha)
    except (ValueError, KeyError, OSError, subprocess.SubprocessError, zipfile.BadZipFile):
        receipts = {}
    cells = []
    with tempfile.TemporaryDirectory(prefix='desktop-daily-') as temporary:
        for platform in TARGETS:
            expected = baseline(platform)
            try:
                entries = {item['app']: item for item in resolve(args.checker, platform, Path(temporary))}
            except (ValueError, OSError, subprocess.SubprocessError):
                entries = {}
            cells.extend(plan_cell(entries.get(app), expected[app], platform, source_sha,
                                   receipts.get((app, platform)), args.force) for app in APPS)
    selected = [c['app'] + '/' + c['platform'] for c in cells if c['state'] == 'qualification-pending']
    report = dict(schemaVersion=1, sourceSha=source_sha, evidenceMode='source-deterministic',
                  excludedApps=['pen-desktop'], cells=cells)
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    with Path(os.environ['GITHUB_OUTPUT']).open('a') as output:
        output.write('cells=' + json.dumps(selected, separators=(',', ':')) + '\n')
        output.write(f"has_cells={str(bool(selected)).lower()}\n")
        output.write(f"resolution_failed={str(any(c['state']=='resolution-failed' for c in cells)).lower()}\n")
    with Path(os.environ['GITHUB_STEP_SUMMARY']).open('a') as output:
        output.write('Desktop source qualification; no release/feed publication. Pen excluded.\n\n')
        output.write('| App | Platform | Baseline | Observed | State |\n|---|---|---|---|---|\n')
        for cell in cells:
            output.write('| ' + ' | '.join(str(cell[k] or 'unknown') for k in
                         ('app', 'platform', 'baselineVersion', 'observedVersion', 'state')) + ' |\n')


if __name__ == '__main__':
    main()
