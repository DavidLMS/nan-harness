#!/usr/bin/env python3
"""Observe four private MiMo inventory captures; never qualify a release."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
import windows_diagnostic as windows
from windows_summary import _inventory_process

SOURCE = re.compile(r"[0-9a-f]{40}\Z")
DIGEST = re.compile(r"[0-9a-f]{64}\Z")
RELEASE_COMMIT = "0be5d88fdefe020e8c302f6538756869042c01fd"
FORMATS = ("json", "default", "json", "default")
EVENTS = frozenset(("text", "stepStart", "stepFinish", "toolUse", "error", "reasoning", "unknown", "invalid", "nonEvent"))
REASONS = frozenset(("source-invalid", "provenance-invalid", "binding-changed", "version-unavailable", "version-mismatch", "installer-failed", "observer-timeout", "observer-nonzero", "observer-launch-failed", "observations-invalid", "observations-incomplete", "observer-failed"))
PROVENANCE_KEYS = frozenset(("schemaVersion", "controllerSha", "releaseTag", "releaseCommit", "assetRunId", "assetWorkflowSha", "artifactId", "artifactSha256", "manifestSha256", "nanHarnessSha256", "canarySha256"))


def integer(value, low, high):
    return type(value) is int and low <= value <= high


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate field")
        result[key] = value
    return result


def read_json(path):
    if path.is_symlink() or not path.is_file():
        raise ValueError("invalid observation file")
    with path.open("rb") as stream:
        raw = stream.read(65537)
    if len(raw) > 65536:
        raise ValueError("observation exceeds bound")
    return json.loads(raw.decode("utf-8"), object_pairs_hook=unique_object)


def digest(path):
    if path.is_symlink() or not path.is_file():
        raise ValueError("invalid binary")
    result = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            result.update(block)
    return result.hexdigest()


def binding(args):
    value = read_json(args.provenance)
    if (type(value) is not dict or set(value) != PROVENANCE_KEYS
            or type(value["schemaVersion"]) is not int or value["schemaVersion"] != 1
            or value["controllerSha"] != args.source_sha
            or value["releaseTag"] != "v0.1.26" or value["releaseCommit"] != RELEASE_COMMIT
            or any(type(value[key]) is not str or not SOURCE.fullmatch(value[key])
                   for key in ("controllerSha", "releaseCommit", "assetWorkflowSha"))
            or any(type(value[key]) is not str or not DIGEST.fullmatch(value[key])
                   for key in ("artifactSha256", "manifestSha256", "nanHarnessSha256", "canarySha256"))
            or any(not integer(value[key], 1, 2**63-1) for key in ("assetRunId", "artifactId"))
            or digest(args.binary) != value["nanHarnessSha256"]):
        raise ValueError("release binding differs")
    return value


def process(value):
    if value is None:
        return None
    if type(value) is not dict or any(item is None for item in value.values()):
        raise ValueError("invalid process observation")
    return _inventory_process(value, "MiMo")


def capture(value):
    if (type(value) is not dict or set(value) != {"stdout", "stderr", "events", "decodedTextMarker", "orderedTextMarker"}
            or any(type(value[key]) is not bool for key in ("decodedTextMarker", "orderedTextMarker"))):
        raise ValueError("invalid capture observation")
    for name in ("stdout", "stderr"):
        stream = value[name]
        if (type(stream) is not dict or set(stream) != {"retainedBytes", "truncated", "marker"}
                or not integer(stream["retainedBytes"], 0, 196608)
                or any(type(stream[key]) is not bool for key in ("truncated", "marker"))):
            raise ValueError("invalid retained stream")
    events = value["events"]
    if (type(events) is not dict or set(events) != EVENTS
            or any(not integer(count, 0, 196608) for count in events.values())
            or sum(events.values()) > value["stdout"]["retainedBytes"]):
        raise ValueError("invalid event counts")
    return value


def observations(path, partial=False):
    values = read_json(path)
    if type(values) is not list or len(values) > 4 or not partial and len(values) != 4:
        raise ValueError("invalid observation count")
    for index, value in enumerate(values):
        if (type(value) is not dict or set(value) != {"index", "format", "process", "provider", "capture", "setupError"}
                or type(value["index"]) is not int or value["index"] != index
                or type(value["format"]) is not str or value["format"] != FORMATS[index]
                or value["setupError"] not in (None, "workspace", "provider")):
            raise ValueError("invalid inventory record")
        if value["setupError"] is not None:
            if any(value[key] is not None for key in ("process", "provider", "capture")):
                raise ValueError("unbound setup observation")
            continue
        process(value["process"])
        provider = value["provider"]
        if (type(provider) is not dict or set(provider) != {"requestCount", "completed", "bounded", "shutdown"}
                or not integer(provider["requestCount"], 0, 128)
                or any(type(provider[key]) is not bool for key in ("completed", "bounded", "shutdown"))):
            raise ValueError("invalid provider observation")
        captured = value["process"] is not None and value["process"]["status"] in ("completed", "nonzero-exit")
        if captured:
            capture(value["capture"])
        elif value["capture"] is not None:
            raise ValueError("capture unavailable after process error")
    return values


def collect(args):
    source = args.source_sha if type(args.source_sha) is str and SOURCE.fullmatch(args.source_sha) else None
    report = dict(schemaVersion=1, diagnosticsOnly=True, qualification=False, observerKind="source-test",
                  observerSourceSha=source, releaseProvenance=None, stockVersion="0.1.16",
                  status="failed", reason="source-invalid", observations=[])
    if (type(args.source_sha) is not str or not SOURCE.fullmatch(args.source_sha)
            or os.environ.get("GITHUB_SHA", args.source_sha) != args.source_sha):
        return report
    report["reason"] = "provenance-invalid"
    failure = "provenance-invalid"
    try:
        original = binding(args)
        report["releaseProvenance"] = original
        failure = "version-unavailable"
        item, error = windows.resolve_one(windows.resolver_module(), "mimo-code", "qwen3.6")
        if item is None or error:
            report["reason"] = "version-unavailable"
            return report
        if item.version != "0.1.16":
            report["reason"] = "version-mismatch"
            return report
        cell = args.directory / "cell"
        cell.mkdir(mode=0o700)
        windows.protect_private(cell)
        isolated = windows.isolated_environment(cell)
        install_env = dict(isolated)
        tokens = {key.upper(): value for key, value in os.environ.items() if key.upper() in {"GITHUB_TOKEN", "GH_TOKEN"}}
        token = tokens.get("GITHUB_TOKEN") or tokens.get("GH_TOKEN")
        if token:
            install_env["GITHUB_TOKEN"] = token
        installer = windows.ROOT / "canary/guest/install-harness.ps1"
        command = ["pwsh", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", str(installer),
                   "-Harness", "mimo-code", "-Version", item.version, "-PythonVersion", "3.12"]
        if item.ref:
            command += ["-Ref", item.ref]
        report["reason"] = "installer-failed"
        failure = "installer-failed"
        if windows.run_bounded(command, cell, install_env, 180)[0] != 0:
            return report
        failure = "binding-changed"
        if binding(args) != original:
            raise ValueError("release binding changed")
        # Cargo keeps its build/toolchain caches; the Rust observer independently
        # clears every stock harness child's environment inside a fresh workspace.
        excluded = {"GH_TOKEN", "GITHUB_TOKEN", "NAN_API_KEY", "PATH", "NAN_MIMO_INVENTORY_BINARY", "NAN_MIMO_INVENTORY_OUTPUT"}
        env = {key: value for key, value in os.environ.items() if key.upper() not in excluded}
        raw = args.directory / "observations.private.json"
        env.update(PATH=isolated["PATH"], NAN_MIMO_INVENTORY_BINARY=str(args.binary.resolve()), NAN_MIMO_INVENTORY_OUTPUT=str(raw.resolve()))
        argv = ["cargo", "test", "--locked", "-p", "nan-harness-test-support", "--lib",
                "conformance::runner::inventory_diagnostic::native_mimo_inventory_output", "--", "--ignored", "--exact"]
        report["reason"] = "observer-failed"
        failure = "observer-failed"
        code, outcome = windows.run_bounded(argv, windows.ROOT, env, 480)
        failure = "binding-changed"
        if binding(args) != original:
            raise ValueError("release binding changed")
        failed = code != 0
        report["reason"] = ({"timeout": "observer-timeout", "nonzero": "observer-nonzero", "launch-failed": "observer-launch-failed"}.get(outcome, "observer-failed") if failed else "observations-incomplete")
        if raw.exists():
            failure = "observations-invalid"
            report["observations"] = observations(raw, partial=True)
        if not failed and len(report["observations"]) == 4:
            report.update(status="observed", reason=None)
        return report
    except Exception:
        # Exception text and child output are private; only this fixed phase
        # label crosses the report boundary.
        report.update(reason=failure, observations=[])
        return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--provenance", required=True, type=Path)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--directory", required=True, type=Path)
    args = parser.parse_args()
    try:
        if args.directory.exists() or args.directory.is_symlink():
            raise ValueError("output directory already exists")
        args.directory.mkdir(mode=0o700)
        windows.protect_private(args.directory)
        report = collect(args)
        destination = args.directory / "mimo-inventory-diagnostic.json"
        with destination.open("x", encoding="utf-8") as output:
            json.dump(report, output, sort_keys=True)
            output.write("\n")
        windows.protect_private(destination)
    except Exception:
        print("MiMo inventory observation could not be recorded safely.")
        return 1
    return 0 if report["status"] == "observed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
