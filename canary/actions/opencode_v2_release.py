#!/usr/bin/env python3
"""Qualify V2 separately without replacing the historical V1 release evidence."""
import argparse
import json
import re
import sys
from pathlib import Path
from types import SimpleNamespace

import cell
import release_gate

VERSION = "2.0.27"
ASSETS = {
    "linux": ("aarch64", "aarch64-unknown-linux-musl"),
    "macos": ("aarch64", "aarch64-apple-darwin"),
    "windows": ("x86_64", "x86_64-pc-windows-msvc.exe"),
}


def qualify(args):
    if not all(re.fullmatch(r"[0-9a-f]{40}", value) for value in
               (args.tag_commit, args.workflow_commit)):
        raise ValueError("invalid source identity")
    architecture, suffix = ASSETS[args.system]
    if cell.detected_identity() != (args.system, architecture):
        raise ValueError("runner identity mismatch")
    # prepare attests this manifest; validate its downloaded bytes again locally.
    release_gate._asset_entries(args.assets)
    binary = args.assets / ("nan-harness-" + suffix)
    canary = args.assets / ("nan-harness-canary-" + suffix)
    binary.chmod(0o755)
    canary.chmod(0o755)
    cell.ensure_private_directory(args.directory, reusable=True)
    probe = SimpleNamespace(directory=args.directory, binary=binary, canary=canary,
                            model=args.model, harness="opencode", harness_version=VERSION,
                            trigger="release")
    state = {"harness": {}}
    cell.install(probe, state)
    if state["harness"].get("version") != VERSION:
        raise ValueError("V2 installation version mismatch")
    if args.mode == "live" and cell.live(probe, {}) != 1:
        raise ValueError("V2 live qualification incomplete")
    return {"schemaVersion": 1, "generation": 2, "harnessVersion": VERSION,
            "tagCommit": args.tag_commit, "workflowCommit": args.workflow_commit,
            "binarySha256": cell.digest(binary), "system": args.system,
            "architecture": architecture, "model": args.model,
            "mode": args.mode, "outcome": "passed"}


def main():
    parser = argparse.ArgumentParser()
    for name in ("assets", "directory", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    for name in ("tag-commit", "workflow-commit", "model"):
        parser.add_argument("--" + name, required=True)
    parser.add_argument("--system", choices=ASSETS, required=True)
    parser.add_argument("--mode", choices=("live", "deterministic"), required=True)
    args = parser.parse_args()
    try:
        report = qualify(args)
    except (OSError, RuntimeError, ValueError):
        # Never print subprocess output, private paths, prompts or credentials.
        print("OpenCode V2 release qualification failed", file=sys.stderr)
        return 1
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
