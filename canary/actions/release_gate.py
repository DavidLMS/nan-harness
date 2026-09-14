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
from pathlib import Path


HARNESSES = (
    "claude-code", "codex", "opencode", "hermes", "pi", "omp", "prime-agent",
    "deepseek-harness", "openclaw", "cline", "qwen-code", "kimi-code", "aider",
    "goose", "fx",
)
SYSTEMS = ("linux", "macos")
ASSETS = (
    "nan-harness-aarch64-unknown-linux-musl",
    "nan-harness-canary-aarch64-unknown-linux-musl",
    "nan-harness-aarch64-apple-darwin",
    "nan-harness-canary-aarch64-apple-darwin",
)
PLATFORM_ASSETS = {
    "linux": "nan-harness-aarch64-unknown-linux-musl",
    "macos": "nan-harness-aarch64-apple-darwin",
}
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
    # first separator to the canonical system/harness pair.
    return {f"{system}-{harness}" for system in SYSTEMS for harness in HARNESSES}


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
        raise ValueError("checksum manifest must contain all four ARM64 assets")
    result = []
    for name in ASSETS:
        path = directory / name
        if not path.is_file() or digest(path) != entries[name]:
            raise ValueError(f"asset checksum mismatch: {name}")
        result.append({"name": name, "sha256": entries[name], "bytes": path.stat().st_size})
    return result, digest(checksum)


def _require_passing_release_report(report, version, path):
    if (report.get("schemaVersion") != 2 or report.get("outcome") != "passed"
            or report.get("trigger") != "release" or report.get("tier") != "release-gate"):
        raise ValueError(f"report is not a passing release-gate report: {path.name}")
    if report.get("nanHarness", {}).get("version") != version:
        raise ValueError(f"report nan-harness version does not match release: {path.name}")
    checks = report.get("checks")
    if not isinstance(checks, list):
        raise ValueError(f"report checks are missing: {path.name}")
    names = [check.get("name") for check in checks if isinstance(check, dict)]
    for required in REQUIRED_CHECKS:
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
        _require_passing_release_report(report, args.tag[1:], path)
        environment = report.get("environment", {})
        harness_evidence = report.get("harness", {})
        nan_harness = report.get("nanHarness", {})
        identity = f"{environment.get('operatingSystem')}-{harness_evidence.get('id')}"
        if identity not in expected_identities() or identity in seen:
            raise ValueError(f"duplicate or unknown report identity: {identity}")
        seen.add(identity)
        if environment.get("architecture") != "aarch64":
            raise ValueError(f"report architecture is not ARM64: {path.name}")
        source = nan_harness.get("source")
        if (source != "commit:" + args.tag_commit or report.get("runId") != args.run_id
                or nan_harness.get("version") != args.tag[1:]
                or nan_harness.get("sha256") != next(item["sha256"] for item in assets
                                                  if item["name"] == PLATFORM_ASSETS[environment["operatingSystem"]])):
            raise ValueError(f"report provenance mismatch: {path.name}")
        reports.append({
            "identity": identity,
            "path": path.name,
            "sha256": digest(path),
            "sourceSha": args.tag_commit,
            "runId": report["runId"],
            "bytes": path.stat().st_size,
        })
    if seen != expected_identities() or len(reports) != 30:
        raise ValueError(f"expected exactly 30 unique passing reports, found {len(reports)}")
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
    cells = []
    for system in SYSTEMS:
        # macos-14 is the previously validated ARM64 hosted label; the cell
        # still checks uname so a label change cannot silently qualify x86_64.
        runner = "ubuntu-24.04-arm" if system == "linux" else "macos-14"
        for harness in HARNESSES:
            cells.append({"system": system, "runner": runner, "architecture": "aarch64", "harness": harness})
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
