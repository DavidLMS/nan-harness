#!/usr/bin/env python3
"""Publish only bounded Windows diagnostic facts to an Actions summary."""
from __future__ import annotations
import argparse, json, re
from pathlib import Path

HARNESSES = frozenset(("claude-code", "codex", "opencode", "hermes", "pi", "omp", "prime-agent", "deepseek-harness", "openclaw", "cline", "qwen-code", "kimi-code", "aider", "goose", "fx"))
PHASES = ("metadata", "prerequisites", "install", "version-doctor", "deterministic-contract", "live-tool")
OUTCOMES = frozenset(("passed", "failed", "blocked"))
STATUSES = frozenset(("PASS", "FAIL", "BLOCKED", "NOT_REQUESTED", "UNSUPPORTED"))
MODES = frozenset(("deterministic", "live", "native-diagnostic"))
SHA = re.compile(r"^[0-9a-f]{40}$")
TOKEN = re.compile(r"^[a-z0-9][a-z0-9_-]{0,63}$")
CAUSE = re.compile(r"^WIN-[A-Z0-9_]+-[0-9a-f]{12}$")
REASONS = frozenset(("official-version-resolved", "native-runtime-present", "native-installer-complete", "installer-failed", "install-failed", "installer-installer-failed", "installer-official-metadata-probe-failed", "installer-timeout", "installer-launch-failed", "installer-nonzero", "probe-diagnostic", "probe-failed", "version-doctor-failed", "prerequisites-failed", "metadata-failed", "build-failed", "deterministic-mode", "credential-not-configured", "private-cleanup-failed", "required-runtime-missing", "private-environment-error", "not-started", "unfinished", "deadline-exhausted", "cleanup-failed"))
SEMVER = re.compile(r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$")
SUBPHASES = frozenset(("metadata", "download", "archive", "asset-selection", "execute", "install", "virtualenv", "cleanup", "unknown"))
EXECUTABLES = frozenset(("unknown", "npm-node", "npm-cmd", "pwsh", "py-launcher", "python", "uv", "github-api", "http-download", "official-metadata", "archive", "archive-extract"))
ASSET_REASONS = frozenset(("release-empty", "expected-asset-missing", "expected-executable-missing", "invalid-archive", "empty-download", "windows-mapping-missing", "windows-asset-missing", "metadata-request-failed", "metadata-inconclusive", "invalid-ref", "invalid-version"))
NPM_CODES = frozenset(("registry-dns", "registry-connection", "registry-timeout", "registry-unreachable", "package-not-found", "permission", "tls-certificate", "npm-command-missing", "npm-unknown"))
PIP_CATEGORIES = frozenset(("network-dns", "network-connection", "network-timeout", "package-not-found", "permission", "tls-certificate", "pip-missing", "pip-unknown"))
PROCESS_REASONS = frozenset(("win32-launch-failed", "exit-nonzero", "native-unavailable"))
PARENT_INSTALL_REASONS = frozenset(("timeout", "launch-failed", "nonzero"))
INSTALLER_MARKER_REASONS = frozenset(("passed", "installer-failed", "official-asset-missing", "official-metadata-probe-failed", "official-metadata-no-windows-asset", "capability-not-implemented", "invalid-frozen-ref", "invalid-version"))
PROBE_DIAGNOSTICS = frozenset(("doctor-child-launch", "doctor-exit-nonzero", "doctor-output-invalid", "doctor-schema-invalid", "doctor-version-missing", "doctor-version-invalid", "doctor-version-mismatch", "doctor-exit-missing", "conformance-child-launch", "conformance-exit-nonzero", "conformance-output-invalid", "conformance-schema-invalid", "conformance-scenario-missing", "conformance-scenario-failed", "conformance-inventory-failed", "conformance-inventory-operational-failed", "conformance-check-invalid", "conformance-exit-missing", "live-child-launch", "live-exit-nonzero", "live-exit-missing", "live-credential-missing", "live-tool-evidence-missing", "live-read-marker-missing", "live-completion-marker-missing", "live-bridge-sentinel", "live-usage-invalid", "live-usage-summary-missing"))
DOCTOR_REASONS = frozenset(("missing", "invalid", "mismatch", "discovery-error"))
DOCTOR_SCHEMA_REASONS = frozenset(("unknown-field", "required-field", "field-type", "field-value"))
INVENTORY_REASONS = frozenset(("process-failed", "marker-missing", "provider-failed", "provider-shutdown-failed", "daemon-cleanup-failed"))
DISCOVERY_CODES = frozenset(f"NH-DISCOVERY-{index:03d}" for index in range(1, 8))

class UnsafeReport(ValueError):
    """Report content is outside the safe publication contract."""

def _token(value, label):
    if not isinstance(value, str) or not TOKEN.fullmatch(value):
        raise UnsafeReport(f"invalid {label}")
    return value

def _cause(value, label):
    if not isinstance(value, str) or not CAUSE.fullmatch(value):
        raise UnsafeReport(f"invalid {label}")
    return value

def _diagnostic(value, label):
    if not isinstance(value, dict):
        raise UnsafeReport(f"invalid {label} diagnostic")
    result = {}
    for key, item in value.items():
        if key in {"subphase", "executable", "assetReason"}:
            choices = {"subphase": SUBPHASES, "executable": EXECUTABLES, "assetReason": ASSET_REASONS}[key]
            if item not in choices: raise UnsafeReport(f"invalid {label} diagnostic")
        elif key in {"npmCode", "pipCategory", "processReason"}:
            choices = {"npmCode": NPM_CODES, "pipCategory": PIP_CATEGORIES, "processReason": PROCESS_REASONS}[key]
            if item not in choices: raise UnsafeReport(f"invalid {label} diagnostic")
        elif key == "parentReason":
            if item not in PARENT_INSTALL_REASONS: raise UnsafeReport(f"invalid {label} diagnostic")
        elif key == "installerReason":
            if item not in INSTALLER_MARKER_REASONS: raise UnsafeReport(f"invalid {label} diagnostic")
        elif key == "cleanupReason":
            if item != "cleanup-failed": raise UnsafeReport(f"invalid {label} diagnostic")
        elif key in {"exitCode", "httpStatus"}:
            if not isinstance(item, int) or isinstance(item, bool) or not -1 <= item <= 65535 or (key == "httpStatus" and not 100 <= item <= 599): raise UnsafeReport(f"invalid {label} diagnostic")
        elif key in {"doctorVersion", "doctorExpectedVersion"}:
            if not isinstance(item, str) or not SEMVER.fullmatch(item): raise UnsafeReport(f"invalid {label} diagnostic")
        elif key == "stage":
            if item not in PHASES and item not in {"complete", "harness-run", "read-marker", "completion-marker", "bridge-sentinel", "usage-evidence", "usage-summary"}: raise UnsafeReport(f"invalid {label} diagnostic")
        elif key == "diagnostics":
            if not isinstance(item, list) or len(item) > 16 or any(x not in PROBE_DIAGNOSTICS for x in item): raise UnsafeReport(f"invalid {label} diagnostic")
        elif key == "doctorReason":
            if item not in DOCTOR_REASONS: raise UnsafeReport(f"invalid {label} diagnostic")
        elif key == "doctorSchemaReason":
            if item not in DOCTOR_SCHEMA_REASONS: raise UnsafeReport(f"invalid {label} diagnostic")
        elif key == "discoveryCode":
            if item not in DISCOVERY_CODES: raise UnsafeReport(f"invalid {label} diagnostic")
        elif key == "inventoryFailureReasons":
            if not isinstance(item, list) or len(item) > 5 or len(set(item)) != len(item) or any(x not in INVENTORY_REASONS for x in item): raise UnsafeReport(f"invalid {label} diagnostic")
        else:
            raise UnsafeReport(f"invalid {label} diagnostic")
        result[key] = item
    return result

def _phase(value, label):
    if not isinstance(value, dict) or set(value) - {"status", "reason", "causalId", "causeGroup", "diagnostic", "causeDetails"} or value.get("status") not in STATUSES:
        raise UnsafeReport(f"invalid {label}")
    result = {"status": value["status"]}
    if "reason" in value:
        if value["reason"] not in REASONS:
            raise UnsafeReport(f"invalid {label} reason")
        result["reason"] = value["reason"]
    for key in ("causalId", "causeGroup"):
        if key in value:
            result[key] = _cause(value[key], f"{label} {key}")
    if "diagnostic" in value:
        result["diagnostic"] = _diagnostic(value["diagnostic"], label)
    if "causeDetails" in value:
        result["causeDetails"] = _diagnostic(value["causeDetails"], label)
    return result

def safe_view(report):
    """Return a strict display projection, rejecting arbitrary report text."""
    allowed = {"schemaVersion", "mode", "model", "sourceSha", "source", "platform", "setup", "harnesses", "totals", "groupedCauses", "nativePrerequisites", "nanHarness", "canary"}
    if not isinstance(report, dict) or set(report) - allowed or report.get("schemaVersion") != 1 or report.get("mode") not in MODES:
        raise UnsafeReport("invalid report identity")
    if not isinstance(report.get("sourceSha"), str) or not SHA.fullmatch(report["sourceSha"]):
        raise UnsafeReport("invalid source SHA")
    raw = report.get("harnesses")
    if not isinstance(raw, list) or len(raw) > len(HARNESSES):
        raise UnsafeReport("invalid harness list")
    harnesses = []
    for item in raw:
        if not isinstance(item, dict) or set(item) - {"harness", "outcome", "phases"} or item.get("harness") not in HARNESSES or item.get("outcome") not in OUTCOMES:
            raise UnsafeReport("invalid harness result")
        phases = item.get("phases")
        if not isinstance(phases, dict) or set(phases) - set(PHASES):
            raise UnsafeReport("invalid phase set")
        harnesses.append({"harness": item["harness"], "outcome": item["outcome"], "phases": {name: _phase(phases[name], name) for name in PHASES if name in phases}})
    if len({item["harness"] for item in harnesses}) != len(harnesses):
        raise UnsafeReport("duplicate harness")
    totals = report.get("totals")
    keys = ("selected", "passed", "failed", "blocked")
    if not isinstance(totals, dict) or set(totals) - set(keys) - {"phases"} or any(not isinstance(totals.get(k), int) or isinstance(totals[k], bool) or not 0 <= totals[k] <= 15 for k in keys):
        raise UnsafeReport("invalid totals")
    if "phases" in totals and (not isinstance(totals["phases"], dict) or set(totals["phases"]) - STATUSES or any(not isinstance(v, int) or isinstance(v, bool) or not 0 <= v <= 90 for v in totals["phases"].values())):
        raise UnsafeReport("invalid phase totals")
    if totals["selected"] != len(harnesses) or (totals["selected"] and totals["passed"] + totals["failed"] + totals["blocked"] != totals["selected"]):
        raise UnsafeReport("inconsistent totals")
    return {"mode": report["mode"], "sourceSha": report["sourceSha"], "harnesses": harnesses, "totals": totals}

def render(view):
    lines = ["# Native Windows CLI diagnostic", "", f"- Mode: `{view['mode']}`", f"- Source SHA: `{view['sourceSha']}`", "", "| Harness | Outcome | Phases |", "| --- | --- | --- |"]
    for item in view["harnesses"]:
        rendered = []
        for name, value in item["phases"].items():
            details = []
            if value.get("diagnostic"):
                details.append("diagnostic=" + ",".join(f"{k}={v}" for k, v in value["diagnostic"].items()))
            if value.get("causeDetails"):
                details.append("cause=" + ",".join(f"{k}={v}" for k, v in value["causeDetails"].items()))
            rendered.append(f"{name}={value['status']}" + (" [" + "; ".join(details) + "]" if details else ""))
        phases = ", ".join(rendered)
        lines.append(f"| `{item['harness']}` | `{item['outcome']}` | {phases} |")
    totals = view["totals"]
    lines += ["", f"Totals: selected={totals['selected']}, passed={totals['passed']}, failed={totals['failed']}, blocked={totals['blocked']}"]
    return "\n".join(lines) + "\n"

def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", type=Path, required=True); parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        text = render(safe_view(json.loads(args.report.read_text(encoding="utf-8-sig"))))
        args.output.write_text(text, encoding="utf-8")
    except (OSError, UnicodeError, ValueError, TypeError, json.JSONDecodeError):
        args.output.write_text("## Safe diagnostic summary unavailable\n\nThe report was missing or malformed; no diagnostic facts were published.\n", encoding="utf-8")
        return 2
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
