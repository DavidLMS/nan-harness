#!/usr/bin/env python3
"""Stage only an exact attested native nanh binary; build probe tools separately."""

import argparse
import os
from pathlib import Path
import subprocess
import sys

from desktop_suite import validate_release_assets
from publication import remote_commit
from selection import native_platform
from state import Store, StateError


def execute(argv):
    result = subprocess.run([str(value) for value in argv], stdout=subprocess.DEVNULL,
                            stderr=subprocess.DEVNULL, check=False, timeout=180)
    if result.returncode:
        raise StateError("release asset verification failed")


def stage(store, tag, commit, system, architecture, directory, command=execute):
    platform = native_platform("desktop", system)
    if system not in ("linux", "macos", "windows") or platform["architecture"] != architecture:
        raise StateError("unexpected desktop native target")
    if remote_commit(store, tag) != commit:
        raise StateError("release tag changed after selection")
    suffix = ".exe" if system == "windows" else ""
    asset = f"nan-harness-{architecture}-{platform['target']}{suffix}"
    if directory.is_symlink() or directory.exists():
        raise StateError("release staging directory already exists")
    directory.mkdir(mode=0o700, parents=True)
    command(["gh", "release", "download", tag, "--repo", store.repository,
             "--pattern", "SHA256SUMS", "--pattern", asset, "--dir", directory])
    binary = directory / asset
    attestation = validate_release_assets(directory / "SHA256SUMS", {asset: binary}, tag, commit)
    command([*attestation, "--repo", store.repository,
             "--signer-workflow", store.repository + "/.github/workflows/release.yml",
             "--deny-self-hosted-runners"])
    binary.chmod(0o700)
    return binary


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--platform", required=True)
    parser.add_argument("--architecture", required=True)
    parser.add_argument("--directory", type=Path, required=True)
    args = parser.parse_args()
    try:
        binary = stage(Store(args.repository), args.tag, args.commit, args.platform,
                       args.architecture, args.directory)
        with Path(os.environ["GITHUB_ENV"]).open("a") as output:
            output.write(f"NANH_PATH={binary}\nSOURCE_SHA={args.commit}\n")
    except (StateError, OSError, ValueError, subprocess.SubprocessError):
        print("Exact release staging failed; no desktop application was started.", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
