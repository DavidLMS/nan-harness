#!/usr/bin/env python3
"""Prepare the inspected official Codex baseline, without resolving latest."""
import argparse
import hashlib
import os
from pathlib import Path
import subprocess
import sys
import tempfile

from codex_release import CODEX_PROJECT_RELEASES, CODEX_PROJECT_VERSIONS

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'canary/actions'))
from cell import ensure_private_directory, write_json
from desktop_suite import read_frozen_manifest


def manifest(platform):
    version = CODEX_PROJECT_VERSIONS[platform]
    base = 'https://persistent.oaistatic.com/codex-app-prod/'
    if platform == 'linux':
        url = f'{base}linux/deb/pool/main/c/chatgpt/chatgpt_{version}_amd64.deb'
        channel, package, staged, installer = f'apt:{base}linux/deb/', 'deb', False, 'checker'
    elif platform == 'macos':
        url = f'{base}ChatGPT-darwin-arm64-{version}.zip'
        channel, package, staged, installer = f'sparkle:{base}appcast.xml', 'zip', True, 'checker'
    else:
        url = f'{base}ChatGPT-x64.msix'
        channel, package, staged, installer = f'official-latest:{url}', 'msix', True, 'external'
    entry = dict(status='frozen', app='chatgpt-desktop', version=version,
                 channel=channel, url=url, format=package,
                 digest='sha256:' + CODEX_PROJECT_RELEASES[platform][0],
                 staged=staged, installer=installer)
    return dict(schemaVersion=1, suite='desktop', platform=platform,
                architecture='aarch64' if platform == 'macos' else 'x86_64',
                model='qwen3.6', apps=[entry])


def download(url, output):
    subprocess.run(['curl', '--fail', '--silent', '--show-error', '--location',
                    '--proto', '=https', '--proto-redir', '=https',
                    '--max-time', '180', '--max-filesize', str(1024 * 1024 * 1024),
                    '--output', str(output), url], check=True, timeout=190,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def stage(entry, directory, fetch=download):
    ensure_private_directory(directory, reusable=True)
    expected = entry['digest'].removeprefix('sha256:')
    destination = directory / ('chatgpt-desktop-' + expected)
    # Exclusive temporary storage; a mismatch never becomes an installable file.
    with tempfile.TemporaryDirectory(prefix='codex-stage-', dir=directory) as temporary:
        partial = Path(temporary) / 'artifact'
        fetch(entry['url'], partial)
        with partial.open('rb') as stream:
            actual = hashlib.file_digest(stream, 'sha256').hexdigest()
        if actual != expected:
            raise ValueError('inspected-codex-artifact-mismatch')
        with destination.open('xb') as output, partial.open('rb') as source:
            os.chmod(destination, 0o600)
            while chunk := source.read(1024 * 1024):
                output.write(chunk)
    return destination


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--platform', choices=CODEX_PROJECT_RELEASES, required=True)
    parser.add_argument('--artifacts', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if (os.environ.get('GITHUB_ACTIONS') != 'true'
            or os.environ.get('RUNNER_ENVIRONMENT') != 'github-hosted'
            or os.environ.get('NAN_API_KEY')):
        raise ValueError('disposable-hosted-runner-required')
    value = manifest(args.platform)
    entry = value['apps'][0]
    if entry['staged']:
        stage(entry, args.artifacts)
    ensure_private_directory(args.output.parent, reusable=True)
    write_json(args.output, value)
    read_frozen_manifest(args.output, ['chatgpt-desktop'], args.platform,
                         value['architecture'], 'qwen3.6')


if __name__ == '__main__':
    try:
        main()
    except (ValueError, OSError, subprocess.SubprocessError):
        raise SystemExit('inspected-codex-freeze-failed') from None
