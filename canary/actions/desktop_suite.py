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
from cell import CleanupError, ensure_private_directory, private_command, write_json
PLATFORMS = {"linux", "macos", "windows"}
ARCHITECTURES = {"linux": {"x86_64", "aarch64"},
                 "macos": {"aarch64"}, "windows": {"x86_64"}}
MANIFEST_LIMIT = 64 * 1024
APP_FIELDS = {"status", "app", "version", "runtimeVersion", "channel", "url", "format",
              "digest", "revision", "staged", "installer", "reason", "evidence"}
MANIFEST_FIELDS = {"schemaVersion", "suite", "platform", "architecture", "model", "apps"}
SHA256 = re.compile(r"[0-9a-f]{64}\Z")
COMMIT = re.compile(r"[0-9a-f]{40}\Z")
TAG = re.compile(r"v(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\Z")
VERSION = re.compile(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\Z")
PRERELEASE_PART = r"(?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)"
RUNTIME_VERSION = re.compile(
    r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)"
    rf"(?:-{PRERELEASE_PART}(?:\.{PRERELEASE_PART})*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?\Z")


def unique_object(pairs):
    value = {}
    for key, item in pairs:
        if key in value:
            raise ValueError("duplicate manifest field")
        value[key] = item
    return value


def read_frozen_manifest(path, apps, platform, architecture, model):
    """Read and validate one exact, private Rust frozen-manifest cell.

    The returned object is a complete manifest with entries in the caller's
    requested order.  URLs are treated as untrusted input: only the catalog's
    known publisher hosts and the Rust entry identity rules are accepted.
    """
    path = Path(path)
    if path.is_symlink() or not path.is_file() or path.stat().st_size > MANIFEST_LIMIT:
        raise ValueError("frozen manifest is not a bounded regular file")
    try:
        with path.open("rb") as source:
            raw = source.read(MANIFEST_LIMIT + 1)
        if len(raw) > MANIFEST_LIMIT:
            raise ValueError("frozen manifest exceeds its bound")
        value = json.loads(raw, object_pairs_hook=unique_object)
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError("frozen manifest is invalid") from error
    if not isinstance(value, dict) or set(value) != MANIFEST_FIELDS:
        raise ValueError("frozen manifest fields are invalid")
    if type(value["schemaVersion"]) is not int or value["schemaVersion"] != 1 or value["suite"] != "desktop":
        raise ValueError("frozen manifest identity is invalid")
    if platform not in PLATFORMS or architecture not in ARCHITECTURES[platform]:
        raise ValueError("frozen manifest target is invalid")
    if not isinstance(model, str) or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._:/-]{0,127}", model):
        raise ValueError("model is invalid")
    if value["platform"] != platform or value["architecture"] != architecture or value["model"] != model:
        raise ValueError("frozen manifest target or model differs")
    requested = list(apps)
    if (not requested or len(requested) != len(set(requested))
            or any(app not in DESKTOP_HARNESSES for app in requested)):
        raise ValueError("apps must be a distinct known desktop selection")
    entries = value["apps"]
    if not isinstance(entries, list) or len(entries) != len(requested):
        raise ValueError("frozen manifest app selection differs")
    by_app = {}
    for entry in entries:
        _validate_manifest_entry(entry, platform, architecture)
        app = entry["app"]
        if app in by_app:
            raise ValueError("frozen manifest contains duplicate apps")
        by_app[app] = entry
    if set(by_app) != set(requested):
        raise ValueError("frozen manifest app selection differs")
    return {**value, "apps": [by_app[app] for app in requested]}


def _validate_manifest_entry(entry, platform, architecture):
    if not isinstance(entry, dict) or "status" not in entry or set(entry) - APP_FIELDS:
        raise ValueError("frozen manifest entry fields are invalid")
    app = entry.get("app")
    if app not in DESKTOP_HARNESSES or not isinstance(entry["status"], str):
        raise ValueError("frozen manifest entry identity is invalid")
    if entry["status"] == "blocked":
        if set(entry) != {"status", "app", "reason", "evidence"}:
            raise ValueError("blocked manifest entry is invalid")
        if (not isinstance(entry["reason"], str)
                or entry["reason"] not in {"upstream-unsupported", "unqualified-platform", "resolution-failed"}):
            raise ValueError("blocked manifest reason is invalid")
        expected_reason, expected_evidence = _blocked_entry(app, platform, architecture)
        if entry["reason"] != expected_reason or entry["evidence"] != expected_evidence:
            raise ValueError("blocked manifest evidence is invalid")
        return
    if entry["status"] != "frozen":
        raise ValueError("frozen manifest status is invalid")
    required = {"status", "app", "version", "channel", "url", "format", "staged", "installer"}
    allowed_release = required | {"runtimeVersion", "digest", "revision"}
    if not required <= set(entry) or set(entry) - allowed_release:
        raise ValueError("frozen manifest release fields are invalid")
    if (not isinstance(entry["version"], str) or not VERSION.fullmatch(entry["version"])
            or not isinstance(entry["channel"], str) or not isinstance(entry["format"], str)
            or not isinstance(entry["installer"], str) or not isinstance(entry["staged"], bool)):
        raise ValueError("frozen manifest version is invalid")
    if not isinstance(entry["url"], str) or not re.fullmatch(r"https://[^/\s]+(?:/[^\s]*)?", entry["url"]):
        raise ValueError("frozen manifest source is invalid")
    expected = _expected_entry(app, platform, architecture)
    if (entry["channel"] != expected[0] or entry["format"] != expected[1]
            or entry["installer"] != expected[2] or entry["staged"] != expected[3]
            or not _expected_url(entry, platform, architecture)):
        raise ValueError("frozen manifest policy drifted")
    if entry["format"] not in {"dmg", "zip", "tar-gz", "deb", "msix", "windows-setup", "source"}:
        raise ValueError("frozen manifest format is invalid")
    if entry["installer"] not in {"checker", "external"} or not isinstance(entry["staged"], bool):
        raise ValueError("frozen manifest installer is invalid")
    digest_value = entry.get("digest")
    revision = entry.get("revision")
    if entry["format"] == "source":
        if entry["installer"] != "external" or entry["staged"] or digest_value is not None:
            raise ValueError("source manifest identity is invalid")
        if not isinstance(revision, str) or not COMMIT.fullmatch(revision):
            raise ValueError("source revision is invalid")
    else:
        if (revision is not None or not isinstance(digest_value, str)
                or not SHA256.fullmatch(digest_value.removeprefix("sha256:"))):
            raise ValueError("artifact digest or revision is invalid")
        if not digest_value.startswith("sha256:"):
            raise ValueError("artifact digest is invalid")
    runtime = entry.get("runtimeVersion")
    if runtime is not None and (not isinstance(runtime, str) or not RUNTIME_VERSION.fullmatch(runtime)):
        raise ValueError("runtime version is invalid")


def _blocked_entry(app, platform, architecture):
    blocked = {
        ("chatgpt-desktop", "macos", "x86_64"): ("upstream-unsupported", "https://learn.chatgpt.com/docs/app"),
        ("chatgpt-desktop", "windows", "aarch64"): ("unqualified-platform", "https://learn.chatgpt.com/docs/windows/windows-app"),
        ("claude-desktop", "windows", "aarch64"): ("unqualified-platform", "https://claude.com/download"),
        ("pen-desktop", "windows", "aarch64"): ("upstream-unsupported", "https://www.pen.dev/downloads"),
    }
    if (app, platform, architecture) in blocked:
        return blocked[(app, platform, architecture)]
    if app in ("hermes-desktop", "zed-desktop"):
        repository = "NousResearch/hermes-agent" if app == "hermes-desktop" else "zed-industries/zed"
        return "resolution-failed", f"https://github.com/{repository}/releases/latest"
    return "resolution-failed", _expected_entry(app, platform, architecture)[0].split(":", 1)[-1]


def _pen_suffix(platform, architecture):
    arch = "x64" if architecture == "x86_64" else "arm64"
    return {"linux": f"linux-{arch}.tar.gz", "macos": f"mac-{arch}.dmg", "windows": "win-x64.exe"}[platform]


def _expected_url(entry, platform, architecture):
    app, version, url = entry["app"], entry["version"], entry["url"]
    if app == "hermes-desktop":
        return url == "https://github.com/NousResearch/hermes-agent.git"
    if app == "zed-desktop":
        asset = {"linux": f"zed-linux-{architecture}.tar.gz", "macos": f"Zed-{architecture}.dmg",
                 "windows": f"Zed-{architecture}.exe"}[platform]
        return url == f"https://github.com/zed-industries/zed/releases/download/v{version}/{asset}"
    if app == "pen-desktop":
        suffix = _pen_suffix(platform, architecture)
        return url == f"https://www.pen.dev/download/Pen-{suffix}"
    if app == "chatgpt-desktop":
        if platform == "linux":
            deb_arch = "amd64" if architecture == "x86_64" else "arm64"
            return url == f"https://persistent.oaistatic.com/codex-app-prod/linux/deb/pool/main/c/chatgpt/chatgpt_{version}_{deb_arch}.deb"
        if platform == "windows":
            return url == "https://persistent.oaistatic.com/codex-app-prod/ChatGPT-x64.msix"
        return url == f"https://persistent.oaistatic.com/codex-app-prod/ChatGPT-darwin-arm64-{version}.zip"
    if app == "claude-desktop":
        if platform == "linux":
            deb_arch = "amd64" if architecture == "x86_64" else "arm64"
            return url == f"https://downloads.claude.ai/claude-desktop/apt/stable/pool/main/c/claude-desktop/claude-desktop_{version}_{deb_arch}.deb"
        if platform == "windows":
            return url == "https://claude.ai/api/desktop/win32/x64/msix"
        return bool(re.fullmatch(rf"https://downloads\.claude\.ai/releases/darwin/universal/{re.escape(version)}/Claude-[0-9a-f]{{40}}\.zip", url))
    return False


def _expected_entry(app, platform, architecture):
    """The Python mirror of frozen.rs policy's non-secret identity fields."""
    if app == "hermes-desktop":
        return ("github-source:NousResearch/hermes-agent", "source", "external", False)
    if app == "zed-desktop":
        return ("github-release:zed-industries/zed", "windows-setup" if platform == "windows" else
                ("dmg" if platform == "macos" else "tar-gz"), "external" if platform == "windows" else "checker", False)
    if app == "pen-desktop":
        suffix = _pen_suffix(platform, architecture)
        return (f"official-latest:https://www.pen.dev/download/Pen-{suffix}", "windows-setup" if platform == "windows" else
                ("dmg" if platform == "macos" else "tar-gz"), "external" if platform == "windows" else "checker", True)
    if app == "chatgpt-desktop":
        return (("official-latest:https://persistent.oaistatic.com/codex-app-prod/ChatGPT-x64.msix" if platform == "windows" else
                 ("apt:https://persistent.oaistatic.com/codex-app-prod/linux/deb/" if platform == "linux" else
                  "sparkle:https://persistent.oaistatic.com/codex-app-prod/appcast.xml")),
                "msix" if platform == "windows" else ("deb" if platform == "linux" else "zip"),
                "external" if platform == "windows" else "checker", platform != "linux")
    if app == "claude-desktop":
        return (("official-latest:https://claude.ai/api/desktop/win32/x64/msix" if platform == "windows" else
                 ("apt:https://downloads.claude.ai/claude-desktop/apt/stable/" if platform == "linux" else
                  "squirrel-mac:https://downloads.claude.ai/releases/darwin/universal/RELEASES.json")),
                "msix" if platform == "windows" else ("deb" if platform == "linux" else "zip"),
                "external" if platform == "windows" else "checker", platform != "linux")
    raise ValueError("unknown desktop app")


class StageTimeout(RuntimeError):
    """The checker and its process group did not stop within the cell bound."""


def validate_release_assets(manifest, assets, release_tag, source_commit):
    """Verify local release bytes and return the exact attestation command.

    Attestation itself is performed by the hosted caller with ``gh``; this
    function never executes an artifact or accepts a tag without its commit.
    """
    if not TAG.fullmatch(release_tag) or not COMMIT.fullmatch(source_commit):
        raise ValueError("release identity is invalid")
    entries = {}
    for line in Path(manifest).read_text().splitlines():
        fields = line.split()
        if len(fields) != 2 or not SHA256.fullmatch(fields[0]) or fields[1] in entries:
            raise ValueError("release checksum manifest is invalid")
        entries[fields[1]] = fields[0]
    for name in assets:
        path = Path(assets[name])
        if name not in entries or not path.is_file() or digest(path) != entries[name]:
            raise ValueError("release asset does not match its attested checksum")
    return ["gh", "attestation", "verify", str(manifest), "--source-ref",
            f"refs/tags/{release_tag}", "--source-digest", source_commit]


def validate_identity(source, source_sha, model, apps, platform, release_tag=""):
    """Validate the closed provenance contract before starting an app."""
    if source not in ("branch", "release"):
        raise ValueError("source must be branch or release")
    # Both modes bind the source commit; release assets additionally bind each
    # downloaded byte to the attested SHA-256 manifest.
    pattern = COMMIT
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


def run_stage(command, timeout=3600, live=False, diagnostics=None, source_sha=None, platform_name=None):
    """Run a checker with all output discarded; return only a closed status."""
    try:
        with tempfile.TemporaryDirectory(prefix="nan-desktop-stage-") as temporary:
            directory = Path(temporary) / "private"
            ensure_private_directory(directory)
            if diagnostics is not None:
                if live:
                    raise ValueError("diagnostics require deterministic execution")
                from desktop_diagnostics import run
                return run(command, diagnostics, source_sha, platform_name, timeout, directory)
            private_command(command, directory, timeout=timeout, live=live)
    except CleanupError:
        raise StageTimeout from None
    except (OSError, RuntimeError, subprocess.SubprocessError):
        return False
    return True


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


def validated_report(path, checker):
    """Validate a bounded snapshot, not a path that can change after capture."""
    if path.is_symlink() or not path.is_file():
        raise ValueError("checker report is not a regular file")
    with path.open("rb") as source:
        raw = source.read(65537)
    if len(raw) > 65536:
        raise ValueError("checker report exceeds its bound")
    with tempfile.TemporaryDirectory(prefix="desktop-prerequisite-") as directory:
        snapshot = Path(directory) / "report.json"
        snapshot.write_bytes(raw)
        snapshot.chmod(0o600)
        if not run_stage([str(checker), "validate-report", str(snapshot)], timeout=60):
            raise ValueError("checker report failed trusted validation")
    return json.loads(raw), hashlib.sha256(raw).hexdigest()


def eligible_live_apps(report, cell, state):
    """Only exact, independently completed per-app prerequisites can use a key."""
    identity = report.get("nanHarness") or {}
    if (report.get("schemaVersion") != 3 or report.get("platform") != cell["platform"]
            or report.get("architecture") != cell["architecture"] or report.get("model") != cell["model"]
            or identity.get("sha256") != state["nanhSha256"]
            or (cell["source"] == "release" and identity.get("version") != cell["releaseTag"][1:])
            or [app["app"] for app in report["results"]] != cell["apps"]):
        raise ValueError("deterministic report does not describe this prepared cell")
    if state.get("cleanup") == "blocked" or report["cleanup"] != "passed":
        return []
    return [app["app"] for app in report["results"]
            if app["cleanup"] == "passed" and app.get("appVersion")
            and len(app["deterministic"]) == 3
            and all(probe["status"] == "passed" for probe in app["deterministic"])]


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
    parser.add_argument("--deterministic-report", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--stage", choices=("deterministic", "live"), required=True)
    parser.add_argument("--diagnostics", type=Path)
    args = parser.parse_args()
    try:
        selection = json.loads(args.selection.read_bytes())
        cell = suite_cell(selection, args.platform, args.source, args.source_sha, args.model, args.release_tag)
        if args.diagnostics is not None and (args.stage != "deterministic" or args.source != "branch"):
            raise ValueError("diagnostics require deterministic branch execution")
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
        selected_cell = cell
        if args.stage == "live":
            if args.deterministic_report is None or not state.get("deterministicReportSha256"):
                raise ValueError("live stage requires its captured deterministic report")
            prerequisite, report_digest = validated_report(args.deterministic_report, args.checker)
            if report_digest != state["deterministicReportSha256"]:
                raise ValueError("the deterministic prerequisite changed after execution")
            eligible = eligible_live_apps(prerequisite, cell, state)
            if not eligible:
                state["stages"].append({"name": "live", "status": "skipped"})
                write_json(output, state)
                return 0
            if not os.environ.get("NAN_API_KEY", "").strip():
                raise ValueError("live stage requires an explicitly supplied key")
            selected_cell = {**cell, "apps": eligible}
        command = checker_command(args.checker, args.stage, selected_cell, args.prepared, args.report,
                                  args.nan_harness if cell["source"] == "branch" else None)
        try:
            passed = run_stage(command, live=args.stage == "live", diagnostics=args.diagnostics,
                               source_sha=cell["sourceSha"], platform_name=cell["platform"])
        except StageTimeout:
            state["stages"].append({"name": args.stage, "status": "blocked", "reason": "cleanup"})
            state["cleanup"] = "blocked"
            state["outcome"] = "blocked"
            write_json(output, state)
            return 1
        if args.stage == "deterministic" and args.report.is_file():
            prerequisite, report_digest = validated_report(args.report, args.checker)
            eligible_live_apps(prerequisite, cell, state)
            state["deterministicReportSha256"] = report_digest
        state["stages"].append({"name": args.stage, "status": "passed" if passed else "failed"})
        deterministic = any(stage.get("name") == "deterministic" and stage.get("status") == "passed" for stage in state["stages"])
        live = any(stage.get("name") == "live" and stage.get("status") == "passed" for stage in state["stages"])
        state["outcome"] = "passed" if deterministic and (args.stage == "deterministic" or live) else "failed"
        write_json(output, state)
        return 0 if passed else 1
    except (OSError, ValueError, json.JSONDecodeError, KeyError, TypeError, StageTimeout):
        return 2


if __name__ == "__main__":
    sys.exit(main())
