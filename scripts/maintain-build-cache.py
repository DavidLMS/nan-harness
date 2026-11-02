#!/usr/bin/env python3
"""Bound this checkout's disposable Cargo cache; preview unless --apply is given."""
import argparse
import os
from pathlib import Path
import subprocess
import time

GIB = 1024 ** 3


def inventory(target):
    """Measure allocated storage and recent writes without following symlinks."""
    size, newest = 0, target.stat().st_mtime
    seen = set()
    for directory, children, files in os.walk(target, followlinks=False):
        children[:] = [name for name in children if not (Path(directory) / name).is_symlink()]
        for name in files:
            item = (Path(directory) / name).lstat()
            identity = (item.st_dev, item.st_ino)
            if identity not in seen:
                size += getattr(item, "st_blocks", (item.st_size + 511) // 512) * 512
                seen.add(identity)
            newest = max(newest, item.st_mtime)
    return size, newest


def busy():
    # Fail closed when process visibility is unavailable. Schedule only in idle
    # periods; cargo clean must not overlap a newly started build.
    result = subprocess.run(["ps", "-axo", "comm="], check=True, capture_output=True, text=True)
    names = {Path(line.strip()).name for line in result.stdout.splitlines()}
    return bool(names & {"cargo", "rustc", "rustdoc", "clippy-driver"})


def maintain(repository, apply=False, max_gib=20, idle_days=14, retire=False):
    target = repository / "target"
    if target.is_symlink():
        raise ValueError("Refusing a symlinked target directory")
    if not target.exists():
        print("No local Cargo build cache.")
        return
    if not (target / "CACHEDIR.TAG").is_file():
        raise ValueError("Refusing target without Cargo's cache marker")
    if subprocess.check_output(["git", "ls-files", "--", "target"], cwd=repository).strip():
        raise ValueError("Refusing target containing tracked files")
    size, newest = inventory(target)
    age = max(0, time.time() - newest)
    eligible = retire or size > max_gib * GIB or age > idle_days * 86400
    print(f"Cargo cache: {size / GIB:.2f} GiB; last write {age / 86400:.1f} days ago.")
    if not eligible:
        print("Within the cache budget.")
        return
    if not apply:
        print("Would run cargo clean for this checkout; pass --apply to clean.")
        return
    if busy() or age < 3600:
        print("Deferred: compiler active or cache written within the last hour.")
        return
    # Explicitly constrain cleanup even when the caller has CARGO_TARGET_DIR set.
    subprocess.run(["cargo", "clean", "--locked", "--manifest-path",
                    str(repository / "Cargo.toml"), "--target-dir", str(target)],
                   cwd=repository, check=True,
                   env=dict(os.environ, CARGO_BUILD_BUILD_DIR=str(target)))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--apply", action="store_true")
    parser.add_argument("--retire", action="store_true", help="Clean before retiring this checkout")
    parser.add_argument("--max-gib", type=float, default=20)
    parser.add_argument("--idle-days", type=float, default=14)
    args = parser.parse_args()
    if args.max_gib <= 0 or args.idle_days <= 0:
        parser.error("Cache limits must be positive")
    repository = Path(__file__).resolve().parents[1]
    maintain(repository, args.apply, args.max_gib, args.idle_days, args.retire)


if __name__ == "__main__":
    main()
