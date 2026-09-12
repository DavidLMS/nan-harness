#!/usr/bin/env python3
"""Project validated runner reports into exact schema-v5 compatibility evidence."""

import datetime
import hashlib
import json
import re

from selection import CLI_HARNESSES, DESKTOP_HARNESSES, resolve_model


HEX = re.compile(r"[0-9a-f]{64}\Z")
VERSION = re.compile(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?\Z")
IDENTITY_FIELDS = ("suite", "id", "platform", "architecture", "harnessVersion",
                   "runtimeVersion", "model", "nanHarnessSha256", "specSha256")
DESKTOP_MISMATCHES = {"input-mismatch", "response-mismatch", "tool-mismatch", "unsupported-version"}


def instant(value):
    parsed = datetime.datetime.fromisoformat(value.replace("Z", "+00:00"))
    if parsed.tzinfo is None:
        raise ValueError("an evidence timestamp requires its timezone")
    return parsed


def target_identity(check):
    return tuple(check.get(field) for field in IDENTITY_FIELDS)


def should_probe(feed, version, target, now):
    """Skip known results; retry infrastructure blocks at most once per day."""
    if feed.get("schemaVersion") != 5:
        raise ValueError("selective checks require a validated schema-v5 feed")
    observations = [check for release in feed["releases"]
                    if release["nanHarnessVersion"] == version
                    for check in release.get("hostedChecks", [])
                    if target_identity(check) == target_identity(target)]
    if not observations:
        return True
    latest = max(observations, key=lambda check: instant(check["checkedAt"]))
    return (latest["outcome"] == "blocked"
            and now - instant(latest["checkedAt"]) >= datetime.timedelta(days=1))


def observation(suite, identity, version, runtime, model, at, outcome, binary, spec, digest, run):
    catalog = CLI_HARNESSES if suite == "cli" else DESKTOP_HARNESSES
    platform, architecture, harness = identity
    if (harness not in catalog or platform not in ("linux", "macos", "windows")
            or architecture not in ("aarch64", "x86_64") or not VERSION.fullmatch(version)
            or any(not HEX.fullmatch(value) for value in (binary, spec, digest))
            or type(run) is not int or run < 1):
        raise ValueError("invalid hosted evidence identity")
    instant(at)
    check = {"suite": suite, "id": harness, "platform": platform, "architecture": architecture,
             "harnessVersion": version, "checkedAt": at, "outcome": outcome,
             "nanHarnessSha256": binary, "specSha256": spec, "evidenceSha256": digest,
             "sourceRun": run}
    if runtime is not None:
        if suite != "desktop" or not VERSION.fullmatch(runtime):
            raise ValueError("invalid bundled runtime identity")
        check["runtimeVersion"] = runtime
    if model is not None:
        check["model"] = resolve_model(model, configured="")
        if not model:
            raise ValueError("live evidence requires its actual model")
    return check


def cli_checks(report, digest, spec, run):
    """Only complete functional checks are positive; setup/provider failures stay blocked."""
    environment = report["environment"]
    identity = (environment["operatingSystem"], environment["architecture"], report["harness"]["id"])
    version = report["harness"]["version"]
    if not VERSION.fullmatch(version):
        return []
    checks = {check["name"]: check["status"] for check in report["checks"]}
    deterministic = all(checks.get(name) == "passed"
                        for name in ("install-and-diagnose", "deterministic-conformance"))
    failure_class = report.get("failure", {}).get("class")
    failed = "failed" if failure_class == "harness" else "blocked"
    common = (identity, version, None)
    at, binary = report["completedAt"], report["nanHarness"]["sha256"]
    deterministic_outcome = "passed" if deterministic and report["outcome"] == "passed" else failed
    if deterministic and report["outcome"] != "passed":
        # A final cleanup failure must not certify an earlier successful stage.
        deterministic_outcome = "blocked"
    result = [observation("cli", *common, None, at,
                          deterministic_outcome, binary, spec, digest, run)]
    if report.get("model") is not None and "live-tool" in checks:
        live_passed = deterministic and checks["live-tool"] == "passed" and report["outcome"] == "passed"
        result.append(observation("cli", *common, report["model"], at,
                                  "passed" if live_passed else failed, binary, spec, digest, run))
    return result


def desktop_outcome(probes, clean):
    if clean and all(probe["status"] == "passed" for probe in probes):
        return "passed"
    if clean and any(probe.get("reason") in DESKTOP_MISMATCHES for probe in probes):
        return "failed"
    return "blocked"


def desktop_checks(report, digest, spec, run):
    if report.get("nanHarness") is None:
        return []
    results = []
    binary = report["nanHarness"]["sha256"]
    for app in report["results"]:
        version = app.get("appVersion")
        if not version or not VERSION.fullmatch(version):
            continue
        identity = (report["platform"], report["architecture"], app["app"])
        clean = report["cleanup"] == "passed" and app["cleanup"] == "passed"
        common = (identity, version, app.get("runtimeVersion"))
        deterministic = desktop_outcome(app["deterministic"], clean)
        results.append(observation("desktop", *common, None, report["startedAt"], deterministic,
                                   binary, spec, digest, run))
        if app["live"]["status"] != "skipped":
            if report.get("schemaVersion") != 3 or not report.get("model"):
                raise ValueError("new live certification requires explicit model evidence")
            outcome = desktop_outcome([app["live"]], clean and deterministic == "passed")
            results.append(observation("desktop", *common, report["model"], report["startedAt"], outcome,
                                       binary, spec, digest, run))
    return results


def report_update(suite, raw, spec, run):
    """The caller must first run the trusted report validator on these exact bytes."""
    if suite not in ("cli", "desktop") or len(raw) > 65536:
        raise ValueError("invalid report kind or size")
    report = json.loads(raw)
    identity = report.get("nanHarness")
    if identity is None:
        return None
    version = identity["version"]
    if not VERSION.fullmatch(version):
        raise ValueError("invalid nan-harness version")
    digest = hashlib.sha256(raw).hexdigest()
    convert = cli_checks if suite == "cli" else desktop_checks
    return {"nanHarnessVersion": version, "verifications": [],
            "hostedChecks": convert(report, digest, spec, run)}
