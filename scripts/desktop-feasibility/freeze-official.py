#!/usr/bin/env python3
"""Freeze public upstream metadata using the hosted job's read-only GitHub token.

Credentials are scoped to metadata acquisition; application installers consume
only the frozen manifest after the token has been removed from their environment.
"""
import argparse
import base64
import json
import os
from pathlib import Path
import re
import subprocess
import time

VERSION = re.compile(r'(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\Z')
SHA = re.compile(r'[0-9a-f]{40}\Z')
DIGEST = re.compile(r'sha256:[0-9a-f]{64}\Z')


def metadata(endpoint):
    for attempt in range(3):
        completed = subprocess.run(['gh', 'api', '--hostname', 'github.com', endpoint],
                                   stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                   timeout=20, check=False)
        if completed.returncode == 0:
            break
        if attempt < 2:
            time.sleep(2)
    if completed.returncode or len(completed.stdout) > 4 * 1024 * 1024:
        raise ValueError('official-metadata-fetch-failed')
    value = json.loads(completed.stdout)
    if not isinstance(value, dict):
        raise ValueError('official-metadata-shape-invalid')
    return value


def release_tag(value):
    if value.get('draft') is not False or value.get('prerelease') is not False:
        raise ValueError('official-release-channel-invalid')
    tag = value.get('tag_name', '')
    if not isinstance(tag, str) or not tag.startswith('v') or not VERSION.fullmatch(tag[1:]):
        raise ValueError('official-release-version-invalid')
    return tag


def release_endpoint(repository, tag):
    if tag is None:
        return f'repos/{repository}/releases/latest'
    if not isinstance(tag, str) or not tag.startswith('v') or not VERSION.fullmatch(tag[1:]):
        raise ValueError('official-release-version-invalid')
    return f'repos/{repository}/releases/tags/{tag}'


def freeze_zed(fetch=metadata, tag=None, platform='macos'):
    repository = 'zed-industries/zed'
    requested = tag
    release = fetch(release_endpoint(repository, tag))
    tag = release_tag(release)
    if requested is not None and tag != requested:
        raise ValueError('official-release-tag-mismatch')
    name = {'macos': 'Zed-aarch64.dmg', 'linux': 'zed-linux-x86_64.tar.gz',
            'windows': 'Zed-x86_64.exe'}[platform]
    format_name = {'macos': 'dmg', 'linux': 'tar-gz', 'windows': 'windows-setup'}[platform]
    url = f'https://github.com/{repository}/releases/download/{tag}/{name}'
    assets = [a for a in release.get('assets', []) if a.get('name') == name]
    if len(assets) != 1 or assets[0].get('browser_download_url') != url:
        raise ValueError('official-asset-identity-invalid')
    digest = assets[0].get('digest', '')
    if not isinstance(digest, str) or not DIGEST.fullmatch(digest):
        raise ValueError('official-asset-digest-invalid')
    # Existing GithubAsset policy requires staged=false. The checker downloads
    # this immutable public URL and verifies this digest during preparation.
    return dict(status='frozen', app='zed-desktop', version=tag[1:],
                channel=f'github-release:{repository}', url=url, format=format_name,
                digest=digest, staged=False, installer='external' if platform == 'windows' else 'checker')


def git_commit(value):
    obj = value.get('object', {})
    kind, sha = obj.get('type'), obj.get('sha', '')
    if kind not in {'tag', 'commit'} or not isinstance(sha, str) or not SHA.fullmatch(sha):
        raise ValueError('official-source-object-invalid')
    return kind, sha


def freeze_hermes(fetch=metadata, tag=None):
    repository = 'NousResearch/hermes-agent'
    requested = tag
    tag = release_tag(fetch(release_endpoint(repository, tag)))
    if requested is not None and tag != requested:
        raise ValueError('official-release-tag-mismatch')
    kind, revision = git_commit(fetch(f'repos/{repository}/git/ref/tags/{tag}'))
    if kind == 'tag':
        kind, revision = git_commit(fetch(f'repos/{repository}/git/tags/{revision}'))
    if kind != 'commit':
        raise ValueError('official-source-commit-invalid')
    content = fetch(f'repos/{repository}/contents/apps/desktop/package.json?ref={revision}')
    if content.get('encoding') != 'base64' or content.get('type') != 'file' or content.get('path') != 'apps/desktop/package.json':
        raise ValueError('official-source-package-invalid')
    package = json.loads(base64.b64decode(''.join(content['content'].split()), validate=True))
    version = package.get('version', '')
    if not isinstance(version, str) or not VERSION.fullmatch(version):
        raise ValueError('official-source-version-invalid')
    return dict(status='frozen', app='hermes-desktop', version=version,
                channel=f'github-source:{repository}', url=f'https://github.com/{repository}.git',
                format='source', revision=revision, staged=False, installer='external')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--app', choices=['zed-desktop', 'hermes-desktop'], required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--platform', choices=['linux', 'macos', 'windows'])
    parser.add_argument('--tag')
    parser.add_argument('--expected-revision')
    args = parser.parse_args()
    if os.environ.get('GITHUB_ACTIONS') != 'true' or os.environ.get('RUNNER_ENVIRONMENT') != 'github-hosted':
        raise ValueError('disposable-hosted-runner-required')
    if not os.environ.get('GH_TOKEN') or os.environ.get('NAN_API_KEY'):
        raise ValueError('metadata-only-credentials-required')
    platform = args.platform or ('macos' if args.app == 'zed-desktop' else 'linux')
    entry = freeze_zed(tag=args.tag, platform=platform) if args.app == 'zed-desktop' else freeze_hermes(tag=args.tag)
    if args.expected_revision is not None and (not SHA.fullmatch(args.expected_revision) or entry.get('revision') != args.expected_revision):
        raise ValueError('official-source-revision-mismatch')
    platform = args.platform or ('macos' if args.app == 'zed-desktop' else 'linux')
    architecture = 'aarch64' if platform == 'macos' else 'x86_64'
    manifest = dict(schemaVersion=1, suite='desktop', platform=platform,
                    architecture=architecture, model='qwen3.6', apps=[entry])
    args.output.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    descriptor = os.open(args.output, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, 'w') as outgoing:
        json.dump(manifest, outgoing)
        outgoing.write('\n')


if __name__ == '__main__':
    try:
        main()
    except ValueError as error:
        category = str(error) if re.fullmatch(r'official-[a-z-]+', str(error)) else 'official-metadata-invalid'
        raise SystemExit(category) from None
    except (KeyError, OSError, subprocess.TimeoutExpired):
        raise SystemExit('official-metadata-io-failed') from None
