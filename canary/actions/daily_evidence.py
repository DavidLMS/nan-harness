"""Release-scoped selection and evidence checks for the daily CLI feed."""

import copy
import datetime
import json
import re
from pathlib import Path

from selection import CLI_HARNESSES, PLATFORM_ASSETS, PLATFORMS, supported_platforms


STABLE = re.compile(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\Z")
VERSION = re.compile(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-([0-9A-Za-z.-]+))?(?:\+([0-9A-Za-z.-]+))?\Z")
CHECKS = ("install-and-diagnose", "deterministic-conformance", "live-tool")


def version_key(value):
    """SemVer precedence, including upstream prereleases and ignoring build metadata."""
    match = VERSION.fullmatch(value)
    if not match:
        raise ValueError("invalid semantic version")
    major, minor, patch, pre, build = match.groups()
    for part in (pre, build):
        if part is not None and any(not token for token in part.split(".")):
            raise ValueError("empty version identifier")
    identifiers = []
    for token in pre.split(".") if pre is not None else []:
        if token.isdigit() and len(token) > 1 and token.startswith("0"):
            raise ValueError("noncanonical prerelease number")
        identifiers.append((0, int(token)) if token.isdigit() else (1, token))
    return (int(major), int(minor), int(patch), pre is None, tuple(identifiers))


def release_tags(available, recommended):
    versions = (available["version"], recommended["tag_name"].removeprefix("v"))
    if any(not isinstance(value, str) or not STABLE.fullmatch(value) for value in versions):
        raise ValueError("channels must identify stable nan-harness releases")
    return list(dict.fromkeys("v" + version for version in versions))


def pending(feed, release_version, harness, upstream, force=False):
    target = version_key(upstream)
    releases = [r for r in feed["releases"] if r["nanHarnessVersion"] == release_version]
    if len(releases) > 1:
        raise ValueError("duplicate release evidence")
    entries = [v for r in releases for v in r["verifications"] if v["id"] == harness]
    if len(entries) > 1:
        raise ValueError("duplicate harness evidence")
    live = entries[0].get("lastLiveVerifiedVersion") if entries else None
    return force or live is None or version_key(live) < target


def instant(value):
    parsed = datetime.datetime.fromisoformat(value.replace("Z", "+00:00"))
    if parsed.tzinfo is None:
        raise ValueError("evidence timestamp must have a timezone")
    return parsed


def validate_report(report, cell, release, plan):
    """Bind even failed reports before allowing any evidence from their release."""
    system = cell["system"]
    expected = {
        "version": release["version"], "source": "commit:" + release["commit"],
        "sha256": release["digests"][PLATFORM_ASSETS[system]["harness"]],
    }
    environment = report["environment"]
    if (report["schemaVersion"] != 2 or report["runId"] != plan["runId"]
            or report["trigger"] != "daily" or report["nanHarness"] != expected
            or report["specSha256"] != plan["specSha256"]
            or report["harness"]["id"] != cell["harness"]
            or environment["operatingSystem"] != system
            or environment["architecture"] != PLATFORMS[system]["architecture"]):
        raise ValueError("daily report provenance mismatch")
    started, completed = instant(report["startedAt"]), instant(report["completedAt"])
    if not instant(plan["startedAt"]) <= started <= completed <= datetime.datetime.now(datetime.timezone.utc):
        raise ValueError("daily report time is outside this run")
    if report["outcome"] != "passed":
        return False
    checks = report["checks"]
    if (report["tier"] != "live-core" or report.get("model") != plan["model"]
            or report["harness"]["version"] != cell["version"]
            or [check["name"] for check in checks] != list(CHECKS)
            or any(check["status"] != "passed" for check in checks)):
        raise ValueError("daily report does not prove the frozen live contract")
    return True


def collect_release(plan, release, reports, validate_file):
    """Accept complete harness groups; malformed evidence rejects the release batch."""
    updates, results = [], []
    selected = [c for c in plan["cells"] if c["tag"] == release["tag"]]
    expected_paths = {c["report"] for c in selected}
    actual_paths = {p.name for p in reports.glob(release["tag"] + "-*.json")}
    if actual_paths - expected_paths or len(expected_paths) != len(selected):
        raise ValueError("unexpected or duplicate daily reports")
    for harness in CLI_HARNESSES:
        cells = [c for c in selected if c["harness"] == harness]
        if not cells:
            continue
        if (sorted(c["system"] for c in cells) != sorted(supported_platforms(harness))
                or len({c["version"] for c in cells}) != 1):
            raise ValueError("incomplete frozen platform selection")
        passed = []
        for cell in cells:
            path = reports / cell["report"]
            if path.is_file():
                validate_file(path)
                report = json.loads(path.read_bytes())
                if validate_report(report, cell, release, plan):
                    passed.append(report)
        status = "verified" if len(passed) == len(cells) else "pending"
        results.append({"tag": release["tag"], "harness": harness, "status": status})
        if status == "verified":
            completed = max((r["completedAt"] for r in passed), key=instant)
            updates.append({"nanHarnessVersion": release["version"], "id": harness,
                            "lastCompatibleVersion": cells[0]["version"], "compatibleAt": completed,
                            "lastLiveVerifiedVersion": cells[0]["version"], "liveVerifiedAt": completed})
    return updates, results


def preserve_unobserved(base, merged, updates):
    """Project Rust's validated merge onto only the observed CLI entries.

    The general release merger may seed Desktop evidence from the checkout. Daily
    qualification must preserve every unobserved record, including absent ones.
    """
    result = copy.deepcopy(base)
    for update in updates:
        version, harness = update["nanHarnessVersion"], update["id"]
        source = next(r for r in merged["releases"] if r["nanHarnessVersion"] == version)
        entry = next(v for v in source["verifications"] if v["id"] == harness)
        target = next((r for r in result["releases"] if r["nanHarnessVersion"] == version), None)
        if target is None:
            target = {"nanHarnessVersion": version, "verifications": []}
            result["releases"].append(target)
        existing = next((v for v in target["verifications"] if v["id"] == harness), None)
        if existing is None:
            target["verifications"].append(copy.deepcopy(entry))
        else:
            existing.update(entry)
    return result


if __name__ == "__main__":
    import sys
    base, merged, directory, output = map(Path, sys.argv[1:])
    updates = [json.loads(p.read_bytes()) for p in sorted(directory.glob("*.json"))]
    output.write_text(json.dumps(preserve_unobserved(json.loads(base.read_bytes()),
                                                   json.loads(merged.read_bytes()), updates), indent=2) + "\n")
