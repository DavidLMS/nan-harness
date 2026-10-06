#!/usr/bin/env python3
"""Choose the latest closed receipt per cell, independently of its outcome."""
import argparse
from datetime import datetime
import hashlib
import io
import json
from pathlib import Path
import re
import sys
import subprocess
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'canary/actions'))
from desktop_qualification import matrix


def select(pages, run_id, source_sha):
    expected = {f"deterministic-qualification-{cell['app']}-{cell['platform']}"
                for cell in matrix(['pen-desktop'])['include']}
    latest = {}
    seen = set()
    if type(pages) is not list:
        raise ValueError('invalid artifact pages')
    for page in pages:
        if type(page) is not dict or type(page.get('artifacts')) is not list:
            raise ValueError('invalid artifact page')
        for artifact in page['artifacts']:
            if type(artifact) is not dict:
                raise ValueError('invalid artifact metadata')
            name = artifact.get('name')
            if type(name) is not str or name not in expected:
                continue
            identifier, created, run = artifact.get('id'), artifact.get('created_at'), artifact.get('workflow_run')
            if (type(identifier) is not int or identifier <= 0 or identifier in seen
                    or type(created) is not str or not re.fullmatch(r'\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z', created)
                    or type(run) is not dict or run.get('id') != run_id or run.get('head_sha') != source_sha
                    or type(artifact.get('expired')) is not bool):
                raise ValueError('invalid cell artifact identity')
            datetime.strptime(created, '%Y-%m-%dT%H:%M:%SZ')
            seen.add(identifier)
            previous = latest.get(name)
            if previous and previous['created_at'] == created:
                raise ValueError('ambiguous cell artifact creation time')
            if not previous or created > previous['created_at']:
                latest[name] = artifact
    if set(latest) != expected or any(value['expired'] for value in latest.values()):
        raise ValueError('latest cell artifacts unavailable')
    return [latest[name]['id'] for name in sorted(expected)]


def download(pages, run_id, source_sha, destination, fetch):
    identifiers = select(pages, run_id, source_sha)
    metadata = {item['id']: item for page in pages for item in page['artifacts']
                if item.get('id') in identifiers}
    destination.mkdir(parents=True, exist_ok=False)
    for identifier in identifiers:
        item = metadata[identifier]
        digest, size = item.get('digest'), item.get('size_in_bytes')
        if (type(digest) is not str or not re.fullmatch(r'sha256:[a-f0-9]{64}', digest)
                or type(size) is not int or not 0 < size <= 16 * 1024 * 1024):
            raise ValueError('invalid closed artifact digest or size')
        archive = fetch(identifier)
        if len(archive) != size or 'sha256:' + hashlib.sha256(archive).hexdigest() != digest:
            raise ValueError('closed artifact digest mismatch')
        with zipfile.ZipFile(io.BytesIO(archive)) as bundle:
            entries = bundle.infolist()
            if (len(entries) != 1 or entries[0].filename != 'qualification.json'
                    or not 0 < entries[0].file_size <= 4 * 1024 * 1024):
                raise ValueError('invalid closed artifact archive')
            payload = bundle.read(entries[0])
        target = destination / str(identifier)
        target.mkdir()
        (target / 'qualification.json').write_bytes(payload)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--metadata', type=Path, required=True)
    parser.add_argument('--run-id', type=int, required=True)
    parser.add_argument('--source-sha', required=True)
    parser.add_argument('--directory', type=Path)
    parser.add_argument('--repository')
    args = parser.parse_args()
    pages = json.loads(args.metadata.read_text())
    if args.directory is None:
        print('ids=' + ','.join(map(str, select(pages, args.run_id, args.source_sha))))
        return
    if not args.repository or not re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', args.repository):
        raise ValueError('invalid artifact repository')
    def fetch(identifier):
        return subprocess.run(
            ['gh', 'api', f'repos/{args.repository}/actions/artifacts/{identifier}/zip'],
            check=True, timeout=60, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL).stdout
    download(pages, args.run_id, args.source_sha, args.directory, fetch)


if __name__ == '__main__':
    main()
