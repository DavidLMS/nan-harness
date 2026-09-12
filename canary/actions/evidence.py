#!/usr/bin/env python3
"""Select exact pending tuples and pack validated, data-only hosted evidence."""

import argparse
import datetime
import json
from pathlib import Path
import subprocess
import sys
import tempfile

from cell import digest, write_json
from hosted import should_probe
from hosted_publication import validate_bundle
from provenance import bind_report, specification_digest
from state import StateError, canonical


def read_json(path, limit):
    if path.is_symlink() or not path.is_file() or path.stat().st_size > limit:
        raise StateError("invalid bounded evidence file")
    raw = path.read_bytes()
    if len(raw) > limit:
        raise StateError("evidence file exceeds its limit")
    return json.loads(raw)


def validate(command):
    result = subprocess.run([str(part) for part in command], stdout=subprocess.DEVNULL,
                            stderr=subprocess.DEVNULL, timeout=60, check=False)
    if result.returncode:
        raise StateError("trusted evidence validation failed")


def pending_harnesses(feed, frozen, version, binary_sha, spec, mode, now):
    """The caller validates the feed and frozen manifest before selection."""
    selected = []
    for item in frozen:
        target = {"suite": "cli", "id": item.harness, "platform": item.system,
                  "architecture": item.architecture, "harnessVersion": item.version,
                  "nanHarnessSha256": binary_sha, "specSha256": spec}
        targets = [target]
        if mode == "live":
            targets.append({**target, "model": item.model})
        if any(should_probe(feed, version, candidate, now) for candidate in targets):
            selected.append(item)
    return selected


def pending_desktop(feed, manifest, version, binary_sha, spec, mode, now):
    """Match only verified frozen identities; unresolved apps remain diagnostic."""
    selected = []
    for item in manifest["apps"]:
        if item["status"] != "frozen":
            selected.append(item)
            continue
        target = {"suite": "desktop", "id": item["app"], "platform": manifest["platform"],
                  "architecture": manifest["architecture"], "harnessVersion": item["version"],
                  "nanHarnessSha256": binary_sha, "specSha256": spec}
        if item.get("runtimeVersion") is not None:
            target["runtimeVersion"] = item["runtimeVersion"]
        # An unknown runtime cannot match historical evidence for a known one.
        # Never borrow a runtime version from the feed to manufacture a match.
        targets = [target]
        if mode == "live":
            targets.append({**target, "model": manifest["model"]})
        if any(should_probe(feed, version, candidate, now) for candidate in targets):
            selected.append(item)
    return selected


def pack_reports(paths, validator, suite, platform, architecture, source_commit,
                 release_tag, release_commit, model, binary_sha, spec, command=validate):
    bundle = {"schemaVersion": 1, "suite": suite, "platform": platform,
              "architecture": architecture, "sourceCommit": source_commit,
              "releaseTag": release_tag, "releaseCommit": release_commit,
              "model": model, "specSha256": spec, "reports": []}
    if not 1 <= len(paths) <= 30 or len(set(paths)) != len(paths):
        raise StateError("expected distinct bounded report inputs")
    expected = {"version": release_tag[1:], "binarySha256": binary_sha,
                "platform": platform, "architecture": architecture, "model": model}
    for path in paths:
        report = read_json(path, 65536)
        # Validate the exact captured bytes, not a mutable original report path.
        with tempfile.TemporaryDirectory(prefix="nan-evidence-") as temporary:
            snapshot = Path(temporary) / "report.json"
            write_json(snapshot, report)
            command([validator, "validate-report", snapshot])
        bind_report(report, suite, expected)
        if suite == "cli" and report["nanHarness"].get("source") != "commit:" + release_commit:
            raise StateError("branch evidence cannot certify a release")
        bundle["reports"].append(report)
    validate_bundle(bundle, source_commit)
    if len(canonical(bundle)) > 2_000_000:
        raise StateError("evidence envelope exceeds its limit")
    return bundle


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="action", required=True)
    selectors = [sub.add_parser("select-cli"), sub.add_parser("select-desktop")]
    for select in selectors:
        select.add_argument("--manifest", required=True, type=Path)
        select.add_argument("--feed", required=True, type=Path)
        select.add_argument("--feed-validator", required=True, type=Path)
        select.add_argument("--harnesses", required=True)
        select.add_argument("--mode", choices=("deterministic", "live"), required=True)
    pack = sub.add_parser("pack")
    pack.add_argument("--suite", choices=("cli", "desktop"), required=True)
    pack.add_argument("--reports", required=True, type=Path)
    pack.add_argument("--validator", required=True, type=Path)
    pack.add_argument("--release-commit", required=True)
    for command_parser in (*selectors, pack):
        command_parser.add_argument("--platform", required=True)
        command_parser.add_argument("--architecture", required=True)
        command_parser.add_argument("--model", required=True)
        command_parser.add_argument("--source-commit", required=True)
        command_parser.add_argument("--release-tag", required=True)
        command_parser.add_argument("--binary", required=True, type=Path)
        command_parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    try:
        suite = getattr(args, "suite", "desktop" if args.action == "select-desktop" else "cli")
        spec = specification_digest(root, args.source_commit, suite)
        binary_sha = digest(args.binary)
        if args.action.startswith("select-"):
            validate([args.feed_validator, "validate-hosted-compatibility-feed", args.feed])
            feed = read_json(args.feed, 2_000_000)
            now = datetime.datetime.now(datetime.timezone.utc)
        if args.action == "select-cli":
            import importlib.util
            module_spec = importlib.util.spec_from_file_location("cli_suite", root / "canary/actions/cli-suite.py")
            module = importlib.util.module_from_spec(module_spec)
            sys.modules[module_spec.name] = module
            module_spec.loader.exec_module(module)
            requested = args.harnesses.split(",")
            frozen = module.read_frozen_manifest(args.manifest, requested,
                                                 args.platform, args.architecture, args.model)
            unresolved = module.read_unresolved_manifest(args.manifest, requested,
                                                        args.platform, args.architecture, args.model)
            selected = pending_harnesses(feed, frozen, args.release_tag[1:], binary_sha, spec,
                                        args.mode, now)
            write_json(args.output, {"harnesses": [item.as_dict() for item in selected],
                                     "unresolved": [item.as_dict() for item in unresolved]})
            pending = {item.harness for item in selected} | {item.harness for item in unresolved}
            print(",".join(name for name in requested if name in pending))
        elif args.action == "select-desktop":
            from desktop_suite import read_frozen_manifest
            manifest = read_frozen_manifest(args.manifest, args.harnesses.split(","),
                                            args.platform, args.architecture, args.model)
            selected = pending_desktop(feed, manifest, args.release_tag[1:], binary_sha, spec, args.mode, now)
            write_json(args.output, {**manifest, "apps": selected})
            print(",".join(item["app"] for item in selected))
        else:
            paths = sorted(args.reports.glob("*.json"))
            bundle = pack_reports(paths, args.validator, args.suite, args.platform, args.architecture,
                                  args.source_commit, args.release_tag, args.release_commit,
                                  args.model, binary_sha, spec)
            if args.output.exists():
                raise StateError("refusing to replace an evidence envelope")
            write_json(args.output, bundle)
    except (StateError, OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError):
        print("Hosted evidence preparation failed; no unvalidated envelope was published.", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
