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
PREFIXES = {b"DESKTOP_DIAGNOSTIC:": "native", b"DESKTOP_INSTALL_DIAGNOSTIC:": "install",
            b"DESKTOP_PREPARE_DIAGNOSTIC:": "prepare", b"DESKTOP_VERSION_DIAGNOSTIC:": "version"}
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
type-text verify-input verify-input-accessibility verify-input-visual verify-response verify-response-guard verify-response-accessibility verify-response-visual send""".split())
CATEGORIES = set("""action-unsupported selector-not-matched permission-required timeout window-changed focus-changed
window-identity-missing window-bounds-changed foreground-changed same-process-window window-off-display
window-occluded native-helper-spawn native-helper-pipe native-helper-timeout native-helper-nonzero-exit
native-helper-window-changed native-helper-query-rejected native-helper-session-unavailable native-helper-output
native-helper-fit-request native-helper-fit-identity-read native-helper-fit-identity-mismatch
native-helper-fit-foreground-read native-helper-fit-foreground-mismatch native-helper-fit-monitor-read
native-helper-fit-workarea-read native-helper-fit-window-read native-helper-fit-workarea-invalid
native-helper-fit-identity-changed native-helper-fit-foreground-changed native-helper-fit-resize
foreground-process-different foreground-window-different foreground-identity-unavailable input-mismatch
ownership-owner-group-lookup-unavailable ownership-candidate-group-lookup-unavailable ownership-different-group
empty-ocr-page missing-composer-anchor marker-without-composer-anchor ambiguous-composer-anchor other""".split())
INSTALL_OPERATIONS = set("""resolve_artifact read_staged_artifact verify_digest verify_staged_artifact
verify_downloaded_artifact fetch_artifact git_init git_remote_add git_fetch git_checkout venv_create pip_install
npm_ci npm_pack verify_revision verify_version verify_desktop_package verify_hermes_launcher check_existing_msix
register_msix check_existing_installation check_platform run_installer verify_installation install identity""".split())
GUARD_CONTEXTS = set("reacquisition before-input before-select-all before-type before-send before-response".split())
FOREGROUND_RELATIONS = set("same-process-different-window different-process identity-unavailable".split())
GEOMETRY_RELATIONS = set("partial-monitor-overlap no-monitor-overlap".split())
SETUP_CAUSES = set("""discovery install configuration runtime current-directory credential-invariant
preflight invalid-plan serialize-plan telemetry-settings update persistence search uninstall
usage-evidence other""".split())


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
                                         "build_prerequisite", "wheel_build", "network", "other"})
    for name in pip_fields - {"pip_failure_hint"}:
        if name in value:
            integer(value[name], 0, 99)


def validate_prepare(value):
    fields(value, {"schemaVersion", "app", "stage", "errorCategory", "reason"},
           {"osError", "transportCategory", "operation", "httpStatus"})
    integer(value["schemaVersion"], 1, 1)
    enum(value["app"], APPS)
    enum(value["stage"], {"discovery", "root-enumeration", "candidate-metadata", "candidate-canonicalization",
                          "candidate-read", "architecture", "version-resource", "frozen-resolution", "installation"})
    enum(value["errorCategory"], {"unsupported", "ambiguous", "incomplete", "unreadable", "version-unknown",
                                  "resolution-failed", "installation-failed", "upstream-unsupported", "unqualified-platform",
                                  "version-mismatch", "resolution-transport", "metadata-parse", "artifact-selection",
                                  "artifact-version", "artifact-staging", "cleanup-uncertain"})
    enum(value["reason"], REASONS)
    if "osError" in value:
        integer(value["osError"], -(2**31), 2**31 - 1)
    if value.keys() & {"transportCategory", "operation", "httpStatus"}:
        require({"transportCategory", "operation"} <= value.keys())
        require(value["stage"] == "frozen-resolution" and value["errorCategory"] == "resolution-transport")
        require("osError" not in value)
        enum(value["operation"], {"metadata", "artifact"})
        enum(value["transportCategory"], {"invalid-url", "policy", "client-setup", "timeout", "connect",
                                           "http-status", "request", "body-read", "body-bound", "local-io"})
        require(("httpStatus" in value) == (value["transportCategory"] == "http-status"))
        if "httpStatus" in value:
            integer(value["httpStatus"], 100, 599)


def validate_version(value):
    fields(value, {"schemaVersion", "app", "source", "failure"},
           {"exitCode", "osError", "runtimeReadAccess"})
    integer(value["schemaVersion"], 1, 1)
    enum(value["app"], APPS)
    enum(value["source"], {"package-metadata", "asar-metadata", "app-version-command",
                           "runtime-metadata", "runtime-version-command",
                           "windows-package-enumeration"})
    enum(value["failure"], {"spawn", "wait", "timeout", "nonzero-exit", "pipe", "read",
                            "oversize", "encoding", "metadata-unreadable", "invalid-metadata"})
    if "exitCode" in value:
        require(value["failure"] == "nonzero-exit")
        integer(value["exitCode"], -(2**31), 2**31 - 1)
    if "osError" in value:
        enum(value["failure"], {"spawn", "wait", "read"})
        integer(value["osError"], -(2**31), 2**31 - 1)
    if "runtimeReadAccess" in value:
        require(value["app"] == "chatgpt-desktop"
                and value["source"] == "runtime-version-command"
                and value["failure"] == "spawn"
                and value.get("osError") == 5)
        enum(value["runtimeReadAccess"], {"readable", "access-denied", "other-error"})


def validate_native(value, platform=None):
    fields(value, {"schemaVersion", "app", "probeIndex", "mode", "launchStage", "composer", "truncated"},
           {"launchExit", "launchFailure", "setupCause", "discoveryCause", "startup", "guiAcquisition", "cleanup", "resultReason", "workerResultFailure", "nativeProcessObservation", "claudeIdentityObservation"})
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
    if "setupCause" in value:
        enum(value["setupCause"], SETUP_CAUSES)
        require(value.get("launchFailure") == "launch-setup-failed")
    if "discoveryCause" in value:
        require(value.get("launchFailure") == "launch-setup-failed")
        require(value.get("setupCause") == "discovery")
        enum(value["discoveryCause"], {"missing-executable", "invalid-executable", "invalid-manifest",
                                       "missing-compatibility-entry", "invalid-version-command",
                                       "version-command", "version-command-failed", "version-probe-timeout",
                                       "version-probe-output-limit", "unsupported-version", "unparseable-version"})
    if "launchExit" in value:
        exit_value = value["launchExit"]
        if exit_value != "unknown":
            require(type(exit_value) is dict and len(exit_value) == 1)
            name = next(iter(exit_value))
            enum(name, {"code", "signal"})
            integer(exit_value[name], -(2**31) if name == "code" else 1, 2**31 - 1 if name == "code" else 127)
    if "resultReason" in value:
        enum(value["resultReason"], REASONS)
    if "startup" in value:
        require(value["app"] == "chatgpt-desktop")
        enum(value.get("launchFailure"), {"native-app-exited", "native-already-running"})
        item = value["startup"]
        fields(item, {"hint"}, {"exit", "sandbox"})
        enum(item["hint"], {"no-usable-sandbox", "missing-shared-library", "display-unavailable",
                            "unknown", "output-unavailable"})
        if "exit" in item:
            exit_value = item["exit"]
            require(type(exit_value) is dict and len(exit_value) == 1)
            name = next(iter(exit_value))
            enum(name, {"code", "signal"})
            integer(exit_value[name], -(2**31) if name == "code" else 1, 2**31 - 1 if name == "code" else 127)
        if "sandbox" in item:
            facts = item["sandbox"]
            fields(facts, {"helperPresence", "helperMode", "helperOwner", "helperLocation", "apparmorUsernsRestriction"})
            enum(facts["helperPresence"], {"present", "missing", "unreadable"})
            enum(facts["helperMode"], {"setuid-executable", "executable-without-setuid", "not-executable", "unknown"})
            enum(facts["helperOwner"], {"root", "non-root", "unknown"})
            enum(facts["helperLocation"], {"sibling-present-or-unreadable", "sibling-absent"})
            enum(facts["apparmorUsernsRestriction"], {"restricted", "unrestricted", "unavailable"})
    if "workerResultFailure" in value:
        enum(value["workerResultFailure"], {"timeout", "wait", "cancelled", "missing", "unreadable-or-oversized", "schema", "exit-mismatch"})
    if "guiAcquisition" in value:
        item = value["guiAcquisition"]
        fields(item, {"stage", "errorCategory", "reason"}, {"foregroundRelation"})
        enum(item["stage"], {"process-live", "native-helper", "window-candidates", "window-ownership", "window-stability",
                             "window-inventory-empty", "window-candidates-empty", "window-candidates-too-small",
                             "window-owner-name-mismatch"})
        enum(item["errorCategory"], CATEGORIES)
        enum(item["reason"], REASONS)
        if "foregroundRelation" in item:
            enum(item["foregroundRelation"], FOREGROUND_RELATIONS)
            require(platform == "windows")
            require(item["stage"] == "window-stability")
            category_relations = {
                "native-helper-fit-foreground-read": {"identity-unavailable"},
                "native-helper-fit-foreground-mismatch": FOREGROUND_RELATIONS,
                "native-helper-fit-foreground-changed": FOREGROUND_RELATIONS,
            }
            require(item["errorCategory"] in category_relations)
            require(item["foregroundRelation"] in category_relations[item["errorCategory"]])
    if "nativeProcessObservation" in value:
        require(value["app"] == "claude-desktop")
        item = value["nativeProcessObservation"]
        fields(item, {"state", "everObservedPresent"})
        enum(item["state"], {"matching-process-present", "matching-process-absent", "query-failed"})
        require(type(item["everObservedPresent"]) is bool)
        require(item["everObservedPresent"] or item["state"] != "matching-process-present")
    if "claudeIdentityObservation" in value:
        require(value["app"] == "claude-desktop")
        enum(value["claudeIdentityObservation"], {"no-matching-bundle-process", "matching-process-no-visible-window",
                                                    "window-name-mismatch", "window-not-eligible", "window-eligible",
                                                    "ambiguous-identity", "query-unavailable", "overflow"})
    if "cleanup" in value:
        item = value["cleanup"]
        fields(item, {"stage", "originalReason", "reason"}, {"absence", "stop"})
        enum(item["stage"], {"stop", "absence-after-stop", "restore", "absence-after-restore"})
        enum(item["reason"], REASONS)
        if item["originalReason"] is not None:
            enum(item["originalReason"], REASONS)
        if "absence" in item:
            enum(item["absence"], {"accessibility-provider", "accessibility-enumeration", "native-windows"})
        if "stop" in item:
            require(item["stage"] == "stop")
            validate_stop(item["stop"])
    require(type(value["composer"]) is list and len(value["composer"]) <= 64)
    for item in value["composer"]:
        fields(item, {"operation", "errorCategory"}, {"guardContext", "geometryRelation"})
        enum(item["operation"], OPERATIONS)
        enum(item["errorCategory"], CATEGORIES)
        if "guardContext" in item:
            enum(item["guardContext"], GUARD_CONTEXTS)
            require(item["operation"] in {"guard", "verify-response-guard"})
        if "geometryRelation" in item:
            enum(item["geometryRelation"], GEOMETRY_RELATIONS)
            require(item["operation"] == "guard" and item.get("guardContext") == "reacquisition"
                    and item["errorCategory"] == "window-off-display")


def validate_stop(value):
    fields(value, {"initialWait", "graceWait", "kill", "finalWait"})
    for name, step in value.items():
        fields(step, {"outcome"}, {"osError"})
        enum(step["outcome"], {"not-attempted", "timed-out", "failed", "issued" if name == "kill" else "reaped"})
        if "osError" in step:
            require(step["outcome"] == "failed")
            integer(step["osError"], -(2**31), 2**31 - 1)


def validate_record(event, platform=None):
    fields(event, {"kind", "record"})
    validators = {"native": validate_native, "install": validate_install, "prepare": validate_prepare,
                  "version": validate_version}
    enum(event["kind"], validators)
    if event["kind"] == "native":
        validate_native(event["record"], platform)
    else:
        validators[event["kind"]](event["record"])
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
        validate_record(event, platform)
        if event["kind"] == "native" and "nativeProcessObservation" in event["record"]:
            require(platform == "macos")
            observation = event["record"]["nativeProcessObservation"]
            require(observation["everObservedPresent"]
                    or observation["state"] != "matching-process-present")
        if event["kind"] == "native" and "claudeIdentityObservation" in event["record"]:
            require(platform == "macos")
        if event["kind"] == "native":
            startup = event["record"].get("startup", {})
            require(platform == "linux" or "sandbox" not in startup)
    return value


class Capture:
    def __init__(self, platform=None):
        self.events = []
        self.invalid = 0
        self.platform = platform

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
                            validate_record(event, self.platform)
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
    capture = Capture(platform)
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
