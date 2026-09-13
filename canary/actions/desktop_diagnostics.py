#!/usr/bin/env python3
"""Retain only closed diagnostic facts from the existing private executor."""

import argparse
import json
from pathlib import Path
import re
import sys

from cell import CleanupError, StageTimeout, private_command, write_json

MAX_STREAM = 4 * 1024 * 1024
MAX_EVENT = 4096
MAX_EVENTS = 64
MAX_BUNDLE = 512 * 1024
PREFIXES = {b"DESKTOP_DIAGNOSTIC:": "native", b"DESKTOP_INSTALL_DIAGNOSTIC:": "install"}
APPS = set("chatgpt-desktop claude-desktop hermes-desktop pen-desktop zed-desktop".split())
PLATFORMS = {"linux", "macos", "windows"}
REASONS = set("""missing-key invalid-key missing-model installation-unavailable installation-failed
installation-ambiguous installation-unreadable version-unknown unsupported-version
harness-capability-unavailable unsupported-architecture already-running permission-required login-required
timeout isolation-unavailable focus-changed window-changed window-occluded desktop-unavailable
application-exited selector-not-matched action-unsupported input-mismatch response-mismatch tool-mismatch
provider-failed budget-exceeded cancelled cleanup-conflict cleanup-failed not-run""".split())
OPERATIONS = set("""locate-accessible accessible-login-check accessible-named-count accessible-editable-count
accessible-editable-visible locate-visual visual-click guard set-value focus wait-focused input-sim select-all
type-text verify-input verify-response verify-response-guard verify-response-accessibility verify-response-visual send""".split())
CATEGORIES = set("""action-unsupported selector-not-matched permission-required timeout window-changed focus-changed
window-identity-missing window-bounds-changed foreground-changed same-process-window window-off-display
window-occluded native-helper-spawn native-helper-pipe native-helper-timeout native-helper-nonzero-exit
native-helper-window-changed native-helper-query-rejected native-helper-session-unavailable native-helper-output
foreground-process-different foreground-window-different foreground-identity-unavailable other""".split())
INSTALL_OPERATIONS = set("""resolve_artifact read_staged_artifact verify_digest verify_staged_artifact
verify_downloaded_artifact fetch_artifact git_init git_remote_add git_fetch git_checkout venv_create pip_install
npm_ci npm_pack verify_revision verify_version verify_desktop_package verify_hermes_launcher check_existing_msix
register_msix check_existing_installation check_platform run_installer verify_installation install identity""".split())


def require(condition):
    if not condition:
        raise ValueError("invalid closed diagnostic")


def enum(value, allowed):
    require(type(value) is str and value in allowed)


def fields(value, required, optional=()):
    require(type(value) is dict and set(required) <= value.keys() <= set(required) | set(optional))


def integer(value, minimum, maximum):
    require(type(value) is int and minimum <= value <= maximum)


def unique(pairs):
    value = {}
    for key, item in pairs:
        require(key not in value)
        value[key] = item
    return value


def decode(raw):
    try:
        return json.loads(raw, object_pairs_hook=unique)
    except (ValueError, UnicodeError, RecursionError):
        raise ValueError("invalid closed diagnostic") from None


def validate_install(value):
    pip_fields = {"pip_failure_hint", "python_major", "python_minor", "pip_major", "pip_minor"}
    spawn_fields = {"os_error", "win_error", "npm_resolution"}
    fields(value, {"schema_version", "app", "stage", "operation", "failure"}, {"return_code"} | pip_fields | spawn_fields)
    integer(value["schema_version"], 1, 1)
    enum(value["app"], APPS)
    enum(value["stage"], {"artifact", "download", "hermes_build", "hermes_verify", "installer", "installer_identity"})
    enum(value["operation"], INSTALL_OPERATIONS)
    enum(value["failure"], {"timeout", "spawn", "nonzero_exit", "missing_artifact", "identity_failure", "cleanup_uncertain"})
    if "return_code" in value:
        integer(value["return_code"], -(2**31), 2**32 - 1)
    if value.keys() & spawn_fields:
        require(value["failure"] == "spawn")
    for name in spawn_fields - {"npm_resolution"}:
        if name in value:
            integer(value[name], -(2**31), 2**32 - 1)
    if "npm_resolution" in value:
        enum(value["operation"], {"npm_ci", "npm_pack"})
        enum(value["npm_resolution"], {"missing", "cmd", "exe", "other"})
    if value.keys() & pip_fields:
        require(value["app"] == "hermes-desktop" and value["stage"] == "hermes_build"
                and value["operation"] == "pip_install")
    if "pip_failure_hint" in value:
        enum(value["pip_failure_hint"], {"interpreter_compatibility", "dependency_resolution",
                                         "build_prerequisite", "network", "other"})
    for name in pip_fields - {"pip_failure_hint"}:
        if name in value:
            integer(value[name], 0, 99)


def validate_native(value):
    fields(value, {"schemaVersion", "app", "probeIndex", "mode", "launchStage", "composer", "truncated"},
           {"launchExit", "launchFailure", "guiAcquisition", "cleanup", "resultReason", "workerResultFailure"})
    integer(value["schemaVersion"], 1, 1)
    enum(value["app"], APPS)
    enum(value["mode"], {"deterministic", "live"})
    if value["mode"] == "deterministic":
        integer(value["probeIndex"], 0, 2)
    else:
        require(value["probeIndex"] is None)
    enum(value["launchStage"], {"not-started", "started", "exited-before-window", "window-unavailable", "window-acquired"})
    require(type(value["truncated"]) is bool)
    if "launchFailure" in value:
        enum(value["launchFailure"], set("""argument-validation-failed launch-setup-failed provider-routing-failed
             launcher-spawn-failed native-app-spawn-failed child-cli-failed native-argument native-capability-probe
             native-capability-missing native-compatibility native-version-probe native-version-unparseable
             native-process-inspection native-installation native-already-running native-profile native-model-catalog
             native-bridge-handshake native-app-exited credential-unavailable""".split()))
    if "launchExit" in value:
        exit_value = value["launchExit"]
        if exit_value != "unknown":
            require(type(exit_value) is dict and len(exit_value) == 1)
            name = next(iter(exit_value))
            enum(name, {"code", "signal"})
            integer(exit_value[name], -(2**31) if name == "code" else 1, 2**31 - 1 if name == "code" else 127)
    if "resultReason" in value:
        enum(value["resultReason"], REASONS)
    if "workerResultFailure" in value:
        enum(value["workerResultFailure"], {"timeout", "wait", "cancelled", "missing", "unreadable-or-oversized", "schema", "exit-mismatch"})
    if "guiAcquisition" in value:
        item = value["guiAcquisition"]
        fields(item, {"stage", "errorCategory", "reason"})
        enum(item["stage"], {"process-live", "native-helper", "window-candidates", "window-ownership", "window-stability",
                             "window-inventory-empty", "window-candidates-empty", "window-candidates-too-small"})
        enum(item["errorCategory"], CATEGORIES)
        enum(item["reason"], REASONS)
    if "cleanup" in value:
        item = value["cleanup"]
        fields(item, {"stage", "originalReason", "reason"}, {"absence"})
        enum(item["stage"], {"stop", "absence-after-stop", "restore", "absence-after-restore"})
        enum(item["reason"], REASONS)
        if item["originalReason"] is not None:
            enum(item["originalReason"], REASONS)
        if "absence" in item:
            enum(item["absence"], {"accessibility-provider", "accessibility-enumeration", "native-windows"})
    require(type(value["composer"]) is list and len(value["composer"]) <= 64)
    for item in value["composer"]:
        fields(item, {"operation", "errorCategory"})
        enum(item["operation"], OPERATIONS)
        enum(item["errorCategory"], CATEGORIES)


def validate_record(event):
    fields(event, {"kind", "record"})
    enum(event["kind"], {"native", "install"})
    (validate_native if event["kind"] == "native" else validate_install)(event["record"])
    require(len(json.dumps(event["record"], separators=(",", ":")).encode()) <= MAX_EVENT)


def identity(source_sha, platform):
    require(type(source_sha) is str and re.fullmatch(r"[0-9a-f]{40}", source_sha) is not None)
    enum(platform, PLATFORMS)


def validate_bundle(path, source_sha, platform):
    identity(source_sha, platform)
    require(not path.is_symlink() and path.is_file())
    with path.open("rb") as source:
        raw = source.read(MAX_BUNDLE + 1)
    require(len(raw) <= MAX_BUNDLE)
    value = decode(raw)
    fields(value, {"schemaVersion", "sourceSha", "platform", "events", "invalidEvents"})
    integer(value["schemaVersion"], 1, 1)
    require(value["sourceSha"] == source_sha and value["platform"] == platform)
    integer(value["invalidEvents"], 0, MAX_STREAM + 1)
    require(type(value["events"]) is list and len(value["events"]) <= MAX_EVENTS)
    for event in value["events"]:
        validate_record(event)
    return value


class Capture:
    def __init__(self):
        self.events = []
        self.invalid = 0

    def observe(self, log):
        # This callback must never mask the executor's cleanup failure.
        try:
            log.flush()
            log.seek(0)
            raw = log.read(MAX_STREAM + 1)
            if len(raw) > MAX_STREAM:
                self.invalid += 1
                raw = raw[:MAX_STREAM]
            for line in raw.splitlines():
                for prefix, kind in PREFIXES.items():
                    if line.startswith(prefix):
                        try:
                            payload = line[len(prefix):].strip()
                            require(len(payload) <= MAX_EVENT)
                            event = {"kind": kind, "record": decode(payload)}
                            validate_record(event)
                            require(len(self.events) < MAX_EVENTS)
                            self.events.append(event)
                        except ValueError:
                            self.invalid += 1
                        break
        except (OSError, ValueError):
            self.invalid += 1


def run(command, output, source_sha, platform, timeout=3600, directory=None):
    identity(source_sha, platform)
    require(0 < timeout <= 5400 and bool(command))
    require(not output.exists() and not output.is_symlink())
    capture = Capture()
    def save():
        write_json(output, {"schemaVersion": 1, "sourceSha": source_sha, "platform": platform,
                            "events": capture.events, "invalidEvents": capture.invalid})
    try:
        status = private_command(command, directory or Path.cwd(), timeout=timeout, allow_failure=True,
                                 diagnostic_callback=capture.observe)
    except BaseException:
        try:
            save()
        except (OSError, RuntimeError):
            pass
        raise
    save()
    return status == 0 and capture.invalid == 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--validate", type=Path)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--platform", required=True)
    parser.add_argument("--timeout", type=int, default=3600)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    try:
        if args.validate:
            require(args.output is None and not args.command)
            validate_bundle(args.validate, args.source_sha, args.platform)
            return 0
        require(args.output is not None and args.command[:1] == ["--"])
        return 0 if run(args.command[1:], args.output, args.source_sha, args.platform, args.timeout) else 1
    except (OSError, ValueError, RuntimeError):
        print("Desktop diagnostics could not complete; no private output was disclosed.", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
