#!/usr/bin/env python3
"""Choose the latest closed receipt per cell, independently of its outcome."""
import argparse
from datetime import datetime
import json
from pathlib import Path
import re
import sys

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


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--metadata', type=Path, required=True)
    parser.add_argument('--run-id', type=int, required=True)
    parser.add_argument('--source-sha', required=True)
    args = parser.parse_args()
    identifiers = select(json.loads(args.metadata.read_text()), args.run_id, args.source_sha)
    print('ids=' + ','.join(map(str, identifiers)))


if __name__ == '__main__':
    main()
