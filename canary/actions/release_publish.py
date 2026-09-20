#!/usr/bin/env python3
"""Publish an already validated release without executing tag-owned code.

This entry point is intended to run from a trusted default-branch checkout.  The
handoff is the only input from the test jobs; its paths are treated as data and
all report and asset bytes are verified before any GitHub mutation.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent))
from selection import (CLI_HARNESSES as HARNESSES, PLATFORM_ASSETS, PLATFORMS,
                       qualified_identities, required_assets)


SCHEMA_VERSION = 1
RECEIPT_SCHEMA_VERSION = 1
MAX_REPORT_BYTES = 2_000_000
MAX_EVIDENCE_BYTES = 64_000_000
REQUIRED_CHECKS = ("install-and-diagnose", "deterministic-conformance", "live-tool")
REQUIRED_IDENTITIES = qualified_identities()
REPORT_COUNT = len(REQUIRED_IDENTITIES)
ASSET_NAMES = required_assets()
HISTORICAL_IDENTITIES = {f"{platform}/{harness}" for platform in ("linux", "macos") for harness in HARNESSES}
HISTORICAL_ASSETS = {name for platform in ("linux", "macos") for name in PLATFORM_ASSETS[platform].values()}
HEX40 = re.compile(r"^[0-9a-f]{40}$")
HEX64 = re.compile(r"^[0-9a-f]{64}$")
TAG = re.compile(r"^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-[0-9A-Za-z.-]+)?$")
REPOSITORY = re.compile(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$")


class ContractError(ValueError):
    """The handoff or durable receipt is not safe to consume."""


def digest(path: Path) -> str:
    hasher = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            hasher.update(block)
    return hasher.hexdigest()


def _string(value: Any, name: str) -> str:
    if not isinstance(value, str) or not value:
        raise ContractError(f"{name} must be a non-empty string")
    return value


def _hex(value: Any, pattern: re.Pattern[str], name: str) -> str:
    value = _string(value, name)
    if not pattern.fullmatch(value):
        raise ContractError(f"{name} is not a lowercase digest")
    return value


def _load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text())
    except (OSError, ValueError) as error:
        raise ContractError(f"could not read JSON evidence {path}") from error
    if not isinstance(value, dict):
        raise ContractError(f"JSON evidence is not an object: {path}")
    return value


def _child_env(*, github: bool) -> dict[str, str]:
    """Give children only the credentials needed for their boundary."""
    environment = dict(os.environ)
    environment.pop("GITHUB_TOKEN", None)
    environment.pop("NAN_API_KEY", None)
    if not github:
        environment.pop("GH_TOKEN", None)
    return environment


def _require_tools() -> None:
    required = ("gh", "jq", "perl", "cargo")
    missing = [name for name in required if not shutil.which(name)]
    if not (shutil.which("sha256sum") or shutil.which("shasum")):
        missing.append("sha256sum or shasum")
    if missing:
        raise ContractError("publisher requires tools: " + ", ".join(missing))


def _canonical_digest(value: dict[str, Any]) -> str:
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def _safe_relative(path: str, root: Path, name: str) -> Path:
    candidate = Path(path)
    if candidate.is_absolute() or ".." in candidate.parts:
        raise ContractError(f"{name} escapes its evidence directory")
    resolved = (root / candidate).resolve()
    try:
        resolved.relative_to(root.resolve())
    except ValueError as error:
        raise ContractError(f"{name} escapes its evidence directory") from error
    return resolved


def _evidence_path(value: str, preferred_root: Path, fallback_root: Path, name: str) -> Path:
    """Resolve either a handoff-root path or an artifact-directory path."""
    candidate = _safe_relative(value, preferred_root, name)
    if candidate.is_file() or preferred_root == fallback_root:
        return candidate
    return _safe_relative(value, fallback_root, name)


def _validate_release_report(report: dict[str, Any], version: str, name: str) -> None:
    if (report.get("schemaVersion") != 2 or report.get("outcome") != "passed"
            or report.get("trigger") != "release" or report.get("tier") != "release-gate"):
        raise ContractError(f"report is not a passing release-gate report: {name}")
    if report.get("nanHarness", {}).get("version") != version:
        raise ContractError(f"report nan-harness version does not match release: {name}")
    checks = report.get("checks")
    if not isinstance(checks, list):
        raise ContractError(f"report checks are missing: {name}")
    names = [check.get("name") for check in checks if isinstance(check, dict)]
    for required in REQUIRED_CHECKS:
        if names.count(required) != 1:
            raise ContractError(f"report must contain exactly one {required} check: {name}")
        if checks[names.index(required)].get("status") != "passed":
            raise ContractError(f"report check is not passing: {name}")


def _matrix_policy(handoff: dict[str, Any], recommendation: bool):
    # Historical evidence is usable only on the recommendation path, which also
    # requires a bound, completed publication receipt and a public stable release.
    if recommendation and handoff.get("reportCount") == len(HISTORICAL_IDENTITIES):
        return HISTORICAL_IDENTITIES, HISTORICAL_ASSETS
    return REQUIRED_IDENTITIES, set(ASSET_NAMES)


def validate_handoff(path: Path, assets_dir: Path | None = None, reports_dir: Path | None = None,
                    require_evidence: bool = True, *, recommendation: bool = False) -> dict[str, Any]:
    """Validate provenance and all local evidence, returning normalized data."""
    handoff = _load_json(path)
    if handoff.get("schemaVersion") != SCHEMA_VERSION:
        raise ContractError("unsupported handoff schema")
    repository = _string(handoff.get("repository"), "repository")
    if not REPOSITORY.fullmatch(repository):
        raise ContractError("repository is not owner/name")
    tag = _string(handoff.get("tag"), "tag")
    if not TAG.fullmatch(tag):
        raise ContractError("tag is not a supported semantic-version tag")
    tag_commit = _hex(handoff.get("tagCommit"), HEX40, "tagCommit")
    workflow_commit = _hex(handoff.get("workflowCommit"), HEX40, "workflowCommit")
    run_id = _string(handoff.get("runId"), "runId")
    expected_identities, expected_assets = _matrix_policy(handoff, recommendation)
    report_count = len(expected_identities)
    if handoff.get("reportCount") != report_count:
        raise ContractError(f"handoff must contain exactly {report_count} reports")
    root = path.parent.resolve()
    assets_root = (assets_dir or root).resolve()
    reports_root = (reports_dir or root).resolve()
    reports = handoff.get("reports")
    if not isinstance(reports, list) or len(reports) != report_count:
        raise ContractError(
            f"handoff reports must contain exactly {report_count} entries")
    identities: set[str] = set()
    asset_entries = handoff.get("assets")
    if not isinstance(asset_entries, list):
        raise ContractError("handoff assets are required before report validation")
    asset_digests = {entry.get("name"): entry.get("sha256") for entry in asset_entries if isinstance(entry, dict)}
    for entry in reports:
        if not isinstance(entry, dict):
            raise ContractError("report entry is not an object")
        identity = _string(entry.get("identity"), "report identity")
        canonical_identity = identity.replace("-", "/", 1) if "/" not in identity else identity
        if canonical_identity in identities or canonical_identity not in expected_identities:
            raise ContractError(f"duplicate or unknown report identity: {identity}")
        identities.add(canonical_identity)
        report_run = _string(entry.get("runId"), "report runId")
        if report_run != run_id:
            raise ContractError("report runId does not match handoff")
        source_sha = _hex(entry.get("sourceSha"), HEX40, "report sourceSha")
        if source_sha != tag_commit:
            raise ContractError("report sourceSha does not match tagCommit")
        report_path_value = _string(entry.get("path"), "report path")
        report_path = _evidence_path(report_path_value, reports_root, root, "report path")
        report_digest = _hex(entry.get("sha256"), HEX64, "report sha256")
        if not report_path.is_file() and not require_evidence:
            continue
        if not report_path.is_file() or digest(report_path) != report_digest:
            raise ContractError(f"report bytes do not match handoff: {identity}")
        report = _load_json(report_path)
        if report.get("runId") != run_id or report.get("trigger") != "release":
            raise ContractError(f"report provenance mismatch: {identity}")
        if report.get("nanHarness", {}).get("source") != f"commit:{tag_commit}":
            raise ContractError(f"report source identity mismatch: {identity}")
        _validate_release_report(report, tag[1:].split("-", 1)[0], identity)
        platform, harness = canonical_identity.split("/", 1)
        platform_asset = PLATFORM_ASSETS[platform]["harness"]
        if report.get("nanHarness", {}).get("sha256") != asset_digests.get(platform_asset):
            raise ContractError(f"report binary digest does not match release asset: {identity}")
        if report.get("harness", {}).get("id") != harness:
            raise ContractError(f"report harness mismatch: {identity}")
        if (report.get("environment", {}).get("operatingSystem") != platform
                or report.get("environment", {}).get("architecture")
                != PLATFORMS[platform]["architecture"]):
            raise ContractError(f"report platform mismatch: {identity}")
    if identities != expected_identities:
        raise ContractError("handoff report matrix is incomplete")

    assets = handoff.get("assets")
    if not isinstance(assets, list) or {item.get("name") for item in assets if isinstance(item, dict)} != expected_assets:
        raise ContractError("handoff assets must contain every required release asset")
    for entry in assets:
        if not isinstance(entry, dict):
            raise ContractError("asset entry is not an object")
        name = entry.get("name")
        if name not in expected_assets:
            raise ContractError("unknown asset name")
        asset_path_value = entry.get("path", f"{entry.get('name', '')}")
        asset_path = _evidence_path(_string(asset_path_value, "asset path"), assets_root, root, "asset path")
        expected = _hex(entry.get("sha256"), HEX64, "asset sha256")
        if not asset_path.is_file() and not require_evidence:
            continue
        if not asset_path.is_file() or digest(asset_path) != expected:
            raise ContractError(f"asset bytes do not match handoff: {name}")
        if not isinstance(entry.get("bytes"), int) or entry["bytes"] != asset_path.stat().st_size:
            raise ContractError(f"asset size does not match handoff: {name}")
    manifest = _hex(handoff.get("assetManifestSha256"), HEX64, "assetManifestSha256")
    manifest_path_value = handoff.get("assetManifest", "SHA256SUMS")
    manifest_path = _evidence_path(_string(manifest_path_value, "assetManifest"), assets_root, root, "assetManifest")
    if not manifest_path.is_file() and not require_evidence:
        manifest_path = None
    if manifest_path is not None and (not manifest_path.is_file() or digest(manifest_path) != manifest):
        raise ContractError("SHA256SUMS does not match handoff")
    attestation = handoff.get("attestation")
    if not isinstance(attestation, dict):
        raise ContractError("attestation metadata is required")
    if _string(attestation.get("sourceRef"), "attestation sourceRef") != f"refs/tags/{tag}":
        raise ContractError("attestation sourceRef does not match tag")
    _string(attestation.get("workflow"), "attestation workflow")
    handoff["_handoffSha256"] = _canonical_digest(handoff)
    return handoff


PHASES = ("assetsVerified", "reportsVerified", "evidencePublished", "compatibilityPublished", "releasePublished", "availablePublished")


def build_evidence(path: Path, handoff: dict[str, Any], reports_dir: Path | None = None) -> dict[str, Any]:
    """Create the bounded, JSON-only durable evidence asset."""
    root = path.parent.resolve()
    reports_root = (reports_dir or root).resolve()
    expected: dict[str, dict[str, Any]] = {}
    for entry in handoff["reports"]:
        identity = entry["identity"].replace("-", "/", 1) if "/" not in entry["identity"] else entry["identity"]
        source = _evidence_path(entry["path"], reports_root, root, "report path")
        if not source.is_file() or source.stat().st_size > MAX_REPORT_BYTES:
            raise ContractError(f"report is missing or exceeds the evidence bound: {identity}")
        expected[identity] = _load_json(source)
    evidence = {
        "schemaVersion": 1,
        "handoff": {key: value for key, value in handoff.items() if not key.startswith("_")},
        "reports": expected,
        "reportDigests": {
            (entry["identity"].replace("-", "/", 1) if "/" not in entry["identity"] else entry["identity"]): entry["sha256"]
            for entry in handoff["reports"]
        },
        "reportCanonicalDigests": {identity: _canonical_digest(report) for identity, report in expected.items()},
    }
    encoded = json.dumps(evidence, sort_keys=True, separators=(",", ":")).encode()
    if len(encoded) > MAX_EVIDENCE_BYTES:
        raise ContractError("release evidence exceeds the size bound")
    return evidence


def validate_evidence(evidence: dict[str, Any], repository: str, tag: str, *,
                      recommendation: bool = False) -> dict[str, Any]:
    """Validate durable evidence without filesystem paths or downloaded binaries."""
    if set(evidence) != {"schemaVersion", "handoff", "reports", "reportDigests", "reportCanonicalDigests"}:
        raise ContractError("evidence has unexpected or missing fields")
    if evidence["schemaVersion"] != 1 or not isinstance(evidence["handoff"], dict):
        raise ContractError("unsupported evidence schema")
    handoff = evidence["handoff"]
    if handoff.get("repository") != repository or handoff.get("tag") != tag:
        raise ContractError("evidence handoff identity mismatch")
    _hex(handoff.get("tagCommit"), HEX40, "evidence tagCommit")
    _hex(handoff.get("workflowCommit"), HEX40, "evidence workflowCommit")
    expected, expected_assets = _matrix_policy(handoff, recommendation)
    if handoff.get("reportCount") != len(expected) or not isinstance(evidence["reports"], dict):
        raise ContractError(f"evidence does not contain exactly {len(expected)} reports")
    reports = evidence["reports"]
    if set(reports) != expected or set(evidence["reportDigests"]) != expected or set(evidence["reportCanonicalDigests"]) != expected:
        raise ContractError("evidence report identities are incomplete or duplicated")
    handoff_by_identity = {
        (item["identity"].replace("-", "/", 1) if "/" not in item["identity"] else item["identity"]): item
        for item in handoff.get("reports", []) if isinstance(item, dict)
    }
    if set(handoff_by_identity) != expected:
        raise ContractError("evidence handoff report identities are incomplete")
    asset_digests = {item.get("name"): item.get("sha256") for item in handoff.get("assets", []) if isinstance(item, dict)}
    if set(asset_digests) != expected_assets:
        raise ContractError("evidence handoff assets are incomplete")
    for identity in sorted(expected):
        report = reports[identity]
        if not isinstance(report, dict) or len(json.dumps(report)) > MAX_REPORT_BYTES:
            raise ContractError(f"invalid or oversized report: {identity}")
        if evidence["reportDigests"][identity] != handoff_by_identity[identity].get("sha256"):
            raise ContractError(f"report digest binding mismatch: {identity}")
        _hex(evidence["reportDigests"][identity], HEX64, "report digest")
        _hex(evidence["reportCanonicalDigests"][identity], HEX64, "report canonical digest")
        if evidence["reportCanonicalDigests"][identity] != _canonical_digest(report):
            raise ContractError(f"report canonical digest mismatch: {identity}")
        platform, harness = identity.split("/", 1)
        _validate_release_report(report, str(handoff.get("tag", ""))[1:].split("-", 1)[0], identity)
        if (report.get("runId") != handoff.get("runId")
                or report.get("nanHarness", {}).get("source") != f"commit:{handoff.get('tagCommit')}"
                or report.get("harness", {}).get("id") != harness
                or report.get("environment", {}).get("operatingSystem") != platform
                or report.get("environment", {}).get("architecture")
                != PLATFORMS[platform]["architecture"]):
            raise ContractError(f"report content/provenance mismatch: {identity}")
        binary = PLATFORM_ASSETS[platform]["harness"]
        if report.get("nanHarness", {}).get("sha256") != asset_digests[binary]:
            raise ContractError(f"report binary binding mismatch: {identity}")
    return handoff


def validate_embedded_reports(evidence: dict[str, Any], validator: str, directory: Path) -> None:
    """Run the trusted validator over every report recovered from durable evidence."""
    directory.mkdir(parents=True, exist_ok=True)
    for identity, report in sorted(evidence["reports"].items()):
        path = directory / (identity.replace("/", "-") + ".json")
        path.write_text(json.dumps(report, sort_keys=True, separators=(",", ":")) + "\n")
        subprocess.run([validator, "validate-report", str(path)], check=True, env=_child_env(github=False))


def validate_receipt(receipt: dict[str, Any], handoff: dict[str, Any]) -> None:
    if receipt.get("schemaVersion") != RECEIPT_SCHEMA_VERSION:
        raise ContractError("unsupported publication receipt schema")
    for field in ("repository", "tag", "tagCommit", "workflowCommit", "runId"):
        if receipt.get(field) != handoff.get(field):
            raise ContractError(f"receipt {field} does not match handoff")
    if receipt.get("handoffSha256") != handoff.get("_handoffSha256"):
        raise ContractError("receipt is not bound to this exact handoff")
    if receipt.get("evidenceSha256") != handoff.get("_evidenceSha256"):
        raise ContractError("receipt is not bound to durable evidence")
    if receipt.get("assetDigests") != handoff.get("_assetDigests"):
        raise ContractError("receipt asset digests do not match the handoff")
    phases = receipt.get("phases")
    if not isinstance(phases, dict) or set(phases) != set(PHASES) or any(type(phases[p]) is not bool for p in PHASES):
        raise ContractError("receipt phases are not closed booleans")
    for previous, current in zip(PHASES, PHASES[1:]):
        if phases[current] and not phases[previous]:
            raise ContractError("receipt phases are not monotonic")


def _gh(*args: str, input_text: str | None = None) -> str:
    result = subprocess.run(["gh", *args], input=input_text, text=True, capture_output=True,
                            env=_child_env(github=True))
    if result.returncode:
        raise RuntimeError(result.stderr.strip() or "gh command failed")
    return result.stdout


def _remote_tag_commit(repository: str, tag: str) -> str:
    value = json.loads(_gh("api", f"repos/{repository}/git/ref/tags/{tag}"))
    obj = value.get("object", {})
    while obj.get("type") == "tag":
        value = json.loads(_gh("api", f"repos/{repository}/git/tags/{obj['sha']}"))
        obj = value.get("object", {})
    if obj.get("type") != "commit" or not HEX40.fullmatch(str(obj.get("sha", ""))):
        raise ContractError("remote tag does not resolve to a commit")
    return obj["sha"]


def _receipt_asset(repository: str, tag: str, destination: Path) -> bool:
    name = f"release-publication-receipt-{tag[1:]}.json"
    result = subprocess.run(["gh", "release", "download", tag, "--repo", repository, "--pattern", name,
                             "--output", str(destination), "--clobber"], capture_output=True, text=True)
    return result.returncode == 0


def _evidence_asset(repository: str, tag: str, destination: Path) -> bool:
    name = f"release-gate-evidence-{tag[1:]}.json"
    result = subprocess.run(["gh", "release", "download", tag, "--repo", repository, "--pattern", name,
                             "--output", str(destination), "--clobber"], capture_output=True, text=True)
    return result.returncode == 0


def _upload_receipt(repository: str, tag: str, path: Path) -> None:
    name = f"release-publication-receipt-{tag[1:]}.json"
    _gh("release", "upload", tag, str(path), "--repo", repository, "--clobber")


def _upload_evidence(repository: str, tag: str, path: Path) -> None:
    _gh("release", "upload", tag, str(path), "--repo", repository, "--clobber")


def _recommendation_version(tag: str) -> tuple[int, int, int]:
    return tuple(int(part) for part in tag[1:].split("-", 1)[0].split("."))


def _release_is_draft(repository: str, tag: str) -> bool:
    value = json.loads(_gh("release", "view", tag, "--repo", repository, "--json", "tagName,isDraft"))
    if value.get("tagName") != tag:
        raise ContractError("release response does not match requested tag")
    return value.get("isDraft") is True


def _remote_feed_asset(repository: str, release: str, asset: str, destination: Path) -> dict[str, Any]:
    _gh("release", "download", release, "--repo", repository, "--pattern", asset,
        "--output", str(destination), "--clobber")
    return _load_json(destination)


def _verify_remote_feed_state(repository: str, tag: str, work: Path) -> None:
    version = tag[1:].split("-", 1)[0]
    available = _remote_feed_asset(repository, "available", "update-manifest.json", work / "available.json")
    if available.get("version") is None or _recommendation_version("v" + str(available["version"])) < _recommendation_version(tag):
        raise ContractError("available feed does not include the published release")
    compatibility = _remote_feed_asset(repository, "compatibility", "compatibility-v3.json", work / "compatibility.json")
    releases = compatibility.get("releases")
    if not isinstance(releases, list) or not any(isinstance(item, dict) and item.get("nanHarnessVersion") == version for item in releases):
        raise ContractError("compatibility feed does not include the published release")


def _set_phase(phases: dict[str, bool], phase: str, receipt: Path, handoff: dict[str, Any], publish: bool) -> None:
    phases[phase] = True
    _write_receipt(receipt, handoff, phases)
    if publish:
        _upload_receipt(handoff["repository"], handoff["tag"], receipt)


def _write_receipt(path: Path, handoff: dict[str, Any], phases: dict[str, bool]) -> None:
    value = {"schemaVersion": RECEIPT_SCHEMA_VERSION, **{key: handoff[key] for key in ("repository", "tag", "tagCommit", "workflowCommit", "runId")},
             "handoffSha256": handoff["_handoffSha256"],
             "evidenceSha256": handoff["_evidenceSha256"], "assetDigests": handoff["_assetDigests"],
             "assetManifestSha256": handoff["assetManifestSha256"], "reportCount": REPORT_COUNT,
             "phases": phases}
    path.write_text(json.dumps(value, sort_keys=True, indent=2) + "\n")


def publish(args: argparse.Namespace) -> int:
    if args.publish and args.recommend:
        raise ContractError("publication and recommendation are separate operations")
    if (args.publish or args.recommend) and "-" in args.tag:
        raise ContractError("publication and recommendation require a stable release tag")
    if args.recommend and args.handoff is None:
        with tempfile.TemporaryDirectory(prefix="nan-release-recommend-bootstrap-") as bootstrap:
            bootstrap_root = Path(bootstrap)
            receipt = bootstrap_root / "receipt.json"
            evidence_path = bootstrap_root / f"release-gate-evidence-{args.tag[1:]}.json"
            if not _receipt_asset(args.repository, args.tag, receipt):
                raise ContractError("recommendation requires the durable completed publication receipt")
            if not _evidence_asset(args.repository, args.tag, evidence_path):
                raise ContractError("recommendation requires the durable release-gate evidence asset")
            evidence = _load_json(evidence_path)
            validate_evidence(evidence, args.repository, args.tag, recommendation=True)
            handoff_path = bootstrap_root / "handoff.json"
            handoff_path.write_text(json.dumps(evidence["handoff"], sort_keys=True) + "\n")
            empty_assets = bootstrap_root / "empty-assets"
            empty_assets.mkdir()
            args.handoff = handoff_path
            args.assets_dir = empty_assets
            args.reports_dir = None
            return publish(args)
    if args.handoff is None or args.assets_dir is None:
        raise ContractError("publish requires --handoff and --assets-dir")
    # A resumed publication may have only the durable evidence left after the
    # hosted report artifact expired. Require local report bytes whenever the
    # report directory is present; the receipt/evidence binding below remains
    # mandatory before recovery is allowed when it is absent.
    local_reports_available = args.reports_dir is None or args.reports_dir.is_dir()
    handoff = validate_handoff(Path(args.handoff), Path(args.assets_dir), args.reports_dir,
                               not args.recommend and local_reports_available,
                               recommendation=args.recommend)
    if args.repository != handoff["repository"] or args.tag != handoff["tag"]:
        raise ContractError("CLI identity does not match handoff")
    if args.tag_commit is not None and args.tag_commit != handoff["tagCommit"]:
        raise ContractError("CLI tagCommit does not match handoff")
    if not args.recommend and args.workflow_commit is not None and args.workflow_commit != handoff["workflowCommit"]:
        raise ContractError("CLI workflowCommit does not match handoff")
    if _remote_tag_commit(args.repository, args.tag) != handoff["tagCommit"]:
        raise ContractError("remote tag no longer names the handed-off commit")
    checkout_commit = subprocess.run(["git", "rev-parse", "HEAD"], capture_output=True, text=True, check=True).stdout.strip()
    if args.workflow_commit is not None and args.workflow_commit != checkout_commit:
        raise ContractError("publisher checkout does not match trusted workflowCommit")
    if not args.recommend and checkout_commit != handoff["workflowCommit"]:
        raise ContractError("publisher checkout does not match handoff workflowCommit")
    if args.recommend and args.workflow_commit is None:
        raise ContractError("recommendation requires the trusted workflowCommit")
    if args.recommend and args.run_id is None:
        raise ContractError("recommendation requires the current workflow runId")
    if not args.recommend and args.run_id is not None and args.run_id != handoff["runId"]:
        raise ContractError("publisher runId does not match handoff")
    if handoff["attestation"].get("workflow") != f"{args.repository}/.github/workflows/release.yml":
        raise ContractError("attestation workflow is not the trusted release workflow")
    _require_tools()
    with tempfile.TemporaryDirectory(prefix="nan-release-publish-") as temporary:
        temporary_path = Path(temporary)
        report_validator = args.report_validator or os.environ.get("NAN_TRUSTED_REPORT_VALIDATOR")
        if report_validator is None:
            trusted_root = Path(__file__).resolve().parents[2]
            trusted_binary = trusted_root / "target" / "debug" / "nan-harness-canary"
            if not trusted_binary.is_file():
                subprocess.run(["cargo", "build", "--locked", "--package", "nan-harness-canary",
                                "--bin", "nan-harness-canary"], cwd=trusted_root, check=True,
                                env=_child_env(github=False))
            report_validator = str(trusted_binary)
        if not Path(report_validator).is_file() or not os.access(report_validator, os.X_OK):
            raise ContractError("report validator is not an executable trusted artifact")
        if args.receipt_dir is not None:
            args.receipt_dir.mkdir(parents=True, exist_ok=True)
            args.receipt_dir.chmod(0o700)
            receipt_path = args.receipt_dir / f"release-publication-receipt-{args.tag[1:]}.json"
            if receipt_path.is_symlink():
                raise ContractError("receipt path must not be a symlink")
        else:
            # GitHub preserves the local basename on upload; keep the durable contract's
            # release-publication-receipt name even when resuming from a temporary workspace.
            receipt_path = temporary_path / f"release-publication-receipt-{args.tag[1:]}.json"
        receipt_exists = _receipt_asset(args.repository, args.tag, receipt_path)
        receipt = _load_json(receipt_path) if receipt_exists else None
        if args.recommend and not receipt_exists:
            raise ContractError("recommendation requires the durable completed publication receipt")
        if receipt is not None:
            handoff["_evidenceSha256"] = receipt.get("evidenceSha256")
            handoff["_assetDigests"] = receipt.get("assetDigests")
            expected_asset_digests = {item.get("name"): item.get("sha256") for item in handoff.get("assets", [])
                                     if isinstance(item, dict)}
            if handoff["_assetDigests"] != expected_asset_digests:
                raise ContractError("receipt asset digests do not match original handoff")
            validate_receipt(receipt, handoff)
            phases = receipt["phases"]
        else:
            if not _release_is_draft(args.repository, args.tag):
                raise ContractError("public release has no durable publication receipt")
            phases = {phase: False for phase in PHASES}
            handoff["_assetDigests"] = {item["name"]: item["sha256"] for item in handoff["assets"]}
        assets = Path(args.assets_dir)
        manifest = _evidence_path(handoff.get("assetManifest", "SHA256SUMS"), assets,
                                  Path(args.handoff).resolve().parent, "assetManifest")
        if not args.recommend and digest(manifest) != handoff["assetManifestSha256"]:
            raise ContractError("asset manifest changed during publication")
        if not args.recommend:
            for entry in handoff["assets"]:
                if digest(assets / entry["name"]) != entry["sha256"]:
                    raise ContractError("asset changed during publication")
        local_evidence = temporary_path / f"release-gate-evidence-{args.tag[1:]}.json"
        if not args.recommend:
            # Evidence is assembled after the trusted validator runs below; this path is used
            # for the immutable byte comparison once it exists.
            local_evidence.write_text("{}")
        remote_evidence = temporary_path / "remote-evidence.json"
        remote_evidence_exists = _evidence_asset(args.repository, args.tag, remote_evidence)
        durable_evidence: dict[str, Any] | None = None
        if args.recommend:
            if not remote_evidence_exists:
                raise ContractError("durable release-gate evidence asset is missing")
            if digest(remote_evidence) != handoff["_evidenceSha256"]:
                raise ContractError("durable evidence digest differs from receipt")
            durable_evidence = _load_json(remote_evidence)
            validate_evidence(durable_evidence, args.repository, args.tag, recommendation=True)
            validate_embedded_reports(durable_evidence, report_validator, temporary_path / "embedded-reports")
            handoff["_evidenceSha256"] = digest(remote_evidence)
        elif remote_evidence_exists:
            if receipt is not None and phases["evidencePublished"]:
                if digest(remote_evidence) != handoff["_evidenceSha256"]:
                    raise ContractError("durable evidence changed after publication")
            durable_evidence = _load_json(remote_evidence)
            validate_evidence(durable_evidence, args.repository, args.tag)
            validate_embedded_reports(durable_evidence, report_validator, temporary_path / "embedded-reports")
            handoff["_evidenceSha256"] = digest(remote_evidence)
            if receipt is None:
                phases.update(assetsVerified=True, reportsVerified=True, evidencePublished=True)
        elif receipt is not None and phases["evidencePublished"]:
            raise ContractError("receipt claims durable evidence but the asset is missing")
        remote_assets = temporary_path / "remote-assets"
        remote_assets.mkdir()
        _gh("release", "download", args.tag, "--repo", args.repository, "--dir", str(remote_assets), "--clobber")
        verify_script = Path(__file__).resolve().parents[1] / "host" / "verify-release-assets.sh"
        subprocess.run([str(verify_script), "--release-tag", args.tag,
                        "--assets-dir", str(remote_assets), "--repository", args.repository], check=True,
                       env=_child_env(github=True))
        if digest(remote_assets / "SHA256SUMS") != handoff["assetManifestSha256"]:
            raise ContractError("remote checksum manifest differs from handoff")
        for entry in handoff["assets"]:
            if digest(remote_assets / entry["name"]) != entry["sha256"]:
                raise ContractError(f"remote asset differs from handoff: {entry['name']}")
        if phases["releasePublished"] and _release_is_draft(args.repository, args.tag):
            raise ContractError("receipt claims publication but GitHub still reports a draft")
        if phases["compatibilityPublished"]:
            compatibility = _remote_feed_asset(args.repository, "compatibility", "compatibility-v3.json",
                                               temporary_path / "resume-compatibility.json")
            version = args.tag[1:].split("-", 1)[0]
            if not any(isinstance(item, dict) and item.get("nanHarnessVersion") == version
                       for item in compatibility.get("releases", [])):
                raise ContractError("receipt claims compatibility publication but feed state disagrees")
        if phases["availablePublished"] and "-" not in args.tag:
            _verify_remote_feed_state(args.repository, args.tag, temporary_path)
        if args.recommend:
            if not all(phases.values()):
                raise ContractError("recommendation requires all publication phases")
            release = json.loads(_gh("release", "view", args.tag, "--repo", args.repository,
                                     "--json", "tagName,isDraft,isPrerelease"))
            if release.get("tagName") != args.tag or release.get("isDraft") or release.get("isPrerelease"):
                raise ContractError("only a published stable release can be recommended")
            _verify_remote_feed_state(args.repository, args.tag, temporary_path)
            latest_result = subprocess.run(["gh", "api", f"repos/{args.repository}/releases/latest"],
                                           capture_output=True, text=True)
            if latest_result.returncode == 0:
                latest = json.loads(latest_result.stdout).get("tag_name", "")
                if latest and _recommendation_version(latest) > _recommendation_version(args.tag):
                    raise ContractError("recommendation is not forward-only")
            elif "404" not in latest_result.stderr:
                raise ContractError("could not read current recommendation")
            _gh("release", "edit", args.tag, "--repo", args.repository, "--latest")
            recommendation = temporary_path / f"release-recommendation-receipt-{args.tag[1:]}.json"
            recommendation.write_text(json.dumps({"schemaVersion": 1, "repository": args.repository,
                                                  "tag": args.tag, "tagCommit": handoff["tagCommit"],
                                                  "workflowCommit": args.workflow_commit,
                                                  "runId": args.run_id,
                                                  "evidenceWorkflowCommit": handoff["workflowCommit"],
                                                  "evidenceRunId": handoff["runId"],
                                                  "handoffSha256": handoff["_handoffSha256"],
                                                  "assetManifestSha256": handoff["assetManifestSha256"]},
                                                 sort_keys=True) + "\n")
            _gh("release", "upload", args.tag, str(recommendation), "--repo", args.repository, "--clobber")
            return 0
        if not phases["assetsVerified"]:
            phases["assetsVerified"] = True
        if not phases["reportsVerified"]:
            reports = temporary_path / "reports"
            reports.mkdir()
            compatibility_reports = temporary_path / "compatibility-reports"
            compatibility_reports.mkdir()
            report_paths = []
            for entry in handoff["reports"]:
                source = (args.reports_dir or Path(args.handoff).resolve().parent) / entry["path"]
                if not source.is_file():
                    source = Path(args.handoff).resolve().parent / entry["path"]
                destination = _safe_relative(entry["path"], reports, "report path")
                destination.parent.mkdir(parents=True, exist_ok=True)
                destination.write_bytes(source.read_bytes())
                # The hosted artifact preserves the platform-specific report
                # basename, while the compatibility helper has a deliberate
                # canonical input contract (linux-HARNESS.json and
                # macos-HARNESS.json). Keep the declared path for immutable
                # evidence and make an explicit, confined helper copy.
                compatibility_name = entry["identity"].replace("/", "-") + ".json"
                (compatibility_reports / compatibility_name).write_bytes(source.read_bytes())
                report_paths.append(destination)
            for report in report_paths:
                subprocess.run([report_validator, "validate-report", str(report)], check=True,
                               env=_child_env(github=False))
            phases["reportsVerified"] = True
        if not args.recommend and not phases["evidencePublished"]:
            if not (temporary_path / "reports").is_dir():
                raise ContractError("reports are not available to create durable evidence")
            evidence = build_evidence(Path(args.handoff), handoff, temporary_path / "reports")
            local_evidence.write_text(json.dumps(evidence, sort_keys=True, separators=(",", ":")) + "\n")
            handoff["_evidenceSha256"] = digest(local_evidence)
            if remote_evidence_exists:
                if digest(remote_evidence) != handoff["_evidenceSha256"]:
                    raise ContractError("existing durable evidence differs from this handoff")
            elif args.publish:
                _upload_evidence(args.repository, args.tag, local_evidence)
                if not _evidence_asset(args.repository, args.tag, remote_evidence):
                    raise ContractError("uploaded durable evidence could not be read back")
                if digest(remote_evidence) != handoff["_evidenceSha256"]:
                    raise ContractError("uploaded durable evidence changed in transit")
            validate_evidence(_load_json(local_evidence), args.repository, args.tag)
            if args.publish:
                _set_phase(phases, "evidencePublished", receipt_path, handoff, args.publish)
        if not args.publish:
            return 0
        if not phases["compatibilityPublished"]:
            reports_for_publication = temporary_path / "reports"
            compatibility_reports = temporary_path / "compatibility-reports"
            if not reports_for_publication.is_dir() and durable_evidence is not None:
                embedded_reports = temporary_path / "embedded-reports"
                if not embedded_reports.is_dir():
                    validate_embedded_reports(durable_evidence, report_validator, embedded_reports)
                reports_for_publication.mkdir()
                for entry in handoff["reports"]:
                    identity = entry["identity"].replace("/", "-") + ".json"
                    source = embedded_reports / identity
                    destination = _safe_relative(entry["path"], reports_for_publication, "report path")
                    destination.parent.mkdir(parents=True, exist_ok=True)
                    destination.write_bytes(source.read_bytes())
            if not compatibility_reports.is_dir() and reports_for_publication.is_dir():
                compatibility_reports.mkdir()
                for entry in handoff["reports"]:
                    source = _safe_relative(entry["path"], reports_for_publication, "report path")
                    destination = compatibility_reports / (entry["identity"].replace("/", "-") + ".json")
                    destination.write_bytes(source.read_bytes())
            if compatibility_reports.is_dir():
                reports_for_publication = compatibility_reports
            if not reports_for_publication.is_dir():
                raise ContractError("reports are unavailable for compatibility publication")
            output = temporary_path / "compatibility-output"
            state = temporary_path / "compatibility-state"
            output.mkdir()
            subprocess.run([str(Path(__file__).resolve().parents[1] / "host" / "publish-compatibility.sh"),
                            "--trigger", "release", "--nan-harness-version", args.tag[1:],
                            "--release-tag", args.tag, "--reports", str(reports_for_publication),
                            "--output-dir", str(output), "--state-dir", str(state),
                            "--report-validator", report_validator, "--repository", args.repository,
                            "--publish-feed"], check=True, env=_child_env(github=True))
            _set_phase(phases, "compatibilityPublished", receipt_path, handoff, args.publish)
        if not phases["releasePublished"]:
            if _release_is_draft(args.repository, args.tag):
                _gh("release", "edit", args.tag, "--repo", args.repository, "--draft=false", "--latest=false")
            _set_phase(phases, "releasePublished", receipt_path, handoff, args.publish)
        if not phases["availablePublished"]:
            subprocess.run([str(Path(__file__).resolve().parents[1] / "host" / "publish-available-release.sh"),
                            "--tag", args.tag, "--assets-dir", str(temporary_path / "remote-assets"),
                            "--repository", args.repository], check=True, env=_child_env(github=True))
            _set_phase(phases, "availablePublished", receipt_path, handoff, args.publish)
        return 0


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument("--repository", required=True)
    result.add_argument("--tag", required=True)
    result.add_argument("--handoff", "--reports-manifest", dest="handoff", type=Path)
    result.add_argument("--assets-dir", type=Path)
    result.add_argument("--reports-dir", type=Path)
    result.add_argument("--report-validator")
    result.add_argument("--tag-commit")
    result.add_argument("--workflow-commit")
    result.add_argument("--run-id")
    result.add_argument("--receipt-dir", type=Path)
    mode = result.add_mutually_exclusive_group()
    mode.add_argument("--recommend", action="store_true")
    mode.add_argument("--publish", action="store_true")
    return result


def main(argv: list[str] | None = None) -> int:
    try:
        return publish(parser().parse_args(argv))
    except (ContractError, RuntimeError, OSError) as error:
        print(f"release publication refused: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
