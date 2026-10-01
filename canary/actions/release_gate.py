#!/usr/bin/env python3
"""Trusted-branch helpers for the hosted release compatibility gate.

This module deliberately handles only closed, hashed inputs.  Downloading and
attestation verification happen in the workflow; this code records the exact
inputs and rejects incomplete or cross-release report sets before publication.
"""

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path


sys.path.insert(0, str(Path(__file__).resolve().parent))
from selection import (CLI_HARNESSES as HARNESSES, PLATFORM_ASSETS, PLATFORMS,
                      qualified_identities, required_assets, supported_platforms)


SYSTEMS = tuple(PLATFORMS)
ASSETS = required_assets()
COMMIT = re.compile(r"[0-9a-f]{40}\Z")
SHA256 = re.compile(r"[0-9a-f]{64}\Z")
TAG = re.compile(r"v(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)(?:-[0-9A-Za-z.-]+)?\Z")
REQUIRED_CHECKS = ("install-and-diagnose", "deterministic-conformance", "live-tool")


def digest(path: Path) -> str:
    hasher = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            hasher.update(chunk)
    return hasher.hexdigest()


def expected_identities():
    # Artifact names use a hyphenated identity; the publisher normalizes the
    # first separator to the canonical system/harness pair. A harness is qualified
    # only on the platforms its support list declares.
    return {identity.replace("/", "-") for identity in qualified_identities()}


def _require(value, pattern, label):
    if not isinstance(value, str) or not pattern.fullmatch(value):
        raise ValueError(f"{label} is malformed")


def _asset_entries(directory: Path):
    checksum = directory / "SHA256SUMS"
    if not checksum.is_file():
        raise ValueError("SHA256SUMS is missing")
    entries = {}
    for line in checksum.read_text().splitlines():
        fields = line.split()
        if len(fields) != 2:
            if len(fields) > 1 and fields[-1] in ASSETS:
                raise ValueError(f"malformed checksum entry: {fields[-1]}")
            continue
        value, name = fields
        if name in ASSETS and not SHA256.fullmatch(value):
            raise ValueError(f"malformed checksum entry: {name}")
        if name in entries:
            raise ValueError(f"duplicate checksum entry: {name}")
        if SHA256.fullmatch(value):
            entries[name] = value
    if not set(ASSETS).issubset(entries):
        raise ValueError("checksum manifest must contain every required release asset")
    result = []
    for name in ASSETS:
        path = directory / name
        if not path.is_file() or digest(path) != entries[name]:
            raise ValueError(f"asset checksum mismatch: {name}")
        result.append({"name": name, "sha256": entries[name], "bytes": path.stat().st_size})
    return result, digest(checksum)


def _require_passing_release_report(report, version, path, mode):
    if (report.get("schemaVersion") != 2 or report.get("outcome") != "passed"
            or report.get("trigger") != "release" or report.get("tier") != "release-gate"):
        raise ValueError(f"report is not a passing release-gate report: {path.name}")
    if report.get("nanHarness", {}).get("version") != version:
        raise ValueError(f"report nan-harness version does not match release: {path.name}")
    checks = report.get("checks")
    if not isinstance(checks, list):
        raise ValueError(f"report checks are missing: {path.name}")
    names = [check.get("name") for check in checks if isinstance(check, dict)]
    required_checks = REQUIRED_CHECKS if mode == "live" else REQUIRED_CHECKS[:2]
    for required in required_checks:
        if names.count(required) != 1:
            raise ValueError(f"report must contain exactly one {required} check: {path.name}")
        check = checks[names.index(required)]
        if check.get("status") != "passed":
            raise ValueError(f"report check is not passing: {path.name}")


def build_manifest(args):
    _require(args.repository, re.compile(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+\Z"), "repository")
    _require(args.tag, TAG, "tag")
    for value, label in ((args.tag_commit, "tagCommit"), (args.workflow_commit, "workflowCommit")):
        _require(value, COMMIT, label)
    assets, asset_manifest = _asset_entries(args.assets_dir)
    reports = []
    seen = set()
    for path in sorted(args.reports_dir.glob("*.json")):
        report = json.loads(path.read_text())
        _require_passing_release_report(report, args.tag[1:], path, getattr(args, "mode", "live"))
        environment = report.get("environment", {})
        harness_evidence = report.get("harness", {})
        nan_harness = report.get("nanHarness", {})
        identity = f"{environment.get('operatingSystem')}-{harness_evidence.get('id')}"
        if identity not in expected_identities() or identity in seen:
            raise ValueError(f"duplicate or unknown report identity: {identity}")
        seen.add(identity)
        expected_architecture = PLATFORMS[environment["operatingSystem"]]["architecture"]
        if environment.get("architecture") != expected_architecture:
            raise ValueError(
                f"report architecture is not {expected_architecture}: {path.name}")
        source = nan_harness.get("source")
        if (source != "commit:" + args.tag_commit or report.get("runId") != args.run_id
                or nan_harness.get("version") != args.tag[1:]
                or nan_harness.get("sha256") != next(
                    item["sha256"] for item in assets
                    if item["name"] == PLATFORM_ASSETS[environment["operatingSystem"]]["harness"])):
            raise ValueError(f"report provenance mismatch: {path.name}")
        reports.append({
            "identity": identity,
            "path": path.name,
            "sha256": digest(path),
            "sourceSha": args.tag_commit,
            "runId": report["runId"],
            "bytes": path.stat().st_size,
        })
    expected = expected_identities()
    if seen != expected or len(reports) != len(expected):
        raise ValueError(
            f"expected exactly {len(expected)} unique passing reports, found {len(reports)}")
    manifest = {
        "schemaVersion": 1,
        "repository": args.repository,
        "tag": args.tag,
        "tagCommit": args.tag_commit,
        "workflowCommit": args.workflow_commit,
        "runId": args.run_id,
        "reports": reports,
        "reportCount": len(reports),
        "assets": assets,
        "assetManifestSha256": asset_manifest,
        "attestation": {
            "workflow": args.repository + "/.github/workflows/release.yml",
            "sourceRef": "refs/tags/" + args.tag,
        },
    }
    args.output.write_text(json.dumps(manifest, sort_keys=True) + "\n")
    return manifest


def assets_command(args):
    assets, asset_manifest = _asset_entries(args.assets_dir)
    args.output.write_text(json.dumps({"assets": assets, "assetManifestSha256": asset_manifest}, sort_keys=True) + "\n")


def matrix(_args):
    """Every cell the release gate must collect, from each harness support list."""
    cells = []
    for harness in HARNESSES:
        for system in supported_platforms(harness):
            entry = PLATFORMS[system]
            if PLATFORM_ASSETS[system]["canary"] is None:
                raise ValueError(
                    system + " qualification requires a published canary release asset")
            cells.append({"system": system, "runner": entry["runner"],
                          "architecture": entry["architecture"], "target": entry["target"],
                          "harness": harness})
    print(json.dumps({"include": cells}, separators=(",", ":")))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    matrix_parser = subparsers.add_parser("matrix")
    matrix_parser.set_defaults(function=matrix)
    assets_parser = subparsers.add_parser("assets")
    assets_parser.add_argument("--assets-dir", type=Path, required=True)
    assets_parser.add_argument("--output", type=Path, required=True)
    assets_parser.set_defaults(function=assets_command)
    manifest_parser = subparsers.add_parser("manifest")
    manifest_parser.add_argument("--mode", choices=("live", "deterministic"), default="live")
    for name in ("repository", "tag", "tag-commit", "workflow-commit", "run-id"):
        manifest_parser.add_argument("--" + name, required=True)
    manifest_parser.add_argument("--reports-dir", type=Path, required=True)
    manifest_parser.add_argument("--assets-dir", type=Path, required=True)
    manifest_parser.add_argument("--output", type=Path, required=True)
    manifest_parser.set_defaults(function=build_manifest)
    args = parser.parse_args()
    try:
        args.function(args)
    except (OSError, ValueError, json.JSONDecodeError) as error:
        parser.error(str(error))


if __name__ == "__main__":
    main()
