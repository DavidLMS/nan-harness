#!/usr/bin/env python3
"""Validate and safely render the Rust Codex diagnostic battery report."""
from __future__ import annotations

import argparse
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import time

CASE_IDS = (
    "install.version_resolution", "launch.direct.pipe", "launch.direct.private_file",
    "launch.supervised.pipe", "launch.supervised.private_file", "probe.inventory",
    "probe.tool_round_trip", "probe.sentinel", "lifecycle.normal_exit",
    "lifecycle.timeout", "lifecycle.cancel",
)
CASES = frozenset(CASE_IDS)
STATUSES = frozenset(("passed", "failed", "blocked"))
REASONS = frozenset((
    "unsafe_prerequisite", "unavailable", "launch_failed", "version_failed",
    "resolution_failed", "capture_failed", "inventory_failed",
    "tool_round_trip_failed", "sentinel_failed", "normal_exit_failed",
    "timeout_failed", "cancel_failed", "deadline_exceeded", "cleanup_failed", "none",
))
ROOT_KINDS = frozenset(("code", "signal", "unavailable"))
EOF_STATES = frozenset(("observed", "not_observed", "unknown"))
TERMINATION_RESULTS = frozenset(("not_needed", "succeeded", "failed", "unknown"))
CLEANUP_STAGES = frozenset(("none", "terminate", "wait", "wait_timeout", "capture_timeout"))
READER_STATES = frozenset(("eof", "open", "error"))
SURVIVOR_SCAN_STATES = frozenset(("not_needed", "available", "unavailable"))
# Bounded executable base names only: paths, command lines, and payloads stay out of the report.
PROCESS_NAME = re.compile(r"^[A-Za-z0-9._+-]{1,48}$")
MAX_UINT32 = 2**32 - 1
MAX_UINT64 = 2**64 - 1


class UnsafeReport(ValueError):
    """Raised when a report cannot be reduced to the closed public contract."""


def _windows_diagnostic_module():
    path = Path(__file__).with_name("windows_diagnostic.py")
    spec = importlib.util.spec_from_file_location("windows_diagnostic_integration", path)
    module = importlib.util.module_from_spec(spec)
    if spec.loader is None:
        raise RuntimeError("diagnostic resolver unavailable")
    spec.loader.exec_module(module)
    return module


def blocked_report(path: Path, reason="unsafe_prerequisite"):
    value = {"schemaVersion": 1, "harness": "codex",
             "cases": [{"id": case, "status": "blocked", "reason": reason,
                         "durationMilliseconds": 0} for case in CASE_IDS],
             "overall": {"executed": 0, "passed": 0, "failed": 0, "blocked": len(CASE_IDS)},
             "durationMilliseconds": 0}
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, separators=(",", ":")), encoding="utf-8")


def prepare_codex(cell: Path, report: Path, model="qwen3.6", timeout=900):
    """Resolve and install Codex into the existing private canary cell."""
    try:
        started = time.monotonic()
        report.parent.mkdir(parents=True, exist_ok=True)
        module = _windows_diagnostic_module()
        cell.mkdir(mode=0o700, parents=True, exist_ok=True)
        env = module.isolated_environment(cell)
        resolved, diagnostic = module.resolve_one(module.resolver_module(), "codex", model, timeout=min(20, timeout))
        if resolved is None:
            report.write_text(json.dumps({"status": "blocked", "reason": "unavailable"}), encoding="utf-8")
            return 2
        installer = Path(__file__).resolve().parents[1] / "guest" / "install-harness.ps1"
        command = ["pwsh", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass",
                   "-File", str(installer), "-Harness", "codex", "-Version", resolved.version]
        remaining = timeout - (time.monotonic() - started)
        if remaining <= 0:
            raise TimeoutError
        code, process_reason = module.run_bounded(command, cell, env, max(0.1, remaining - 20))
        marker = cell / "installer-result.json"
        if code != 0 or not marker.is_file():
            report.write_text(json.dumps({"status": "blocked", "reason": "unavailable"}), encoding="utf-8")
            return 2
        # Use the installed native binary, not the npm.cmd shim or another PATH installation.
        installed = json.loads(marker.read_text(encoding="utf-8"))
        if installed.get("status") != "passed":
            raise ValueError("installer did not pass")
        candidates = list((cell / "home" / ".npm-global" / "node_modules" / "@openai").rglob("codex.exe"))
        candidates = [path for path in candidates if path.is_file() and path.resolve().is_relative_to(cell.resolve())]
        if len(candidates) != 1:
            raise ValueError("native executable resolution ambiguous")
        report.write_text(json.dumps({"status": "passed", "reason": "resolved-installed",
                                      "executable": str(candidates[0].resolve()), "version": resolved.version}), encoding="utf-8")
        return 0
    except (OSError, RuntimeError, TimeoutError, ValueError, subprocess.TimeoutExpired):
        report.parent.mkdir(parents=True, exist_ok=True)
        report.write_text(json.dumps({"status": "blocked", "reason": "unsafe_prerequisite"}), encoding="utf-8")
        return 2


def _object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise UnsafeReport("duplicate report field")
        result[key] = value
    return result


def load_report(path: Path):
    try:
        return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=_object,
                          parse_constant=lambda _value: (_ for _ in ()).throw(UnsafeReport("non-finite report number")))
    except (OSError, UnicodeError, json.JSONDecodeError, UnsafeReport) as exc:
        raise UnsafeReport("report unavailable or malformed") from exc


def _uint(value, label, maximum=MAX_UINT64):
    if not isinstance(value, int) or isinstance(value, bool) or not 0 <= value <= maximum:
        raise UnsafeReport(f"invalid {label}")
    return value


def _eof(value, label):
    if not isinstance(value, dict) or set(value) != {"eof", "atMilliseconds"} or value.get("eof") not in EOF_STATES:
        raise UnsafeReport(f"invalid {label}")
    result = {"eof": value["eof"]}
    result["atMilliseconds"] = None if value["atMilliseconds"] is None else _uint(value["atMilliseconds"], f"{label}.atMilliseconds")
    if (value["eof"] == "observed") != (value["atMilliseconds"] is not None):
        raise UnsafeReport("inconsistent EOF observation")
    return result


def _termination(value, label, before):
    expected = {"attempted", "result"} if before else {"reaped", "result"}
    if not isinstance(value, dict) or set(value) != expected:
        raise UnsafeReport(f"invalid {label}")
    flag = "attempted" if before else "reaped"
    if not isinstance(value[flag], bool) or value["result"] not in TERMINATION_RESULTS:
        raise UnsafeReport(f"invalid {label}")
    return {flag: value[flag], "result": value["result"]}


def _readers(value):
    if not isinstance(value, dict) or set(value) != {"stdout", "stderr"}:
        raise UnsafeReport("invalid readers")
    for name in ("stdout", "stderr"):
        if value[name] not in READER_STATES:
            raise UnsafeReport("invalid reader state")
    return {"stdout": value["stdout"], "stderr": value["stderr"]}


def _delayed_eof(value, label):
    if not isinstance(value, dict) or set(value) != {"eof", "atMilliseconds"}:
        raise UnsafeReport(f"invalid {label}")
    if value["eof"] not in ("observed", "unknown"):
        raise UnsafeReport(f"invalid {label}")
    at = None if value["atMilliseconds"] is None else _uint(value["atMilliseconds"], label)
    if (value["eof"] == "observed") != (at is not None):
        raise UnsafeReport(f"inconsistent {label}")
    return {"eof": value["eof"], "atMilliseconds": at}


def _survivor_scan(value, label):
    if not isinstance(value, dict) or set(value) != {"scan", "names", "count"}:
        raise UnsafeReport(f"invalid {label}")
    if value["scan"] not in SURVIVOR_SCAN_STATES:
        raise UnsafeReport(f"invalid {label} state")
    names = value["names"]
    if not isinstance(names, list) or len(names) > 8:
        raise UnsafeReport(f"invalid {label} names")
    for name in names:
        if not isinstance(name, str) or (PROCESS_NAME.match(name) is None and name != "<unknown>"):
            raise UnsafeReport(f"invalid {label} name")
    count = _uint(value["count"], f"{label} count", MAX_UINT32)
    if len(names) > count:
        raise UnsafeReport(f"inconsistent {label} count")
    return {"scan": value["scan"], "names": list(names), "count": count}


def _survivors(value):
    if not isinstance(value, dict) or set(value) != {"atFailure", "residual"}:
        raise UnsafeReport("invalid survivors")
    return {"atFailure": _survivor_scan(value["atFailure"], "survivors.atFailure"),
            "residual": _survivor_scan(value["residual"], "survivors.residual")}


def _evidence(value):
    allowed = {"event", "capture", "rootExit", "stdout", "stderr", "termination", "cleanup", "marker", "provider",
               "readers", "afterCleanup", "survivors"}
    if not isinstance(value, dict) or set(value) != allowed:
        raise UnsafeReport("invalid evidence")
    if value["event"] not in {"exited", "timed_out", "cancelled", "failed"} or value["capture"] not in {"pipe", "private_file"}:
        raise UnsafeReport("invalid process event or capture")
    result = {"event": value["event"], "capture": value["capture"]}
    if "rootExit" in value:
        item = value["rootExit"]
        if not isinstance(item, dict) or set(item) - {"kind", "value"} or "kind" not in item or item["kind"] not in ROOT_KINDS:
            raise UnsafeReport("invalid root exit")
        if "value" in item:
            if item["kind"] == "unavailable":
                raise UnsafeReport("invalid unavailable root exit")
            if not isinstance(item["value"], int) or isinstance(item["value"], bool) or not -(2**63) <= item["value"] <= 2**63 - 1:
                raise UnsafeReport("invalid root exit value")
        result["rootExit"] = dict(item)
    for name in ("stdout", "stderr"):
        if name in value:
            result[name] = _eof(value[name], name)
    if "termination" in value:
        item = value["termination"]
        if not isinstance(item, dict) or set(item) - {"before", "after"} or set(item) != {"before", "after"}:
            raise UnsafeReport("invalid termination")
        result["termination"] = {"before": _termination(item["before"], "termination.before", True),
                                  "after": _termination(item["after"], "termination.after", True)}
    if "cleanup" in value:
        item = value["cleanup"]
        if not isinstance(item, dict) or set(item) - {"stage", "osErrorCode"} or "stage" not in item or item["stage"] not in CLEANUP_STAGES:
            raise UnsafeReport("invalid cleanup")
        clean = {"stage": item["stage"]}
        if "osErrorCode" in item:
            clean["osErrorCode"] = _uint(item["osErrorCode"], "cleanup.osErrorCode", MAX_UINT32)
        result["cleanup"] = clean
    if "marker" in value:
        item = value["marker"]
        if not isinstance(item, dict) or set(item) != {"observed"} or not isinstance(item["observed"], bool):
            raise UnsafeReport("invalid marker")
        result["marker"] = item
    if "provider" in value:
        item = value["provider"]
        if not isinstance(item, dict) or set(item) != {"requests", "roundTrip"} or not isinstance(item.get("roundTrip"), bool):
            raise UnsafeReport("invalid provider evidence")
        result["provider"] = {"requests": _uint(item["requests"], "provider.requests", MAX_UINT32),
                               "roundTrip": item["roundTrip"]}
        if "readers" in value:
            result["readers"] = _readers(value["readers"])
        if "afterCleanup" in value:
            item = value["afterCleanup"]
            if not isinstance(item, dict) or set(item) != {"stdout", "stderr"}:
                raise UnsafeReport("invalid afterCleanup")
            result["afterCleanup"] = {"stdout": _delayed_eof(item["stdout"], "afterCleanup.stdout"),
                                       "stderr": _delayed_eof(item["stderr"], "afterCleanup.stderr")}
        if "survivors" in value:
            result["survivors"] = _survivors(value["survivors"])
    return result


def safe_view(report):
    if not isinstance(report, dict) or set(report) - {"schemaVersion", "harness", "cases", "overall", "durationMilliseconds"}:
        raise UnsafeReport("invalid report envelope")
    if (not isinstance(report.get("schemaVersion"), int) or isinstance(report["schemaVersion"], bool)
            or report["schemaVersion"] != 1 or report.get("harness") != "codex"):
        raise UnsafeReport("invalid report identity")
    raw_cases = report.get("cases")
    if not isinstance(raw_cases, list) or len(raw_cases) != len(CASE_IDS):
        raise UnsafeReport("cases must account for every diagnostic")
    cases = []
    for item in raw_cases:
        if not isinstance(item, dict) or set(item) - {"id", "status", "reason", "durationMilliseconds", "evidence"}:
            raise UnsafeReport("invalid case")
        if item.get("id") not in CASES or item.get("status") not in STATUSES or item.get("reason") not in REASONS:
            raise UnsafeReport("invalid case identity")
        if (item["status"] == "passed") != (item["reason"] == "none"):
            raise UnsafeReport("inconsistent case status and reason")
        if item["status"] == "passed" and "evidence" not in item:
            raise UnsafeReport("passed case lacks evidence")
        if "durationMilliseconds" not in item:
            raise UnsafeReport("missing case duration")
        case = {"id": item["id"], "status": item["status"], "reason": item["reason"],
                "durationMilliseconds": _uint(item["durationMilliseconds"], "case duration")}
        if "evidence" in item:
            case["evidence"] = _evidence(item["evidence"])
        cases.append(case)
    if {item["id"] for item in cases} != CASES:
        raise UnsafeReport("missing or duplicate case")
    overall = report.get("overall")
    keys = {"executed", "passed", "failed", "blocked"}
    if not isinstance(overall, dict) or set(overall) != keys:
        raise UnsafeReport("invalid totals")
    counts = {status: sum(item["status"] == status for item in cases) for status in STATUSES}
    if any(_uint(overall[name], f"overall.{name}", len(CASE_IDS)) != counts[name] for name in STATUSES):
        raise UnsafeReport("inconsistent totals")
    if _uint(overall["executed"], "overall.executed", len(CASE_IDS)) != counts["passed"] + counts["failed"]:
        raise UnsafeReport("inconsistent totals")
    view = {"harness": "codex", "cases": cases, "overall": dict(overall),
            "durationMilliseconds": _uint(report.get("durationMilliseconds"), "report duration")}
    return view


def _detail_observation(evidence):
    """Renders the attribution facts that explain a capture that never reached end of file."""
    parts = []
    readers = evidence.get("readers")
    if readers:
        parts.append(f"; readers=stdout:{readers['stdout']},stderr:{readers['stderr']}")
    after = evidence.get("afterCleanup")
    if after:
        parts.append(f"; afterCleanup=stdout:{after['stdout']['eof']},stderr:{after['stderr']['eof']}")
    survivors = evidence.get("survivors")
    if survivors:
        for label, key in (("atFailure", "atFailure"), ("residual", "residual")):
            scan = survivors[key]
            names = "+".join(scan["names"]) if scan["names"] else "-"
            parts.append(f"; survivors.{label}={scan['scan']}:{scan['count']}({names})")
    return "".join(parts)


def render(view):
    lines = ["# Codex process diagnostic", "", "- Harness: `codex`", "", "| Case | Status | Reason | Duration (ms) | Evidence |", "| --- | --- | --- | ---: | --- |"]
    for item in view["cases"]:
        evidence = item.get("evidence")
        detail = ""
        if evidence:
            root = evidence["rootExit"]
            root_text = root["kind"] + (f":{root['value']}" if "value" in root else "")
            detail = (f"event={evidence['event']}; capture={evidence['capture']}; root={root_text}; stdout={evidence['stdout']['eof']}@{evidence['stdout']['atMilliseconds']}ms; "
                      f"stderr={evidence['stderr']['eof']}@{evidence['stderr']['atMilliseconds']}ms; "
                      f"cleanup={evidence['cleanup']['stage']}; osErrorCode={evidence['cleanup'].get('osErrorCode', 'unavailable')}; marker={str(evidence['marker']['observed']).lower()}; "
                      f"requests={evidence['provider']['requests']}; roundTrip={str(evidence['provider']['roundTrip']).lower()}{_detail_observation(evidence)}")
        lines.append(f"| `{item['id']}` | `{item['status']}` | `{item['reason']}` | {item['durationMilliseconds']} | {detail} |")
    totals = view["overall"]
    lines += ["", f"Totals: executed={totals['executed']}, passed={totals['passed']}, failed={totals['failed']}, blocked={totals['blocked']}", f"Duration: {view['durationMilliseconds']} ms", ""]
    return "\n".join(lines)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--prepare", action="store_true")
    parser.add_argument("--cell", type=Path)
    parser.add_argument("--model", default="qwen3.6")
    parser.add_argument("--setup-report", type=Path)
    parser.add_argument("--timeout-seconds", type=float, default=900)
    parser.add_argument("--blocked-reason", choices=("unsafe_prerequisite", "unavailable", "deadline_exceeded"))
    args = parser.parse_args(argv)
    if args.prepare:
        if args.cell is None or args.setup_report is None:
            parser.error("--prepare requires --cell and --setup-report")
        return prepare_codex(args.cell, args.setup_report, args.model, args.timeout_seconds)
    if args.blocked_reason:
        blocked_report(args.report, args.blocked_reason)
        return 2
    if args.output is None:
        parser.error("--output is required unless preparing or writing a blocked report")
    try:
        view = safe_view(load_report(args.report))
    except UnsafeReport:
        args.output.write_text("## Safe diagnostic summary unavailable\n\nThe Codex report was missing or malformed; no diagnostic facts were published.\n", encoding="utf-8")
        return 2
    args.output.write_text(render(view), encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
