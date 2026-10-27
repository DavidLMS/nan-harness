#!/usr/bin/env python3
"""Observe the canonical MiMo cell with immutable release binaries; never qualify it."""
import argparse
from contextlib import contextmanager
import json
import os
from pathlib import Path
import shutil
import sys
from types import SimpleNamespace

sys.path.insert(0, str(Path(__file__).resolve().parent))
import cell
import mimo_inventory_diagnostic as inventory

SCENARIOS = ("inventory", "tool-round-trip", "sentinel", "external-prerequisite")
CHECKS = ("contract", "native-configuration")
MAX_DURATION = 86400000


def binding(args):
    receipt = inventory.binding(args)
    if inventory.digest(args.canary) != receipt["canarySha256"]:
        raise ValueError("canary binding differs")
    return receipt


@contextmanager
def deterministic_environment():
    # Match cli-suite's environment for its install and conformance stage processes.
    removed = {key: os.environ.pop(key) for key in ("NAN_API_KEY", "GITHUB_TOKEN") if key in os.environ}
    try:
        yield
    finally:
        os.environ.update(removed)


def frozen_selection(args):
    inventory.read_json(args.manifest)
    suite = inventory.windows.resolver_module()
    items = suite.read_frozen_manifest(args.manifest, ["mimo-code"], "windows", "x86_64", "qwen3.6")
    if len(items) != 1 or items[0].version != "0.1.16" or items[0].ref or items[0].runtime_version:
        raise ValueError("frozen stock version differs")
    return items[0]


def cell_arguments(args, item):
    temporary = os.environ.get("RUNNER_TEMP")
    if (not temporary or not args.directory.is_absolute() or args.directory.is_symlink()
            or args.directory.exists() or args.directory.name != "cli-cell"
            or args.directory.resolve().parent != Path(temporary).resolve()
            or args.output.resolve().is_relative_to(args.directory.resolve())):
        raise ValueError("canonical cell layout differs")
    # cli-suite receives RUNNER_TEMP/cli-cell and adds the harness identifier.
    cell.ensure_private_directory(args.directory)
    directory = args.directory / "mimo-code"
    cell.ensure_private_directory(directory)
    return SimpleNamespace(harness="mimo-code", harness_version=item.version,
                           harness_ref=item.ref, runtime_version=item.runtime_version,
                           binary=args.binary, canary=args.canary, directory=directory,
                           model="qwen3.6", mode="deterministic")


def location(path, directory):
    if path is None:
        return "missing"
    selected = Path(path).resolve()
    for label, roots in (
        ("cell", [directory]),
        ("runner-toolcache", [os.environ.get("RUNNER_TOOL_CACHE")]),
        ("runner-temp", [os.environ.get("RUNNER_TEMP")]),
        ("system", [os.environ.get("SystemRoot"), os.environ.get("WINDIR")]),
        ("program-files", [os.environ.get(key) for key in ("ProgramFiles", "ProgramFiles(x86)", "ProgramW6432")]),
        ("user-home", [os.environ.get("HOME"), os.environ.get("USERPROFILE")]),
    ):
        if any(root and selected.is_relative_to(Path(root).resolve()) for root in roots):
            return label
    return "other"


def prerequisites(directory):
    environment = cell.cell_environment(directory)
    tools = {}
    selected_git = None
    for name, executable in (("node", "node"), ("python", "python"), ("git", "git.exe"), ("bash", "bash.exe")):
        selected = shutil.which(executable, path=environment["PATH"])
        tools[name] = {"found": selected is not None, "location": location(selected, directory)}
        if name == "git":
            selected_git = selected
    git_location = "missing"
    if selected_git:
        parent = Path(selected_git).parent
        if parent.name.casefold() == "cmd":
            git_location = "cmd"
        elif parent.name.casefold() == "bin":
            git_location = "mingw-bin" if parent.parent.name.casefold() in ("mingw32", "mingw64") else "bin"
        else:
            git_location = "other"
    chosen = environment.get("NAN_HARNESS_GIT_BASH")
    node = environment.get("NAN_CANARY_EXPECTED_NODE_VERSION")
    if type(node) is not str or not cell.SEMVER.fullmatch(node):
        node = None
    return {"tools": tools, "gitLocation": git_location,
            "chosenGitBashExists": bool(chosen and Path(chosen).is_file()),
            "runtimes": {"node": node, "python": ".".join(str(value) for value in sys.version_info[:3])}}


def project_attempt(attempt, status, result):
    if (not inventory.integer(attempt, 1, 2) or not inventory.integer(status, -2147483648, 4294967295)
            or type(result) is not dict or type(result.get("schemaVersion")) is not int
            or result["schemaVersion"] not in (1, 2) or result.get("harness") != "mimo-code"
            or type(result.get("outcome")) is not str or result["outcome"] not in ("passed", "failed")
            or not inventory.integer(result.get("durationMilliseconds"), 0, MAX_DURATION)
            or type(result.get("scenarios")) is not list or len(result["scenarios"]) != 4):
        raise ValueError("invalid canonical observation")
    scenarios = []
    checks = None
    for value, name in zip(result["scenarios"], SCENARIOS):
        statuses = ("passed", "failed", "skipped") if name == "external-prerequisite" else ("passed", "failed")
        if (type(value) is not dict or value.get("name") != name
                or type(value.get("status")) is not str or value["status"] not in statuses
                or not inventory.integer(value.get("durationMilliseconds"), 0, MAX_DURATION)):
            raise ValueError("invalid canonical scenario")
        scenarios.append({"name": name, "status": value["status"], "durationMilliseconds": value["durationMilliseconds"]})
        if name == "tool-round-trip":
            checks = project_checks(value.get("checks"))
            if (value["status"] == "passed") != all(check["status"] == "passed" for check in checks):
                raise ValueError("contradictory canonical tool checks")
    failed = any(value["status"] == "failed" for value in scenarios)
    if (result["outcome"] == "failed") != failed or (status == 0) != (not failed):
        raise ValueError("contradictory canonical outcome")
    return {"attempt": attempt, "process": {"status": "completed" if status == 0 else "nonzero-exit", "exitCode": status},
            "scenarios": scenarios, "checks": checks, "durationMilliseconds": result["durationMilliseconds"]}


def project_checks(values):
    if type(values) is not list or len(values) != 2:
        raise ValueError("invalid canonical check count")
    result = []
    for value, name in zip(values, CHECKS):
        if (type(value) is not dict or value.get("name") != name
                or type(value.get("status")) is not str or value["status"] not in ("passed", "failed")
                or not inventory.integer(value.get("durationMilliseconds"), 0, MAX_DURATION)):
            raise ValueError("invalid canonical check")
        result.append({"name": name, "status": value["status"], "durationMilliseconds": value["durationMilliseconds"]})
    return result


def collect(args):
    source = args.source_sha if type(args.source_sha) is str and inventory.SOURCE.fullmatch(args.source_sha) else None
    report = dict(schemaVersion=1, diagnosticsOnly=True, qualification=False, observerKind="canonical-controller",
                  observerSourceSha=source, releaseProvenance=None, stockVersion="0.1.16", prerequisites=None,
                  installStatus="not-run", conformanceStatus="not-run", attempts=[], status="failed", reason="source-invalid")
    if source is None or os.environ.get("GITHUB_SHA", source) != source:
        return report
    phase = "provenance-invalid"
    try:
        original = binding(args)
        report["releaseProvenance"] = original
        phase = "manifest-invalid"
        item = frozen_selection(args)
        phase = "layout-invalid"
        command = cell_arguments(args, item)
        state = {"harness": {"id": "mimo-code", "version": "unknown"}, "environment": {"runtimes": []}}
        with deterministic_environment():
            phase = "install-failed"
            report["installStatus"] = "failed"
            try:
                cell.install(command, state)
                report["installStatus"] = "passed"
            finally:
                previous = phase
                phase = "binding-changed"
                if binding(args) != original:
                    raise ValueError("release binding changed")
                phase = previous
            phase = "prerequisites-unavailable"
            report["prerequisites"] = prerequisites(command.directory)
            report["conformanceStatus"] = "failed"
            phase = "conformance-failed"

            def observe(attempt, status, result):
                nonlocal phase
                try:
                    value = project_attempt(attempt, status, result)
                    if attempt != len(report["attempts"]) + 1:
                        raise ValueError("repeated canonical observation")
                    report["attempts"].append(value)
                except (ValueError, TypeError, KeyError):
                    phase = "observations-invalid"
                    raise ValueError("invalid closed canonical observation") from None

            try:
                completed = cell.conformance(command, state, observe=observe)
                if not report["attempts"] or completed != len(report["attempts"]):
                    phase = "observations-incomplete"
                    raise ValueError("canonical observations incomplete")
                report["conformanceStatus"] = "passed"
            finally:
                previous = phase
                phase = "binding-changed"
                if binding(args) != original:
                    raise ValueError("release binding changed")
                phase = previous
        report.update(status="observed", reason=None)
    except cell.CleanupError:
        report.update(reason="cleanup-unproven")
    except Exception:
        report["reason"] = phase
        if phase == "binding-changed":
            report["attempts"] = []
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("binary", "canary", "provenance", "manifest", "directory", "output"):
        parser.add_argument("--" + name, required=True, type=Path)
    parser.add_argument("--source-sha", required=True)
    args = parser.parse_args()
    created = False
    try:
        if (not args.output.is_absolute() or args.output.exists() or args.output.is_symlink()
                or not args.output.parent.is_dir()):
            raise ValueError("diagnostic destination is unavailable")
        report = collect(args)
        with args.output.open("x", encoding="utf-8") as output:
            created = True
            cell.protect_private(args.output)
            json.dump(report, output, sort_keys=True)
            output.write("\n")
    except Exception:
        if created:
            try:
                args.output.unlink(missing_ok=True)
            except OSError:
                pass
        print("Canonical MiMo observation could not be recorded safely.")
        return 1
    return 0 if report["status"] == "observed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
