#!/usr/bin/env python3
"""Run one hosted CLI cell; only closed, validated evidence leaves the runner.

Stages are separate processes so the installation/conformance steps never receive
the provider secret. All child output is private, even on a timeout or failure.
"""

import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, str(Path(__file__).resolve().parent))
from selection import CLI_HARNESSES, resolve_model

HARNESSES = CLI_HARNESSES
SEMVER = re.compile(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?\Z")
ROOT = Path(__file__).resolve().parents[2]
# Kept for the historical coverage helper; new workflow selection comes from
# selection.select_suite and includes the native Windows platform.
PLATFORMS = (("linux", "ubuntu-24.04-arm", "unknown-linux-musl", "aarch64"),
             ("macos", "macos-15", "apple-darwin", "aarch64"))
SMOKE_PLATFORMS = (("linux", "ubuntu-24.04", "unknown-linux-musl", "x86_64"),
                   ("macos", "macos-15", "apple-darwin", "aarch64"))
SMOKE_LIMIT = 4


def select_coverage(coverage, harnesses, ordinal, release_commit, workflow_commit):
    """Choose hosted cells, the per-cell trigger and the commit that supplies cell code.

    Only release coverage runs the release commit's own policy; it is also the only
    coverage the workflow can enqueue. Evidence-only coverage runs the dispatched
    workflow's code, so it can test releases that predate these scripts.
    """
    requested = [name for name in harnesses.split(",") if name] if harnesses else []
    if coverage == "smoke":
        if (not requested or len(requested) > SMOKE_LIMIT or len(set(requested)) != len(requested)
                or any(name not in HARNESSES for name in requested)):
            raise ValueError(f"smoke coverage needs 1-{SMOKE_LIMIT} distinct known harnesses")
        cells = [{"system": system, "runner": runner, "target": target,
                  "architecture": architecture,
                  "binary_asset": f"nan-harness-{architecture}-{target}",
                  "canary_asset": f"nan-harness-canary-{architecture}-{target}",
                  "canary_source": "source-build" if system == "linux" else "release-asset",
                  "harness": harness, "live": False}
                 for system, runner, target, architecture in SMOKE_PLATFORMS
                 for harness in requested]
        return {"cells": cells, "trigger": "manual", "source": workflow_commit}
    if coverage not in ("daily", "weekly", "release"):
        raise ValueError("unknown coverage")
    if requested:
        raise ValueError("only smoke coverage may select a harness subset")
    rotation = ordinal % len(HARNESSES)
    platforms = PLATFORMS[:1] if coverage == "daily" else PLATFORMS
    cells = [{"system": system, "runner": runner, "target": target,
              "architecture": architecture,
              "binary_asset": f"nan-harness-{architecture}-{target}",
              "canary_asset": f"nan-harness-canary-{architecture}-{target}",
              "canary_source": "release-asset",
              "harness": harness,
              "live": coverage != "daily" or index in (rotation, (rotation + 1) % len(HARNESSES))}
             for system, runner, target, architecture in platforms
             for index, harness in enumerate(HARNESSES)]
    source = release_commit if coverage == "release" else workflow_commit
    return {"cells": cells, "trigger": coverage, "source": source}


def timestamp():
    return datetime.datetime.now(datetime.timezone.utc).isoformat().replace("+00:00", "Z")


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write_json(path, value):
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(mode="w", dir=path.parent, delete=False) as out:
        json.dump(value, out, sort_keys=True)
        out.write("\n")
        out.flush()
        os.fsync(out.fileno())
    os.replace(out.name, path)


def private_command(command, directory, timeout=900, output=None, live=False, allow_failure=False):
    env = os.environ.copy()
    if not live:
        env.pop("NAN_API_KEY", None)
    env["NAN_CANARY_REDACT_FAILURE_OUTPUT"] = "1"
    env["CI"] = "1"
    with tempfile.TemporaryFile() as log:
        destination = output.open("wb") if output else log
        try:
            creationflags = getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0) if os.name == "nt" else 0
            child = subprocess.Popen(command, stdout=destination, stderr=log,
                                     env=env, cwd=directory, start_new_session=os.name != "nt",
                                     creationflags=creationflags)
            try:
                status = child.wait(timeout=timeout)
            except (subprocess.TimeoutExpired, KeyboardInterrupt):
                terminate_process_tree(child.pid)
                child.wait()
                raise RuntimeError("stage exceeded its execution limit") from None
            finally:
                # A harness may leave descendants after its foreground process exits.
                try:
                    terminate_process_tree(child.pid)
                except (OSError, ProcessLookupError):
                    pass
            if status and not allow_failure:
                raise RuntimeError("stage did not pass")
        finally:
            if output:
                destination.close()


def terminate_process_tree(pid):
    """Terminate a stage and descendants on both native POSIX and Windows hosts."""
    if os.name == "nt":
        subprocess.run(["taskkill", "/PID", str(pid), "/T", "/F"],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                       check=False, timeout=10)
    else:
        os.killpg(pid, signal.SIGKILL)


def install(args, state):
    installer = ROOT / "canary/guest/install-harness.ps1" if os.name == "nt" else ROOT / "canary/guest/install-harness.sh"
    command = ["powershell", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File",
               str(installer), args.harness] if os.name == "nt" else ["bash", str(installer), args.harness]
    private_command(command, args.directory)
    doctor = args.directory / "doctor.json"
    private_command([str(args.binary), "doctor", args.harness, "--allow-unsupported",
                     "--allow-untested", "--json"], args.directory, output=doctor)
    try:
        version = json.loads(doctor.read_bytes())["version"]
        if not isinstance(version, str) or not SEMVER.fullmatch(version):
            raise ValueError()
        state["harness"]["version"] = version
    except (KeyError, ValueError, TypeError):
        raise RuntimeError("installed version could not be verified") from None
    finally:
        doctor.unlink(missing_ok=True)


def conformance(args, state):
    report = args.directory / "conformance-private.json"
    try:
        private_command([str(args.canary), "conformance", "--nan-harness", str(args.binary),
                         "--harness", args.harness, "--json"], args.directory, output=report,
                        allow_failure=True)
        private_command(["bash", str(ROOT / "canary/guest/evaluate-conformance.sh"),
                         str(report), args.harness], args.directory)
        result = json.loads(report.read_bytes())
        if any(scenario["name"] == "inventory" and scenario["status"] == "failed"
               for scenario in result["scenarios"]):
            identity = f"{args.harness}:{state['harness']['version']}:inventory-drift"
            state["observations"] = [{"kind": "inventory-drift",
                                      "fingerprint": hashlib.sha256(identity.encode()).hexdigest()}]
    finally:
        report.unlink(missing_ok=True)


def live(args, _state):
    if not os.environ.get("NAN_API_KEY"):
        raise RuntimeError("live stage requires an explicitly supplied key")
    os.environ["NAN_CANARY_NAN_COMMAND"] = str(args.binary)
    os.environ["NAN_CANARY_MODEL"] = args.model
    probe = ROOT / "canary/guest/probe-harness.ps1" if os.name == "nt" else ROOT / "canary/guest/probe-harness.sh"
    command = ["powershell", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File",
               str(probe), args.harness] if os.name == "nt" else ["bash", str(probe), args.harness]
    private_command(command,
                    args.directory, timeout=600, live=True)


def initial_state(args):
    platform = {"linux": "linux", "darwin": "macos", "win32": "windows"}.get(sys.platform)
    machine = os.environ.get("PROCESSOR_ARCHITECTURE", "") if os.name == "nt" else getattr(os, "uname")().machine
    if platform is None or machine.lower() not in ("arm64", "aarch64", "x86_64", "amd64"):
        raise RuntimeError("this gate requires a supported hosted runner")
    architecture = "x86_64" if machine.lower() in ("x86_64", "amd64") else "aarch64"
    if platform == "windows" and architecture != "x86_64":
        raise RuntimeError("Windows CLI qualification requires x86_64")
    if architecture == "x86_64" and platform != "windows" and not (platform == "linux" and args.trigger == "manual"):
        raise RuntimeError("x86_64 is supported only by the Linux manual smoke gate")
    node = subprocess.run(["node", "-p", "process.versions.node"], check=True,
                          capture_output=True, timeout=10).stdout.decode().strip()
    if node != "24.20.0":
        raise RuntimeError("hosted Node runtime does not match the pinned version")
    tier = {"release": "release-gate", "weekly": "live-extended",
            "daily": "deterministic", "manual": "deterministic"}[args.trigger]
    return {
        "schemaVersion": 2, "runId": args.run_id,
        "cellId": f"{platform}-{args.harness}-{args.trigger}",
        "specSha256": digest(Path(__file__)), "trigger": args.trigger, "tier": tier,
        "scenario": "hosted-clean-install-deterministic-and-live-tool",
        "startedAt": timestamp(), "completedAt": timestamp(), "durationMilliseconds": 0,
        "nanHarness": {"version": args.tag[1:], "source": "release:" + args.tag,
                       "sha256": digest(args.binary)},
        "environment": {"operatingSystem": platform, "architecture": architecture,
                        "image": "github-hosted", "profile": "clean-" + platform,
                        "runtimes": [{"name": "node", "version": node},
                                     {"name": "python", "version": ".".join(str(v) for v in sys.version_info[:3])}]},
        "harness": {"id": args.harness, "version": "unknown"}, "checks": [],
        "outcome": "passed",
    }


def run(args):
    state_path = args.directory / "state.json"
    state = json.loads(state_path.read_bytes()) if state_path.exists() else initial_state(args)
    if not state_path.exists():
        write_json(state_path, state)
    if state["nanHarness"]["sha256"] != digest(args.binary) or state["harness"]["id"] != args.harness:
        raise RuntimeError("cell identity changed between stages")
    steps = {"install": ("install-and-diagnose", install),
             "conformance": ("deterministic-conformance", conformance),
             "live": ("live-tool", live)}
    if args.stage == "report":
        required = ["install-and-diagnose", "deterministic-conformance"]
        optional_live = args.trigger in ("daily", "manual")
        if not optional_live or any(c["name"] == "live-tool" for c in state["checks"]):
            required.append("live-tool")
        if [check["name"] for check in state["checks"]] != required:
            raise RuntimeError("cell has incomplete or repeated stages")
        state["completedAt"] = timestamp()
        if "live-tool" in required:
            state["model"] = args.model
            if optional_live:
                state["tier"] = "live-core"
        write_json(args.output, state)
        try:
            private_command([str(args.canary), "validate-report", str(args.output)], args.directory)
        except RuntimeError:
            args.output.unlink(missing_ok=True)
            raise
        return
    name, execute = steps[args.stage]
    index = ["install", "conformance", "live"].index(args.stage)
    if len(state["checks"]) != index:
        raise RuntimeError("cell stages must execute once in order")
    started = time.monotonic()
    execute(args, state)
    duration = int((time.monotonic() - started) * 1000)
    state["checks"].append({"name": name, "status": "passed",
                            "durationMilliseconds": duration, "attempts": 1})
    state["durationMilliseconds"] += duration
    write_json(state_path, state)


def failed_report(args):
    state_path = args.directory / "state.json"
    state = json.loads(state_path.read_bytes()) if state_path.exists() else initial_state(args)
    phase, failure_class = {
        "install": ("install-and-diagnose", "installation"),
        "conformance": ("deterministic-conformance", "harness"),
        "live": ("live-tool", "harness"),
        "report": ("report-validation", "test-contract"),
    }[args.stage]
    state["completedAt"] = timestamp()
    elapsed = (datetime.datetime.fromisoformat(state["completedAt"].replace("Z", "+00:00"))
               - datetime.datetime.fromisoformat(state["startedAt"].replace("Z", "+00:00")))
    total = max(state["durationMilliseconds"], int(elapsed.total_seconds() * 1000))
    state["checks"].append({"name": phase, "status": "failed",
                            "durationMilliseconds": total - state["durationMilliseconds"], "attempts": 1})
    state["durationMilliseconds"] = total
    state["outcome"] = "failed"
    fingerprint = hashlib.sha256(f"{args.harness}:{phase}:{failure_class}".encode()).hexdigest()
    state["failure"] = {"class": failure_class, "phase": phase,
                        "summary": "Hosted check did not complete successfully.", "fingerprint": fingerprint}
    write_json(args.output, state)
    try:
        private_command([str(args.canary), "validate-report", str(args.output)], args.directory)
    except (OSError, RuntimeError):
        args.output.unlink(missing_ok=True)


def main():
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("stage", choices=("install", "conformance", "live", "report"))
    parser.add_argument("--harness", choices=HARNESSES, required=True)
    parser.add_argument("--trigger", choices=("release", "weekly", "daily", "manual"), required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--canary", type=Path, required=True)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--model", default="")
    args = parser.parse_args()
    try:
        args.model = resolve_model(args.model)
    except ValueError as error:
        parser.error(str(error))
    for field in ("binary", "canary", "directory", "output"):
        setattr(args, field, getattr(args, field).resolve())
    if not args.tag.startswith("v") or not SEMVER.fullmatch(args.tag[1:]):
        parser.error("expected a semantic release tag")
    args.directory.mkdir(parents=True, mode=0o700, exist_ok=True)
    try:
        run(args)
    except (OSError, ValueError, KeyError, RuntimeError, subprocess.SubprocessError):
        try:
            failed_report(args)
        except (OSError, ValueError, KeyError, RuntimeError, subprocess.SubprocessError):
            args.output.unlink(missing_ok=True)
        # Exception messages and child logs can contain provider/user-controlled text.
        print("Hosted CLI cell failed; no private process output was published.", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
