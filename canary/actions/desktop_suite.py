#!/usr/bin/env python3
"""Run one bounded desktop suite cell with private, sanitized evidence.

Preparation is intentionally separate from checks.  This lets a hosted caller
reuse an installation receipt while keeping branch-source and release-asset
qualification identities explicit.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(Path(__file__).resolve().parent))
from selection import DESKTOP_HARNESSES
PLATFORMS = {"linux", "macos", "windows"}
SHA256 = re.compile(r"[0-9a-f]{64}\Z")
COMMIT = re.compile(r"[0-9a-f]{40}\Z")


class StageTimeout(RuntimeError):
    """The checker and its process group did not stop within the cell bound."""


def validate_identity(source, source_sha, model, apps, platform, release_tag=""):
    """Validate the closed provenance contract before starting an app."""
    if source not in ("branch", "release"):
        raise ValueError("source must be branch or release")
    pattern = COMMIT if source == "branch" else SHA256
    if not isinstance(source_sha, str) or not pattern.fullmatch(source_sha):
        raise ValueError("source identity is invalid")
    if source == "release" and not re.fullmatch(r"v(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)", release_tag):
        raise ValueError("release tag is invalid")
    if not isinstance(model, str) or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._:/-]{0,127}", model):
        raise ValueError("model is invalid")
    if platform not in PLATFORMS:
        raise ValueError("platform is invalid")
    if not apps or len(apps) != len(set(apps)) or any(app not in DESKTOP_HARNESSES for app in apps):
        raise ValueError("apps must be a distinct known desktop selection")
    return tuple(app for app in DESKTOP_HARNESSES if app in apps)


def suite_cell(selection, platform, source, source_sha, model, release_tag=""):
    """Return one OS cell; applications remain sequential within that cell."""
    if selection.get("suite") != "desktop":
        raise ValueError("desktop suite selection required")
    apps = validate_identity(source, source_sha, model, selection.get("harnesses", ()), platform, release_tag)
    selected = [job for job in selection.get("platforms", []) if job.get("system") == platform]
    if len(selected) != 1 or selected[0].get("harnesses") != list(apps):
        raise ValueError("platform selection does not match the desktop cell")
    return {
        "suite": "desktop", "platform": platform, "runner": selected[0]["runner"],
        "architecture": selected[0]["architecture"], "target": selected[0]["target"],
        "apps": list(apps), "mode": selection.get("mode", "deterministic"),
        "model": model, "source": source, "sourceSha": source_sha,
        "releaseTag": release_tag,
    }


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write_json(path, value):
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    if os.name == "nt":
        # RUNNER_TEMP is private on hosted Windows, but a caller may redirect
        # output.  Protect the directory before creating the first payload.
        username = os.environ.get("USERNAME", "")
        if not username or subprocess.run(
                ["icacls", str(path.parent), "/inheritance:r", "/grant:r",
                 f"{username}:F", "SYSTEM:F"], stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL, check=False).returncode:
            raise OSError("private Windows output directory is unavailable")
    with tempfile.NamedTemporaryFile(mode="w", dir=path.parent, delete=False) as output:
        json.dump(value, output, sort_keys=True)
        output.write("\n")
        output.flush()
        os.fsync(output.fileno())
    os.replace(output.name, path)


def checker_command(checker, stage, cell, receipt, output, nan_harness=None):
    """Build a command without credentials or shell interpolation."""
    command = [str(checker), "run", "--yes", "--non-interactive", "--ephemeral",
               "--mode", stage, "--model", cell["model"], "--prepared", str(receipt)]
    for app in cell["apps"]:
        command.extend(("--app", app))
    # A prepared receipt binds the tested nanh identity.  Passing another
    # binary would let a caller accidentally qualify a different executable.
    command.extend(("--output", str(output)))
    if cell["platform"] == "windows":
        command.extend(("--session", "github-hosted"))
    return command


def run_stage(command, timeout=3600, live=False):
    """Run a checker with all output discarded; return only a closed status."""
    environment = os.environ.copy()
    environment["CI"] = "1"
    if not live:
        environment.pop("NAN_API_KEY", None)
    creationflags = getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0) if os.name == "nt" else 0
    try:
        child = subprocess.Popen(command, cwd=ROOT, env=environment,
                                 stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                 start_new_session=os.name != "nt", creationflags=creationflags)
        result = child.wait(timeout=timeout)
    except subprocess.TimeoutExpired:
        if os.name == "nt":
            subprocess.run(["taskkill", "/PID", str(child.pid), "/T", "/F"],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)
        else:
            os.killpg(child.pid, 9)
        child.wait()
        raise StageTimeout from None
    except OSError:
        return False
    return result == 0


def initial_state(cell, checker, nan_harness, prepared):
    """Create the report envelope; app output never enters this state."""
    return {
        "schemaVersion": 1, "suite": "desktop", "platform": cell["platform"],
        "architecture": cell["architecture"], "apps": cell["apps"],
        "model": cell["model"], "source": cell["source"],
        "sourceSha": cell["sourceSha"], "releaseTag": cell["releaseTag"],
        "checkerSha256": digest(checker),
        "nanhSha256": digest(nan_harness), "preparedSha256": digest(prepared),
        "stages": [], "outcome": "blocked",
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--selection", type=Path, required=True)
    parser.add_argument("--platform", required=True, choices=sorted(PLATFORMS))
    parser.add_argument("--source", required=True, choices=("branch", "release"))
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--release-tag", default="")
    parser.add_argument("--model", required=True)
    parser.add_argument("--checker", type=Path, required=True)
    parser.add_argument("--nan-harness", type=Path, required=True)
    parser.add_argument("--prepared", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--stage", choices=("deterministic", "live"), required=True)
    args = parser.parse_args()
    try:
        selection = json.loads(args.selection.read_bytes())
        cell = suite_cell(selection, args.platform, args.source, args.source_sha, args.model, args.release_tag)
        if args.stage == "live" and not os.environ.get("NAN_API_KEY", "").strip():
            raise ValueError("live stage requires an explicitly supplied key")
        if not args.checker.is_file() or not args.nan_harness.is_file() or not args.prepared.is_file():
            raise ValueError("checker, nanh and prepared receipt are required")
        output = args.output
        state = initial_state(cell, args.checker, args.nan_harness, args.prepared) if not output.exists() else json.loads(output.read_bytes())
        expected = initial_state(cell, args.checker, args.nan_harness, args.prepared)
        identity_fields = ("platform", "architecture", "apps", "model", "source", "sourceSha",
                           "releaseTag", "checkerSha256", "nanhSha256", "preparedSha256")
        if any(state.get(field) != expected[field] for field in identity_fields):
            raise ValueError("suite state identity changed; start a new private run")
        if any(stage.get("name") == args.stage for stage in state["stages"]):
            raise ValueError("stage was already recorded")
        if args.stage == "live" and not any(stage.get("name") == "deterministic" and stage.get("status") == "passed" for stage in state["stages"]):
            raise ValueError("live stage requires a passing deterministic prerequisite")
        command = checker_command(args.checker, args.stage, cell, args.prepared, args.report,
                                  args.nan_harness if cell["source"] == "branch" else None)
        try:
            passed = run_stage(command, live=args.stage == "live")
        except StageTimeout:
            state["stages"].append({"name": args.stage, "status": "blocked", "reason": "cleanup"})
            state["cleanup"] = "blocked"
            state["outcome"] = "blocked"
            write_json(output, state)
            return 1
        state["stages"].append({"name": args.stage, "status": "passed" if passed else "failed"})
        deterministic = any(stage.get("name") == "deterministic" and stage.get("status") == "passed" for stage in state["stages"])
        live = any(stage.get("name") == "live" and stage.get("status") == "passed" for stage in state["stages"])
        state["outcome"] = "passed" if deterministic and (args.stage == "deterministic" or live) else "failed"
        write_json(output, state)
        return 0 if passed else 1
    except (OSError, ValueError, json.JSONDecodeError, KeyError, TypeError):
        return 2


if __name__ == "__main__":
    sys.exit(main())
