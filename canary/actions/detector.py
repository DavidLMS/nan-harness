#!/usr/bin/env python3
"""Resolve detector inputs without provider credentials or publication writes."""

import argparse
import json
from pathlib import Path
import re
import subprocess
import sys

from evidence import validate
from publication import remote_commit
from selection import resolve_model, select_names, select_suite
from state import Store, StateError


def feed_asset(assets):
    name = "compatibility-v5.json"
    current = [asset for asset in assets if asset.get("name") == name]
    if len(current) > 1:
        raise StateError("ambiguous compatibility feed")
    if current:
        return name
    backups = [asset for asset in assets if asset.get("name", "").startswith(name + ".backup.")]
    return max(backups, key=lambda asset: (asset["created_at"], asset["name"]))["name"] if backups else None


def select_detector(suites, platforms, harnesses, desktop_harnesses, mode, model):
    """Select independent native suites before reserving any worker runner."""
    active = select_names(suites, ("cli", "desktop"), "suites")
    model = resolve_model(model)
    selections = {suite: select_suite(suite, platforms,
                                     harnesses if suite == "cli" else desktop_harnesses,
                                     mode, model) for suite in active}
    desktop = selections.get("desktop", {})
    return {
        "matrix": json.dumps({"include": selections.get("cli", {}).get("platforms", [])}),
        "cli": str("cli" in active).lower(), "desktop": str("desktop" in active).lower(),
        "desktop_platforms": ",".join(job["system"] for job in desktop.get("platforms", [])),
        "desktop_harnesses": ",".join(desktop.get("harnesses", [])),
        "model": model, "mode": mode,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("select", "feed"))
    parser.add_argument("--repository", required=True)
    parser.add_argument("--platforms", default="all")
    parser.add_argument("--harnesses", default="all")
    parser.add_argument("--desktop-harnesses", default="all")
    parser.add_argument("--suites", default="cli")
    parser.add_argument("--mode", default="deterministic")
    parser.add_argument("--model", default="")
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--validator", type=Path)
    args = parser.parse_args()
    try:
        store = Store(args.repository)
        if args.action == "select":
            release = store.call("releases/latest")
            tag = release["tag_name"]
            if (not re.fullmatch(r"v(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)", tag)
                    or release["draft"] or release["prerelease"]):
                raise StateError("detector needs a stable published release")
            values = select_detector(args.suites, args.platforms, args.harnesses,
                                     args.desktop_harnesses, args.mode, args.model)
            values.update(tag=tag, commit=remote_commit(store, tag))
            with args.output.open("a") as output:
                for key, value in values.items():
                    output.write(key + "=" + value + "\n")
        else:
            if args.validator is None or args.output.exists():
                raise StateError("fresh output and trusted validator required")
            # A failed API call is not an absent feed. A positively observed
            # older-schema release simply has no model-scoped observations yet.
            assets = store.call("releases/tags/compatibility")["assets"]
            name = feed_asset(assets)
            if name:
                validate(["gh", "release", "download", "compatibility", "--repo", args.repository,
                          "--pattern", name, "--output", args.output])
            else:
                validate([args.validator, "hosted-compatibility-feed", args.output])
            validate([args.validator, "validate-hosted-compatibility-feed", args.output])
    except (StateError, OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError):
        print("Detector input resolution failed; no compatibility state was changed.", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
