#!/usr/bin/env python3
"""Bind closed desktop qualification to independently attested release bytes."""
import argparse
import json
import os
from pathlib import Path
import sys
import subprocess

from desktop_maintenance import APPS, TARGETS, accepted, baseline
from desktop_qualification import bounded_json, digest
from desktop_release import stage
from desktop_suite import COMMIT, TAG
from state import Store, StateError


def validate_selection(tag, commit):
    if not tag and not commit:
        return False
    if not TAG.fullmatch(tag) or not COMMIT.fullmatch(commit):
        raise ValueError('release requires an exact tag and commit')
    return True


def bind(value, source_sha, tag, commit, binaries):
    if not validate_selection(tag, commit) or not COMMIT.fullmatch(source_sha):
        raise ValueError('release binding identity is invalid')
    expected = {(app, platform) for app in APPS for platform in TARGETS}
    cells = value.get('cells', [])
    observed = [(cell.get('app'), cell.get('platform')) for cell in cells]
    if (value.get('schemaVersion') != 1 or value.get('sourceSha') != source_sha
            or value.get('qualification') != 'deterministic-full'
            or value.get('excludedApps') != ['pen-desktop']
            or len(observed) != len(expected) or set(observed) != expected
            or set(binaries) != set(TARGETS)):
        raise ValueError('release matrix is incomplete or differs from the trusted source')
    assets = {platform: dict(asset=path.name, sha256=digest(path))
              for platform, path in binaries.items()}
    for cell in cells:
        platform = cell['platform']
        if (not accepted(cell, baseline(platform)[cell['app']], platform, source_sha)
                or cell['realNanhSha256'] != assets[platform]['sha256']):
            raise ValueError('desktop cell does not qualify the exact release binary')
    return {**value, 'release': dict(tag=tag, commit=commit, assets=assets,
                                    verification='sha256-and-github-attestation')}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=('validate-selection', 'bind'))
    parser.add_argument('--matrix', type=Path)
    parser.add_argument('--directory', type=Path)
    args = parser.parse_args()
    tag, commit = os.environ.get('RELEASE_TAG', ''), os.environ.get('RELEASE_COMMIT', '')
    try:
        selected = validate_selection(tag, commit)
        if args.command == 'validate-selection':
            return 0
        if not selected or args.matrix is None or args.directory is None:
            raise ValueError('missing release binding inputs')
        store = Store(os.environ['GITHUB_REPOSITORY'])
        binaries = {platform: stage(store, tag, commit, platform,
                                    'aarch64' if platform == 'macos' else 'x86_64',
                                    args.directory / platform)
                    for platform in TARGETS}
        result = bind(bounded_json(args.matrix), os.environ['GITHUB_SHA'], tag, commit, binaries)
        args.matrix.write_text(json.dumps(result, indent=2) + '\n')
    except (OSError, ValueError, StateError, subprocess.SubprocessError):
        print('Desktop release binding failed; publication is blocked.', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
