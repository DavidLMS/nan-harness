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
    failure = report.get("failure", {})
    failure_class = failure.get("class")
    failure_phase = failure.get("phase")
    failed = "failed" if failure_class == "harness" else "blocked"
    common = (identity, version, None)
    at, binary = report["completedAt"], report["nanHarness"]["sha256"]
    # A live-stage provider/auth failure occurs after the deterministic contract
    # has already been closed and may retain that earlier evidence.  Every other
    # failed report (including report/cleanup failures) leaves all stages blocked.
    deterministic_closed = (deterministic and report["outcome"] == "passed")
    deterministic_retainable = (deterministic and failure_phase == "live-tool"
                                and failure_class in ("harness", "infrastructure", "provider"))
    deterministic_outcome = "passed" if deterministic_closed or deterministic_retainable else failed
    result = [observation("cli", *common, None, at,
                          deterministic_outcome, binary, spec, digest, run)]
    if report.get("model") is not None and "live-tool" in checks:
        live_passed = deterministic_closed and checks["live-tool"] == "passed"
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


def desktop_batch_checks(reports, digests, spec, run):
    """Project split deterministic/live reports without synthesizing a report.

    A live-stage desktop report intentionally records deterministic probes as
    ``not-run``.  Those entries can only be completed by a matching
    deterministic report from this same validated bundle; the raw report
    digest and timestamp on each resulting check identify which report supplied
    the observation.
    """
    deterministic = {}
    live = {}
    for report, digest in zip(reports, digests):
        if report.get("nanHarness") is None:
            continue
        for app in report["results"]:
            version = app.get("appVersion")
            if not version or not VERSION.fullmatch(version):
                continue
            key = (report["platform"], report["architecture"], app["app"], version,
                   app.get("runtimeVersion"), report["nanHarness"].get("sha256"),
                   report.get("model"))
            if app["live"]["status"] == "skipped":
                deterministic[key] = (report, app, digest)
            else:
                live[key] = (report, app, digest)

    results = []
    keys = set(deterministic) | set(live)
    for key in sorted(keys, key=repr):
        det_item = deterministic.get(key)
        live_item = live.get(key)
        if det_item:
            det_report, det_app, det_digest = det_item
            clean = (det_report["cleanup"] == "passed"
                     and det_app["cleanup"] == "passed")
            det_outcome = desktop_outcome(det_app["deterministic"], clean)
            identity = ((det_report["platform"], det_report["architecture"], key[2]),
                        key[3], key[4])
            results.append(observation("desktop", *identity, None,
                                       det_report["startedAt"], det_outcome,
                                       key[5], spec, det_digest, run))
        if live_item:
            live_report, live_app, live_digest = live_item
            clean = (live_report["cleanup"] == "passed"
                     and live_app["cleanup"] == "passed")
            # A live report is not allowed to certify its own deterministic
            # prerequisite, even when its live probe passed.
            det_outcome = (desktop_outcome(det_item[1]["deterministic"],
                                           det_item[0]["cleanup"] == "passed"
                                           and det_item[1]["cleanup"] == "passed")
                           if det_item else "blocked")
            if not det_item:
                identity = ((live_report["platform"], live_report["architecture"], key[2]),
                            key[3], key[4])
                results.append(observation("desktop", *identity, None,
                                           live_report["startedAt"], det_outcome,
                                           key[5], spec, live_digest, run))
            live_outcome = desktop_outcome([live_app["live"]],
                                           clean and det_outcome == "passed")
            identity = ((live_report["platform"], live_report["architecture"], key[2]),
                        key[3], key[4])
            results.append(observation("desktop", *identity, live_report.get("model"),
                                       live_report["startedAt"], live_outcome,
                                       key[5], spec, live_digest, run))
    return results


def desktop_batch_update(raw_reports, spec, run):
    """Return one update for exact raw reports from one validated bundle."""
    reports = [json.loads(raw) for raw in raw_reports]
    if not reports:
        return {"nanHarnessVersion": "", "verifications": [], "hostedChecks": []}
    version = reports[0]["nanHarness"]["version"]
    if any(report.get("nanHarness", {}).get("version") != version for report in reports):
        raise ValueError("split desktop reports have mismatched harness versions")
    digests = [hashlib.sha256(raw).hexdigest() for raw in raw_reports]
    return {"nanHarnessVersion": version, "verifications": [],
            "hostedChecks": desktop_batch_checks(reports, digests, spec, run)}


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
