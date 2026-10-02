#!/usr/bin/env python3
"""Stage the immutable official Windows export decoder in a private runner directory."""
import argparse
import hashlib
import io
import os
from pathlib import Path
import sys
import urllib.request
import zipfile

URL = 'https://github.com/facebook/zstd/releases/download/v1.5.7/zstd-v1.5.7-win64.zip'
SHA256 = 'acb4e8111511749dc7a3ebedca9b04190e37a17afeb73f55d4425dbf0b90fad9'
ENTRY = 'zstd-v1.5.7-win64/zstd.exe'
LIMIT = 10 * 1024 * 1024


def extract(data):
    if len(data) > LIMIT or hashlib.sha256(data).hexdigest() != SHA256:
        raise ValueError('decoder archive identity differs')
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        entries = [entry for entry in archive.infolist() if entry.filename == ENTRY]
        if len(entries) != 1 or entries[0].file_size > 2 * 1024 * 1024:
            raise ValueError('decoder entry is not bounded and unique')
        return archive.read(entries[0])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if (sys.platform != 'win32' or os.environ.get('GITHUB_ACTIONS') != 'true'
            or os.environ.get('RUNNER_ENVIRONMENT') != 'github-hosted'
            or any(os.environ.get(key) for key in ['NAN_API_KEY', 'GH_TOKEN', 'GITHUB_TOKEN'])):
        raise ValueError('credential-free disposable Windows runner required')
    if not args.output.is_absolute() or args.output.exists() or args.output.is_symlink():
        raise ValueError('decoder destination must be new')
    with urllib.request.urlopen(URL, timeout=30) as response:
        data = response.read(LIMIT + 1)
    payload = extract(data)
    with args.output.open('xb') as output:
        output.write(payload)


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, zipfile.BadZipFile):
        raise SystemExit('fixed Windows decoder preparation failed') from None
