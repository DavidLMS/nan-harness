#!/usr/bin/env python3
"""Run one hosted CLI cell; only closed, validated evidence leaves the runner.

Stages are separate processes so the installation/conformance steps never receive
the provider secret. All child output is private, even on a timeout or failure.
"""

import argparse
from contextlib import contextmanager
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time

if os.name == "nt":
    import ctypes
    from ctypes import wintypes

    class IOCounters(ctypes.Structure):
        _fields_ = [(name, ctypes.c_ulonglong) for name in (
            "ReadOperationCount", "WriteOperationCount", "OtherOperationCount",
            "ReadTransferCount", "WriteTransferCount", "OtherTransferCount")]

    class BasicLimitInformation(ctypes.Structure):
        _fields_ = [("per_process_user_time", ctypes.c_longlong),
                    ("per_job_user_time", ctypes.c_longlong), ("limit_flags", wintypes.DWORD),
                    ("min_working_set", ctypes.c_size_t), ("max_working_set", ctypes.c_size_t),
                    ("active_process_limit", wintypes.DWORD), ("affinity", ctypes.c_size_t),
                    ("priority_class", wintypes.DWORD), ("scheduling_class", wintypes.DWORD)]

    class ExtendedLimitInformation(ctypes.Structure):
        _fields_ = [("basic", BasicLimitInformation), ("io", IOCounters),
                    ("process_memory", ctypes.c_size_t), ("job_memory", ctypes.c_size_t),
                    ("peak_process_memory", ctypes.c_size_t), ("peak_job_memory", ctypes.c_size_t)]

    class Trustee(ctypes.Structure):
        _fields_ = [("p_multiple", wintypes.LPVOID), ("multiple_count", wintypes.DWORD),
                    ("form", wintypes.DWORD), ("trustee_type", wintypes.DWORD),
                    ("name", wintypes.LPWSTR)]

    class ExplicitAccess(ctypes.Structure):
        _fields_ = [("permissions", wintypes.DWORD), ("access_mode", wintypes.DWORD),
                    ("inheritance", wintypes.DWORD), ("trustee", Trustee)]

    class JobAccounting(ctypes.Structure):
        _fields_ = [("user_time", ctypes.c_longlong), ("kernel_time", ctypes.c_longlong),
                    ("period_user_time", ctypes.c_longlong), ("period_kernel_time", ctypes.c_longlong),
                    ("page_faults", wintypes.DWORD), ("total_processes", wintypes.DWORD),
                    ("active_processes", wintypes.DWORD), ("terminated_processes", wintypes.DWORD)]

sys.path.insert(0, str(Path(__file__).resolve().parent))
from selection import CLI_HARNESSES, PLATFORMS as HOSTED_PLATFORMS, resolve_model

HARNESSES = CLI_HARNESSES
SEMVER = re.compile(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?\Z")
SHA256 = re.compile(r"[0-9a-f]{40}\Z")


def source_identity(source_sha):
    """Use the canary report validator's immutable commit identity form."""
    if not SHA256.fullmatch(source_sha):
        raise ValueError("source SHA must be a 40-character lowercase commit SHA")
    return f"commit:{source_sha}"
ROOT = Path(__file__).resolve().parents[2]
# Kept for the historical coverage helper; new workflow selection comes from
# selection.select_suite and includes the native Windows platform.
PLATFORMS = (("linux", "ubuntu-24.04-arm", "unknown-linux-musl", "aarch64"),
             ("macos", "macos-15", "apple-darwin", "aarch64"))
SMOKE_PLATFORMS = (("linux", "ubuntu-24.04", "unknown-linux-musl", "x86_64"),
                   ("macos", "macos-15", "apple-darwin", "aarch64"))
SMOKE_LIMIT = 4


class CleanupError(RuntimeError):
    """The next harness must not start when process cleanup is unproven."""


class StageTimeout(RuntimeError):
    """A stage limit says nothing about compatibility; it remains retryable."""


class CompatibilityMismatch(RuntimeError):
    """A closed, positively evidenced contract failure; the only failed-compatibility source.

    ``code`` is a closed identifier built from a fixed probe-stage name, never from
    child output.
    """

    def __init__(self, code):
        super().__init__("hosted check demonstrated a compatibility mismatch")
        self.code = code


class ProbeCleanupError(RuntimeError):
    """The live probe could not remove its private workspace; nothing is certified."""


class ProbeFailure(RuntimeError):
    """The live probe closed a safe stage marker but did not pass."""

    def __init__(self, stage, status, diagnostic=None):
        super().__init__("hosted live probe failed at a closed stage")
        self.stage = stage
        self.status = status
        self.diagnostic = diagnostic


INSTALLER_FAILURE_PHASE = "install-package"
DOCTOR_FAILURE_PHASE = "doctor-command"
DOCTOR_VERSION_FAILURE_PHASE = "doctor-version-mismatch"
INSTALL_FAILURE_CODES = {
    "npm-network", "npm-package-not-found", "npm-permission",
    "npm-engine-mismatch", "npm-script-failure", "npm-openclaw-preinstall",
    "npm-openclaw-postinstall", "npm-openclaw-preinstall-signal",
    "npm-openclaw-postinstall-signal", "npm-dependency-script-failure",
    "npm-openclaw-preinstall-runtime", "npm-openclaw-preinstall-runtime-signal",
    "npm-openclaw-preinstall-legacy-guard", "npm-openclaw-preinstall-legacy-guard-signal",
    "npm-openclaw-preinstall-module", "npm-openclaw-preinstall-module-signal",
    "npm-openclaw-preinstall-permission", "npm-openclaw-preinstall-permission-signal",
    "npm-dependency-script-exit",
    "npm-dependency-script-signal", "exit-nonzero", "signal-terminated",
    "hosted-node-missing", "hosted-node-version-mismatch", "hosted-npm-missing",
    "diagnostic-unknown", "unknown",
}
WINDOWS_INSTALL_CATEGORIES = frozenset({
    "network-dns", "network-timeout", "network-connection", "tls-certificate", "permission",
    "disk-space", "tool-missing", "package-not-found", "installer-refused",
})
HERMES_INSTALL_STAGES = frozenset({
    "uv", "git", "node", "system-packages", "repository", "python", "venv", "dependencies",
    "node-deps", "path", "config-templates", "platform-sdks", "bootstrap-marker", "setup", "gateway",
})
INSTALL_FAILURE_CODES.update("windows-installer-hermes-" + stage for stage in HERMES_INSTALL_STAGES)
INSTALL_FAILURE_CODES.update("windows-installer-hermes-" + stage + "-" + category
                             for stage in HERMES_INSTALL_STAGES for category in WINDOWS_INSTALL_CATEGORIES)
INSTALL_FAILURE_CODES.update("windows-installer-" + code for code in WINDOWS_INSTALL_CATEGORIES)
WINDOWS_INSTALL_DETAILS = frozenset({
    "launcher-missing", "expected-executable-missing", "invalid-ref", "invalid-version",
    "empty-download", "metadata-request-failed", "invalid-archive",
    "official-asset-missing", "official-metadata-probe-failed", "invalid-frozen-ref",
    "marker-missing", "marker-invalid", "marker-passed", "download-failed", "native-exit",
})
INSTALL_FAILURE_CODES.update("windows-installer-" + code for code in WINDOWS_INSTALL_DETAILS)
PRIVATE_DIAGNOSTIC_LIMIT = 64 * 1024
NPM_ERROR_LINE = re.compile(r"^\s*npm\s+(?:err!|error)\s?(.*)$", re.IGNORECASE)
# Reviewed against npm metadata for pinned openclaw@2026.9.2: its 65 direct
# dependencies plus optional sqlite-vec. These names validate one npm path
# record; they are not copied into public diagnostics.
OPENCLAW_DEPENDENCIES = frozenset({
    "@agentclientprotocol/sdk", "@anthropic-ai/sdk", "@clack/core",
    "@clack/prompts", "@earendil-works/pi-tui", "@google/genai",
    "@grammyjs/runner", "@grammyjs/transformer-throttler", "@homebridge/ciao",
    "@lydell/node-pty", "@mistralai/mistralai", "@modelcontextprotocol/sdk",
    "@mozilla/readability", "@openclaw/ai", "@openclaw/fs-safe",
    "@openclaw/proxyline", "@silvia-odwyer/photon-node", "@trycua/cua-driver",
    "acorn", "chalk", "chokidar", "clawpdf", "commander", "croner", "diff",
    "dotenv", "entities", "execa", "express", "file-type", "grammy",
    "highlight.js", "hosted-git-info", "iconv-lite", "ignore", "jiti", "json5",
    "jszip", "koffi", "kysely", "linkedom", "minimatch", "ms", "node-edge-tts",
    "openai", "p-limit", "p-map", "partial-json", "playwright-core", "pretty-ms",
    "qrcode", "quickjs-wasi", "rastermill", "semver", "sqlite-vec", "tar",
    "tree-sitter-bash", "tslog", "typebox", "typescript", "undici", "web-push",
    "web-tree-sitter", "ws", "yaml", "zod",
})

DEPENDENCY_EXECUTABLES = frozenset({"bash", "node", "npm", "sh"})
DEPENDENCY_TERMINATIONS = frozenset({"exit", "signal"})


def _dependency_token(package):
    token = package[1:].replace("/", "-") if package.startswith("@") else package
    normalized = re.sub(r"[^A-Za-z0-9-]", "-", token)
    if normalized != token or len(token) > 24:
        token = normalized
        token = token[:17] + "-" + hashlib.sha256(package.encode()).hexdigest()[:6]
    return token


def dependency_failure_code(package, executable, termination):
    """Return a closed telemetry-safe code for one reviewed npm record."""
    package = package.lower()
    executable = executable.lower()
    if (package not in OPENCLAW_DEPENDENCIES or executable not in DEPENDENCY_EXECUTABLES
            or termination not in DEPENDENCY_TERMINATIONS):
        return None
    scope, token = ("S", _dependency_token(package)) if package.startswith("@") else ("U", _dependency_token(package))
    code = f"NH-CLI-DEP-{scope}-{token.upper()}-{executable.upper()}-{termination.upper()}"
    if len(code) > 51 or not re.fullmatch(r"NH-[A-Z0-9-]+", code):
        return None
    return code


DEPENDENCY_FAILURE_CODES = frozenset(code for code in (
    dependency_failure_code(package, executable, termination)
    for package in OPENCLAW_DEPENDENCIES
    for executable in DEPENDENCY_EXECUTABLES
    for termination in DEPENDENCY_TERMINATIONS
) if code is not None)
INSTALL_FAILURE_CODES.update(DEPENDENCY_FAILURE_CODES)

NPM_RECORD_PACKAGE = re.compile(
    r"^path\s+[^\n]*node_modules[\\/]"
    r"(?P<package>@[^/\\\s]+[\\/][^/\\\s]+|[^/\\\s]+)(?=[/\\\s]|$)", re.IGNORECASE)
NPM_RECORD_CODE = re.compile(r"^code\s+([a-z][a-z0-9_]*|[0-9]+)\b", re.IGNORECASE)
NPM_RECORD_COMMAND = re.compile(
    r"^command\s+(?P<executable>sh|bash|node|npm)\s+-c\s+(?P<script>.+)$",
    re.IGNORECASE)
NPM_RECORD_ACTION = re.compile(r"^(?:command failed|lifecycle script)\b", re.IGNORECASE)
NPM_RECORD_LIFECYCLE = re.compile(
    r"^command\s+(?:sh|bash)\s+-c\s+node\s+scripts/"
    r"(?P<script>preinstall-package-manager-warning|postinstall-bundled-plugins)\.mjs\b",
    re.IGNORECASE)

# Fixed markers emitted by OpenClaw's pinned preinstall script. Interpolated
# versions, paths, URLs, and exception text are intentionally not retained.
OPENCLAW_PREINSTALL_DIAGNOSTICS = (
    ("npm-openclaw-preinstall-runtime", ("[openclaw] error: this OpenClaw release requires Node ",
                                          "[openclaw] detected Node missing")),
    ("npm-openclaw-preinstall-legacy-guard", ("could not remove the legacy package install guard",)),
    ("npm-openclaw-preinstall-module", ("ERR_MODULE_NOT_FOUND", "Cannot find module")),
    ("npm-openclaw-preinstall-permission", ("EACCES", "EPERM")),
)
HOSTED_INSTALL_DIAGNOSTICS = (
    ("hosted-node-missing", "hosted Node runtime is missing"),
    ("hosted-node-version-mismatch", "hosted Node runtime version mismatch"),
    ("hosted-npm-missing", "hosted npm runtime could not be found"),
)


def classify_install_failure(log, status, expected_package=None):
    """Classify bounded private installer evidence without retaining its text."""
    if status is None:
        return "diagnostic-unknown"
    try:
        log.seek(0)
        evidence = log.read(PRIVATE_DIAGNOSTIC_LIMIT + 1).decode("utf-8", "replace")
    except (OSError, UnicodeError):
        return "diagnostic-unknown"
    evidence = evidence[:PRIVATE_DIAGNOSTIC_LIMIT]
    direct_categories = [code for code, marker in HOSTED_INSTALL_DIAGNOSTICS
                         if any(line.strip() == marker for line in evidence.splitlines())]
    if len(direct_categories) == 1:
        return direct_categories[0]
    if direct_categories:
        return "diagnostic-unknown"
    records = []
    current = []
    for line in evidence.splitlines():
        match = NPM_ERROR_LINE.match(line)
        if match:
            current.append(match.group(1).strip())
        elif current:
            records.append(current)
            current = []
    if current:
        records.append(current)
    if not records:
        return "diagnostic-unknown"
    categories = {
        "npm-package-not-found": {"E404", "ENOTARGET"},
        "npm-network": {"EAI_AGAIN", "ENOTFOUND", "ETIMEDOUT", "ENETUNREACH"},
        "npm-permission": {"EACCES", "EPERM"},
        "npm-engine-mismatch": {"EBADENGINE"},
    }
    record_categories = []
    for record in records:
        codes = {match.group(1).upper() for line in record
                 for match in [NPM_RECORD_CODE.match(line)] if match}
        known_category = None
        ambiguous_code = False
        for category, markers in categories.items():
            if codes & markers:
                if known_category is not None and known_category != category:
                    ambiguous_code = True
                    break
                known_category = category
        if ambiguous_code:
            record_categories.append("diagnostic-unknown")
            continue
        if known_category is not None:
            record_categories.append(known_category)
            continue
        if codes and any(not code.isdigit() for code in codes):
            record_categories.append("diagnostic-unknown")
            continue
        action = any(NPM_RECORD_ACTION.match(line) for line in record)
        package_matches = [NPM_RECORD_PACKAGE.match(line) for line in record
                           if NPM_RECORD_PACKAGE.match(line)]
        command_matches = [NPM_RECORD_COMMAND.match(line) for line in record
                           if NPM_RECORD_COMMAND.match(line)]
        lifecycle_match = next((NPM_RECORD_LIFECYCLE.match(line) for line in record
                                if NPM_RECORD_LIFECYCLE.match(line)), None)
        if not action and not lifecycle_match:
            continue
        if len(package_matches) != 1 or len(command_matches) != 1:
            record_categories.append("diagnostic-unknown")
            continue
        package = package_matches[0].group("package").lower()
        if expected_package == "openclaw" and package == "openclaw" and lifecycle_match:
            script = lifecycle_match.group(1).lower()
            if script == "preinstall-package-manager-warning":
                record_text = "\n".join(record)
                # The guard's explicit failure message may include EACCES or
                # EPERM; retain the more specific cleanup condition.
                if "could not remove the legacy package install guard" in record_text:
                    matches = ["npm-openclaw-preinstall-legacy-guard"]
                else:
                    matches = [code for code, markers in OPENCLAW_PREINSTALL_DIAGNOSTICS
                               if any(marker in record_text for marker in markers)]
                record_categories.append(matches[0] if len(matches) == 1 else "npm-openclaw-preinstall")
            else:
                record_categories.append("npm-openclaw-postinstall")
        elif package in OPENCLAW_DEPENDENCIES:
            record_categories.append(dependency_failure_code(
                package, command_matches[0].group("executable"),
                "signal" if status < 0 else "exit") or "diagnostic-unknown")
        else:
            record_categories.append("diagnostic-unknown")
    if len(record_categories) != 1 or record_categories[0] == "diagnostic-unknown":
        return "diagnostic-unknown"
    result = record_categories[0]
    if status < 0:
        if result.startswith("NH-CLI-DEP-"):
            return result
        if result == "npm-openclaw-preinstall":
            return "npm-openclaw-preinstall-signal"
        if result == "npm-openclaw-postinstall":
            return "npm-openclaw-postinstall-signal"
        if result.startswith("npm-openclaw-preinstall-"):
            return result + "-signal"
        if result == "npm-dependency-script-failure":
            return "npm-dependency-script-signal"
        return "signal-terminated"
    if result == "npm-dependency-script-failure":
        return "npm-dependency-script-exit"
    return result


class InstallFailure(RuntimeError):
    """A bounded installation substage failed without exposing child output."""

    def __init__(self, phase, code="unknown"):
        if phase not in {INSTALLER_FAILURE_PHASE, DOCTOR_FAILURE_PHASE,
                         DOCTOR_VERSION_FAILURE_PHASE}:
            raise ValueError("unknown installation failure phase")
        if code not in INSTALL_FAILURE_CODES:
            raise ValueError("unknown installation failure code")
        super().__init__("hosted installation substage failed")
        self.phase = phase
        self.code = code


CONFORMANCE_SCENARIOS = ("inventory", "tool-round-trip", "sentinel", "external-prerequisite")
CONFORMANCE_ATTEMPTS = 2
# Probe stages whose failure is deterministic after every provider check passed:
# nan-harness exited successfully, the tool and completion markers were present,
# no bridge diagnostic appeared and nan-harness wrote "observed" usage evidence,
# which it does only for a successful run with usage-bearing responses. Such a
# run always renders the usage summary to stderr, so its absence is a closed
# nan-harness output contract failure rather than a provider result.
LIVE_MISMATCH_STAGES = frozenset({"usage-summary"})
PROBE_STAGES = frozenset({"setup", "harness-run", "tool-evidence", "read-marker", "completion-marker",
                          "bridge-sentinel", "usage-evidence", "usage-summary", "cleanup", "complete"})
PROBE_DIAGNOSTICS = frozenset({
    "aider-completion-marker-stdout-empty-stderr-empty",
    "aider-completion-marker-stdout-empty-stderr-nonempty",
    "aider-completion-marker-stdout-nonempty-stderr-empty",
    "aider-completion-marker-stdout-nonempty-stderr-nonempty",
})
PROBE_DIAGNOSTIC_CODES = {
    diagnostic: f"live-{diagnostic}-exit-1" for diagnostic in PROBE_DIAGNOSTICS
}
# The PowerShell probe publishes its own closed marker (schema 2) with the stage the
# native run reached and a bounded diagnostic list.
WINDOWS_PROBE_STAGES = frozenset({"live-tool", "harness-run", "read-marker", "completion-marker",
                                  "bridge-sentinel", "usage-evidence", "usage-summary", "complete"})
WINDOWS_LIVE_DIAGNOSTICS = frozenset({
    "live-error-auth", "live-error-network", "live-error-arguments", "live-error-permission",
    "live-error-provider", "live-error-config",
    "live-child-launch", "live-exit-nonzero", "live-exit-missing", "live-credential-missing",
    "live-tool-evidence-missing", "live-read-marker-missing", "live-completion-marker-missing",
    "live-bridge-sentinel", "live-usage-invalid", "live-usage-summary-missing",
    "probe-unexpected-failure",
})


def detected_identity():
    """The hosted platform and architecture this process runs on, in canonical names."""
    if os.name == "nt":
        system = "windows"
        machine = os.environ.get("PROCESSOR_ARCHITECTURE", "")
    else:
        system = {"linux": "linux", "darwin": "macos"}.get(sys.platform)
        machine = getattr(os, "uname")().machine
    architecture = {"arm64": "aarch64", "aarch64": "aarch64",
                    "amd64": "x86_64", "x86_64": "x86_64"}.get(machine.strip().lower())
    if system is None or architecture is None:
        raise RuntimeError("this gate requires a supported hosted runner")
    return system, architecture


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
    protect_private(path.parent)
    with tempfile.NamedTemporaryFile(mode="w", dir=path.parent, delete=False) as out:
        protect_private(Path(out.name))
        json.dump(value, out, sort_keys=True)
        out.write("\n")
        out.flush()
        os.fsync(out.fileno())
    os.replace(out.name, path)
    protect_private(path)


def protect_private(path):
    """Apply the repository's owner/SYSTEM-only ACL contract on Windows."""
    if os.name != "nt":
        return
    if path.is_symlink() or getattr(path, "is_junction", lambda: False)():
        raise RuntimeError("Windows private paths cannot be links or junctions")
    sid = windows_current_user_sid()
    advapi32 = ctypes.windll.advapi32
    kernel32 = ctypes.windll.kernel32
    advapi32.ConvertStringSidToSidW.argtypes = [wintypes.LPWSTR, ctypes.POINTER(wintypes.LPVOID)]
    advapi32.ConvertStringSidToSidW.restype = wintypes.BOOL
    advapi32.SetEntriesInAclW.argtypes = [wintypes.DWORD, ctypes.POINTER(ExplicitAccess), wintypes.LPVOID,
                                          ctypes.POINTER(wintypes.LPVOID)]
    advapi32.SetEntriesInAclW.restype = wintypes.DWORD
    advapi32.SetNamedSecurityInfoW.argtypes = [wintypes.LPWSTR, wintypes.DWORD, wintypes.DWORD,
                                               wintypes.LPVOID, wintypes.LPVOID, wintypes.LPVOID, wintypes.LPVOID]
    advapi32.SetNamedSecurityInfoW.restype = wintypes.DWORD
    kernel32.LocalFree.argtypes = [wintypes.HLOCAL]
    kernel32.LocalFree.restype = wintypes.HLOCAL
    sid_ptrs = []
    acl = wintypes.LPVOID()
    try:
        for text in (sid, "S-1-5-18"):
            pointer = wintypes.LPVOID()
            if not advapi32.ConvertStringSidToSidW(text, ctypes.byref(pointer)):
                raise RuntimeError("Windows private-path SID conversion failed")
            sid_ptrs.append(pointer)
        inheritance = 0x3 if path.is_dir() else 0
        entries = (ExplicitAccess * 2)()
        for entry, pointer in zip(entries, sid_ptrs):
            entry.permissions = 0x001F01FF  # FILE_ALL_ACCESS: concrete file rights, not GENERIC_ALL
            entry.access_mode = 2  # SET_ACCESS
            entry.inheritance = inheritance
            entry.trustee.form = 0  # TRUSTEE_IS_SID
            entry.trustee.trustee_type = 1  # TRUSTEE_IS_USER / well-known SID accepted
            entry.trustee.name = ctypes.cast(pointer, wintypes.LPWSTR)
        error = advapi32.SetEntriesInAclW(2, entries, None, ctypes.byref(acl))
        if error:
            raise RuntimeError("Windows private-path ACL construction failed")
        error = advapi32.SetNamedSecurityInfoW(str(path), 1, 0x00000004 | 0x80000000,
                                               None, None, acl, None)
        if error:
            raise RuntimeError("Windows private-path protection failed")
    finally:
        if acl:
            kernel32.LocalFree(acl)
        for pointer in sid_ptrs:
            kernel32.LocalFree(pointer)


def windows_current_user_sid():
    """Read the SID from the current process token, never from an env name."""
    kernel32 = ctypes.windll.kernel32
    advapi32 = ctypes.windll.advapi32
    kernel32.GetCurrentProcess.argtypes = []
    kernel32.GetCurrentProcess.restype = wintypes.HANDLE
    kernel32.CloseHandle.argtypes = [wintypes.HANDLE]
    kernel32.CloseHandle.restype = wintypes.BOOL
    kernel32.LocalFree.argtypes = [wintypes.HLOCAL]
    kernel32.LocalFree.restype = wintypes.HLOCAL
    advapi32.OpenProcessToken.argtypes = [wintypes.HANDLE, wintypes.DWORD, ctypes.POINTER(wintypes.HANDLE)]
    advapi32.OpenProcessToken.restype = wintypes.BOOL
    advapi32.GetTokenInformation.argtypes = [wintypes.HANDLE, wintypes.DWORD, wintypes.LPVOID,
                                             wintypes.DWORD, ctypes.POINTER(wintypes.DWORD)]
    advapi32.GetTokenInformation.restype = wintypes.BOOL
    advapi32.ConvertSidToStringSidW.argtypes = [wintypes.LPVOID, ctypes.POINTER(wintypes.LPWSTR)]
    advapi32.ConvertSidToStringSidW.restype = wintypes.BOOL
    token = wintypes.HANDLE()
    process = kernel32.GetCurrentProcess()
    if not advapi32.OpenProcessToken(process, 8, ctypes.byref(token)):
        raise RuntimeError("Windows private-path token is unavailable")
    try:
        size = wintypes.DWORD()
        advapi32.GetTokenInformation(token, 1, None, 0, ctypes.byref(size))
        buffer = ctypes.create_string_buffer(size.value)
        if not advapi32.GetTokenInformation(token, 1, buffer, size, ctypes.byref(size)):
            raise RuntimeError("Windows private-path token could not be read")
        sid_ptr = ctypes.cast(buffer, ctypes.POINTER(ctypes.c_void_p))[0]
        text = ctypes.c_wchar_p()
        if not advapi32.ConvertSidToStringSidW(sid_ptr, ctypes.byref(text)):
            raise RuntimeError("Windows private-path SID could not be resolved")
        try:
            return text.value
        finally:
            kernel32.LocalFree(text)
    finally:
        kernel32.CloseHandle(token)


def ensure_private_directory(path, reusable=False):
    """Reject links and accidental reuse before any stage payload is written."""
    if path.is_symlink():
        raise RuntimeError("private cell directory cannot be a symlink")
    if path.exists() and not reusable:
        raise RuntimeError("private cell directory already exists")
    path.mkdir(mode=0o700, parents=True, exist_ok=reusable)
    protect_private(path)


@contextmanager
def private_log(directory):
    with tempfile.NamedTemporaryFile(dir=directory, delete=False) as log:
        path = Path(log.name)
        try:
            protect_private(path)
            yield log
        finally:
            log.close()
            remove_private_log(path)


def remove_private_log(path):
    deadline = time.monotonic() + 2
    while True:
        try:
            path.unlink(missing_ok=True)
            return
        except OSError as error:
            # Windows can briefly retain a terminating child's inherited file
            # handle. Keep the payload private and require actual removal.
            if getattr(error, "winerror", None) != 32 or time.monotonic() >= deadline:
                raise CleanupError("private stage log could not be removed") from error
            time.sleep(0.02)


def runner_toolcache_bin(node_path, version):
    """Accept only the runner's standard setup-node toolcache bin."""
    if node_path.name != "node" or not version:
        return None
    parts = node_path.parent.parts
    if (len(parts) < 5 or parts[-5] != "hostedtoolcache" or parts[-4] != "node"
            or parts[-3] != version or parts[-1] != "bin"
            or parts[-2] not in {"arm64", "aarch64"}):
        return None
    return node_path.parent


def cell_environment(directory):
    """Keep each harness's installation, caches and configuration in its cell."""
    env = os.environ.copy()
    home = directory / "home"
    locations = {
        "HOME": home, "USERPROFILE": home,
        "XDG_CONFIG_HOME": home / ".config", "XDG_DATA_HOME": home / ".local/share",
        "XDG_CACHE_HOME": home / ".cache", "XDG_STATE_HOME": home / ".local/state",
        "APPDATA": home / "AppData/Roaming", "LOCALAPPDATA": home / "AppData/Local",
        "NPM_CONFIG_PREFIX": home / ".local", "NPM_CONFIG_CACHE": home / ".cache/npm",
        "UV_TOOL_DIR": home / ".local/share/uv/tools", "UV_TOOL_BIN_DIR": home / ".local/bin",
        "UV_CACHE_DIR": home / ".cache/uv", "HERMES_HOME": home / ".hermes",
        "KIMI_INSTALL_DIR": home / ".kimi-code", "KIMI_HOME": home / ".kimi",
        "NAN_HARNESS_CONFIG_DIR": home / ".config/nan-harness",
        "TMPDIR": directory / "tmp", "TEMP": directory / "tmp", "TMP": directory / "tmp",
    }
    if os.name == "nt":
        # The native installer stages Hermes outside HOME, unlike its Unix recipe.
        locations["HERMES_HOME"] = directory / "hermes"
    for location in set(locations.values()):
        ensure_private_directory(location, reusable=True)
    # Keep runner-provisioned runtimes, but do not discover a harness left in the
    # shared user profile or in a sibling cell by an earlier cell or installer.
    caller_node = Path(shutil.which("node") or "").resolve()
    caller_node_version = subprocess.run(
        ["node", "-p", "process.versions.node"], check=True,
        capture_output=True, text=True, timeout=10).stdout.strip()
    # setup-node installs its selected runtime below the runner HOME on macOS.
    # Keep only that standard toolcache bin after HOME filtering; arbitrary
    # user-managed tools under the same HOME remain excluded.
    trusted_runtime_bin = runner_toolcache_bin(caller_node, caller_node_version)
    hidden = [Path(env[key]).resolve() for key in ("HOME", "USERPROFILE") if env.get(key)]
    hidden.append(directory.resolve().parent)
    inherited = [entry for entry in env.get("PATH", "").split(os.pathsep)
                 if entry and not any(Path(entry).resolve().is_relative_to(old) for old in hidden)]
    bins = [home / ".local/bin", home / ".local", home / ".kimi-code/bin",
            home / ".hermes/bin", home / ".local/share/nan-harness-canary-uv/bin"]
    if os.name == "nt":
        bins = [directory / "bin", directory / "hermes/bin",
                home / ".nan-harness-canary-venv/Scripts",
                home / "AppData/Roaming/npm"] + bins
    env.update({key: str(value) for key, value in locations.items()})
    retained = [str(path) for path in bins]
    if trusted_runtime_bin is not None and str(trusted_runtime_bin) not in inherited:
        retained.append(str(trusted_runtime_bin))
    env["PATH"] = os.pathsep.join(retained + inherited)
    if os.name == "nt":
        git = shutil.which("git.exe", path=env["PATH"])
        bash = Path(git).parent.parent / "usr/bin/bash.exe" if git else None
        if bash is not None and bash.is_file():
            for name in ("NAN_HARNESS_GIT_BASH", "KIMI_SHELL_PATH", "KIMI_CLI_GIT_BASH_PATH"):
                env[name] = str(bash)
    # Hosted installers must retain the runner-selected Node/npm ahead of the
    # legacy Tart/Homebrew prefixes; the guest script uses this only as an
    # explicit hosted-mode contract. Tart's historical one-argument callers
    # do not set it and retain their existing installer semantics.
    env["NAN_CANARY_HOSTED"] = "1"
    env["NAN_CANARY_EXPECTED_NODE_VERSION"] = caller_node_version
    return env


def private_command(command, directory, timeout=900, output=None, live=False, allow_failure=False,
                    environment=None, diagnostic_callback=None):
    env = dict(os.environ if environment is None else environment)
    if not live:
        env.pop("NAN_API_KEY", None)
    env["NAN_CANARY_REDACT_FAILURE_OUTPUT"] = "1"
    env["CI"] = "1"
    with private_log(directory) as log:
        if output:
            output.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
            destination = output.open("wb")
            protect_private(output)
        else:
            destination = log
        try:
            creationflags = (getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0) | 0x00000004) if os.name == "nt" else 0
            child = subprocess.Popen(command, stdout=destination, stderr=log,
                                     env=env, cwd=directory, start_new_session=os.name != "nt",
                                     creationflags=creationflags)
            job = None
            try:
                # Keep resume inside this try: setup failures must close the job
                # before any child can run outside its kill-on-close boundary.
                job = WindowsJob(child.pid) if os.name == "nt" else None
                if job:
                    job.resume(child.pid)
                status = child.wait(timeout=timeout)
            except (subprocess.TimeoutExpired, KeyboardInterrupt):
                raise StageTimeout("stage exceeded its execution limit") from None
            finally:
                try:
                    finish_stage(child, job)
                finally:
                    if diagnostic_callback is not None:
                        diagnostic_callback(log, status)
            if status and not allow_failure:
                raise RuntimeError("stage did not pass")
            return status
        finally:
            if output:
                destination.close()


def finish_stage(child, job):
    """A successful foreground exit does not imply its descendants exited."""
    try:
        if job:
            job.close()
        else:
            try:
                terminate_process_tree(child.pid)
            except ProcessLookupError:
                pass
    except (OSError, RuntimeError, subprocess.SubprocessError) as error:
        raise CleanupError("stage process cleanup could not be verified") from error
    finally:
        try:
            child.wait(timeout=10)
        except (subprocess.TimeoutExpired, OSError) as error:
            raise CleanupError("stage process did not exit after cleanup") from error


def terminate_process_tree(pid):
    """Terminate a stage and descendants on both native POSIX and Windows hosts."""
    if os.name == "nt":
        subprocess.run(["taskkill", "/PID", str(pid), "/T", "/F"],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                       check=False, timeout=10)
    else:
        os.killpg(pid, signal.SIGKILL)


class WindowsJob:
    """Typed kill-on-close Job Object attached before a Windows stage resumes."""

    def __init__(self, pid):
        self.pid = pid
        kernel32 = ctypes.windll.kernel32
        kernel32.CreateJobObjectW.argtypes = [wintypes.LPVOID, wintypes.LPCWSTR]
        kernel32.CreateJobObjectW.restype = wintypes.HANDLE
        kernel32.SetInformationJobObject.argtypes = [wintypes.HANDLE, wintypes.DWORD,
                                                      wintypes.LPVOID, wintypes.DWORD]
        kernel32.SetInformationJobObject.restype = wintypes.BOOL
        kernel32.AssignProcessToJobObject.argtypes = [wintypes.HANDLE, wintypes.HANDLE]
        kernel32.AssignProcessToJobObject.restype = wintypes.BOOL
        kernel32.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
        kernel32.OpenProcess.restype = wintypes.HANDLE
        kernel32.CloseHandle.argtypes = [wintypes.HANDLE]
        kernel32.CloseHandle.restype = wintypes.BOOL
        kernel32.TerminateJobObject.argtypes = [wintypes.HANDLE, wintypes.UINT]
        kernel32.TerminateJobObject.restype = wintypes.BOOL
        kernel32.GetLastError.argtypes = []
        kernel32.GetLastError.restype = wintypes.DWORD
        self.handle = kernel32.CreateJobObjectW(None, None)
        if not self.handle:
            raise RuntimeError("Windows process supervision could not start")
        limits = ExtendedLimitInformation()
        limits.basic.limit_flags = 0x00002000  # JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        if not kernel32.SetInformationJobObject(self.handle, 9, ctypes.byref(limits), ctypes.sizeof(limits)):
            self.close()
            raise RuntimeError("Windows process cleanup policy could not be installed")
        self._assign(pid)

    def _assign(self, pid):
        process = ctypes.windll.kernel32.OpenProcess(0x1F0FFF, False, pid)
        if not process or not ctypes.windll.kernel32.AssignProcessToJobObject(self.handle, process):
            if process:
                ctypes.windll.kernel32.CloseHandle(process)
            self.close()
            raise RuntimeError("Windows process supervision could not attach")
        ctypes.windll.kernel32.CloseHandle(process)

    def resume(self, pid):
        if pid != self.pid or not self.handle:
            raise RuntimeError("suspended process does not belong to this job")
        kernel32 = ctypes.windll.kernel32
        kernel32.CreateToolhelp32Snapshot.argtypes = [wintypes.DWORD, wintypes.DWORD]
        kernel32.CreateToolhelp32Snapshot.restype = wintypes.HANDLE
        kernel32.Thread32First.argtypes = [wintypes.HANDLE, wintypes.LPVOID]
        kernel32.Thread32First.restype = wintypes.BOOL
        kernel32.Thread32Next.argtypes = [wintypes.HANDLE, wintypes.LPVOID]
        kernel32.Thread32Next.restype = wintypes.BOOL
        kernel32.OpenThread.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
        kernel32.OpenThread.restype = wintypes.HANDLE
        kernel32.ResumeThread.argtypes = [wintypes.HANDLE]
        kernel32.ResumeThread.restype = wintypes.DWORD
        class ThreadEntry(ctypes.Structure):
            _fields_ = [("size", wintypes.DWORD), ("usage", wintypes.DWORD),
                        ("thread_id", wintypes.DWORD), ("owner_pid", wintypes.DWORD),
                        ("base_priority", wintypes.LONG), ("delta_priority", wintypes.LONG),
                        ("flags", wintypes.DWORD)]
        snapshot = kernel32.CreateToolhelp32Snapshot(4, 0)
        if not snapshot or snapshot == wintypes.HANDLE(-1).value:
            self.close()
            raise RuntimeError("Windows thread snapshot could not be created")
        resumed = False
        try:
            entry = ThreadEntry(ctypes.sizeof(ThreadEntry))
            found = kernel32.Thread32First(snapshot, ctypes.byref(entry))
            while found:
                if entry.owner_pid == pid:
                    thread = kernel32.OpenThread(0x0002, False, entry.thread_id)
                    if not thread:
                        raise RuntimeError("Windows suspended thread could not be opened")
                    try:
                        if kernel32.ResumeThread(thread) == 0xFFFFFFFF:
                            raise RuntimeError("Windows suspended thread could not be resumed")
                    finally:
                        kernel32.CloseHandle(thread)
                    resumed = True
                    break
                found = kernel32.Thread32Next(snapshot, ctypes.byref(entry))
        finally:
            kernel32.CloseHandle(snapshot)
        if not resumed:
            self.close()
            raise RuntimeError("Windows suspended stage thread was not found")

    def close(self):
        if self.handle:
            kernel32 = ctypes.windll.kernel32
            try:
                if not kernel32.TerminateJobObject(self.handle, 1):
                    raise CleanupError("Windows job termination failed")
                self.wait_empty()
            finally:
                closed = kernel32.CloseHandle(self.handle)
                self.handle = None
                if not closed:
                    raise CleanupError("Windows job handle could not be closed")

    def wait_empty(self):
        query = ctypes.windll.kernel32.QueryInformationJobObject
        query.argtypes = [wintypes.HANDLE, wintypes.DWORD, wintypes.LPVOID,
                          wintypes.DWORD, ctypes.POINTER(wintypes.DWORD)]
        query.restype = wintypes.BOOL
        deadline = time.monotonic() + 10
        while True:
            accounting = JobAccounting()
            if not query(self.handle, 1, ctypes.byref(accounting), ctypes.sizeof(accounting), None):
                raise CleanupError("Windows job cleanup could not be inspected")
            if accounting.active_processes == 0:
                return
            if time.monotonic() >= deadline:
                raise CleanupError("Windows job descendants did not exit")
            time.sleep(0.02)


def installer_command(harness, version, ref=""):
    """Exact-version installer argv; a ref is the frozen immutable source commit."""
    if os.name == "nt":
        command = ["pwsh", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File",
                   str(ROOT / "canary/guest/install-harness.ps1"), "-Harness", harness, "-Version", version]
        return command + (["-Ref", ref] if ref else [])
    return ["bash", str(ROOT / "canary/guest/install-harness.sh"), harness, version] + ([ref] if ref else [])


def windows_install_failure(marker, fallback):
    """Project one closed installer category; discard all private marker content."""
    try:
        if marker.stat().st_size > PRIVATE_DIAGNOSTIC_LIMIT:
            return "windows-installer-marker-invalid"
        value = json.loads(marker.read_bytes())
        if not isinstance(value, dict) or value.get("schemaVersion") != 2 or value.get("status") not in ("failed", "passed"):
            return "windows-installer-marker-invalid"
        if value["status"] == "passed":
            return "windows-installer-marker-passed"
        diagnostic = value.get("diagnostic")
        code = diagnostic.get("processCategory") if isinstance(diagnostic, dict) else None
        stage = diagnostic.get("upstreamStage") if isinstance(diagnostic, dict) else None
        if isinstance(stage, str) and stage in HERMES_INSTALL_STAGES:
            if code == "installer-refused":
                return "windows-installer-hermes-" + stage
            if isinstance(code, str) and code in WINDOWS_INSTALL_CATEGORIES:
                return "windows-installer-hermes-" + stage + "-" + code
        if isinstance(code, str) and code in WINDOWS_INSTALL_CATEGORIES:
            return "windows-installer-" + code
        if isinstance(diagnostic, dict):
            asset = diagnostic.get("assetReason")
            if isinstance(asset, str) and asset in WINDOWS_INSTALL_DETAILS:
                return "windows-installer-" + asset
            if diagnostic.get("subphase") == "download":
                return "windows-installer-download-failed"
            if diagnostic.get("processReason") == "exit-nonzero":
                return "windows-installer-native-exit"
        reason = value.get("reason")
        if isinstance(reason, str) and reason in WINDOWS_INSTALL_DETAILS:
            return "windows-installer-" + reason
        return fallback
    except FileNotFoundError:
        return "windows-installer-marker-missing"
    except (OSError, ValueError):
        return "windows-installer-marker-invalid"
    finally:
        marker.unlink(missing_ok=True)


def install(args, state):
    command = installer_command(args.harness, args.harness_version, getattr(args, "harness_ref", "") or "")
    environment = cell_environment(args.directory)
    installer_marker = args.directory / "installer-result.json"
    if os.name == "nt":
        installer_marker.unlink(missing_ok=True)
    installer_code = "unknown"

    def capture_installer(log, status):
        nonlocal installer_code
        installer_code = classify_install_failure(log, status,
                                                  "openclaw" if args.harness == "openclaw" else None)

    try:
        status = private_command(command, args.directory, allow_failure=True,
                                 environment=environment, diagnostic_callback=capture_installer)
    except CleanupError:
        raise
    except (OSError, RuntimeError, subprocess.SubprocessError) as error:
        raise InstallFailure(INSTALLER_FAILURE_PHASE, installer_code) from error
    if status:
        if os.name == "nt":
            installer_code = windows_install_failure(installer_marker, installer_code)
        raise InstallFailure(INSTALLER_FAILURE_PHASE, installer_code)
    doctor = args.directory / "doctor.json"
    try:
        status = private_command([str(args.binary), "doctor", args.harness, "--allow-unsupported",
                                  "--allow-untested", "--json"], args.directory, output=doctor,
                                 allow_failure=True, environment=environment)
    except CleanupError:
        raise
    except (OSError, RuntimeError, subprocess.SubprocessError) as error:
        raise InstallFailure(DOCTOR_FAILURE_PHASE) from error
    if status:
        raise InstallFailure(DOCTOR_FAILURE_PHASE, "exit-nonzero")
    try:
        version = json.loads(doctor.read_bytes())["version"]
        if not isinstance(version, str) or not SEMVER.fullmatch(version) or version != args.harness_version:
            raise ValueError()
        state["harness"]["version"] = version
    except (KeyError, ValueError, TypeError):
        raise InstallFailure(DOCTOR_VERSION_FAILURE_PHASE) from None
    finally:
        doctor.unlink(missing_ok=True)


def conformance_result(value, harness):
    """Validate the closed conformance report; return its failed contract scenarios.

    Accepted statuses match evaluate-conformance.sh, plus ``failed`` for the
    contract scenarios. Anything else is an unproven runner result.
    """
    if (not isinstance(value, dict) or value.get("schemaVersion") not in (1, 2)
            or value.get("harness") != harness or value.get("outcome") not in ("passed", "failed")
            or not isinstance(value.get("scenarios"), list)):
        raise ValueError("conformance report is not a closed contract result")
    scenarios = value["scenarios"]
    names = [scenario.get("name") if isinstance(scenario, dict) else None for scenario in scenarios]
    if sorted(names, key=str) != sorted(CONFORMANCE_SCENARIOS):
        raise ValueError("conformance report scenario set is not closed")
    allowed = {"inventory": ("passed", "failed"), "tool-round-trip": ("passed", "failed"),
               "sentinel": ("passed", "failed"), "external-prerequisite": ("passed", "skipped", "failed")}
    failed = {}
    for scenario in scenarios:
        duration = scenario.get("durationMilliseconds")
        if (scenario.get("status") not in allowed[scenario["name"]]
                or not isinstance(scenario.get("checks"), list) or not scenario["checks"]
                or type(duration) is not int or duration < 0):
            raise ValueError("conformance scenario is not a closed contract result")
        # A failed inventory remains the historical drift observation, not a mismatch.
        if scenario["status"] == "failed" and scenario["name"] != "inventory":
            failed[scenario["name"]] = duration
    has_failed_scenario = any(scenario["status"] == "failed" for scenario in scenarios)
    if (value["outcome"] == "failed") != has_failed_scenario:
        raise ValueError("conformance outcome does not match its scenario statuses")
    return failed


def conformance(args, state):
    """Deterministic conformance only certifies success; its failures stay blocked.

    The published report has one status per scenario, shared by workspace setup,
    scripted-provider startup, wrapper timeouts and contract assertions. Neither
    duration nor repetition separates those, so a failure is never a mismatch.
    A second attempt can still produce positive evidence after a transient error.
    """
    report = args.directory / "conformance-private.json"
    environment = cell_environment(args.directory)
    for attempt in range(1, CONFORMANCE_ATTEMPTS + 1):
        try:
            private_command([str(args.canary), "conformance", "--nan-harness", str(args.binary),
                             "--harness", args.harness, "--json"], args.directory, output=report,
                            allow_failure=True, environment=environment)
            result = json.loads(report.read_bytes())
            failed = conformance_result(result, args.harness)
        finally:
            report.unlink(missing_ok=True)
        if not failed:
            if any(scenario["name"] == "inventory" and scenario["status"] == "failed"
                   for scenario in result["scenarios"]):
                identity = f"{args.harness}:{state['harness']['version']}:inventory-drift"
                state["observations"] = [{"kind": "inventory-drift",
                                          "fingerprint": hashlib.sha256(identity.encode()).hexdigest()}]
            return attempt
    raise RuntimeError("conformance did not produce positive contract evidence")


def probe_result(path):
    """Read the probe's closed stage marker; missing or malformed markers are unproven."""
    try:
        value = json.loads(path.read_bytes())
    except (OSError, ValueError):
        return None
    required = {"schemaVersion", "stage", "status"}
    if (not isinstance(value, dict) or set(value) not in (required, required | {"diagnostic"})
            or type(value.get("schemaVersion")) is not int or value["schemaVersion"] != 1
            or not isinstance(value.get("stage"), str) or value["stage"] not in PROBE_STAGES
            or not isinstance(value.get("status"), str) or value["status"] not in ("passed", "failed")
            or (value["status"] == "passed") != (value["stage"] == "complete")
            or ("diagnostic" in value and
                (not isinstance(value["diagnostic"], str) or
                 value["status"] != "failed" or value["stage"] != "completion-marker" or
                 value["diagnostic"] not in PROBE_DIAGNOSTICS))):
        return None
    return value


def windows_probe_result(path):
    """Read the PowerShell probe marker; missing or malformed markers are unproven."""
    try:
        value = json.loads(path.read_bytes())
    except (OSError, ValueError):
        return None
    if (not isinstance(value, dict) or type(value.get("schemaVersion")) is not int
            or value["schemaVersion"] != 2 or value.get("stage") not in WINDOWS_PROBE_STAGES
            or value.get("status") not in ("passed", "failed")
            or (value["status"] == "passed") != (value["stage"] == "complete")):
        return None
    diagnostics = value.get("diagnostics")
    if diagnostics is not None and (not isinstance(diagnostics, list) or len(diagnostics) > 1
                                    or any(not isinstance(item, str) or item not in WINDOWS_LIVE_DIAGNOSTICS
                                           for item in diagnostics)):
        return None
    return value


def live(args, _state):
    if not os.environ.get("NAN_API_KEY"):
        raise RuntimeError("live stage requires an explicitly supplied key")
    environment = cell_environment(args.directory)
    environment["NAN_CANARY_NAN_COMMAND"] = str(args.binary)
    environment["NAN_CANARY_MODEL"] = args.model
    marker = args.directory / "probe-result.json"
    marker.unlink(missing_ok=True)
    # Forward slashes keep the path valid for Git Bash on native Windows.
    environment["NAN_CANARY_PROBE_RESULT"] = marker.as_posix()
    windows = os.name == "nt"
    probe = ROOT / ("canary/guest/probe-harness.ps1" if windows else "canary/guest/probe-harness.sh")
    if windows:
        # Match the native batch runner: PowerShell 7 preserves embedded quotes
        # when passing the tool prompt to the native nan-harness executable.
        command = ["pwsh", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass",
                   "-File", str(probe), "-Harness", args.harness, "-Stage", "live-tool",
                   "-Model", args.model, "-NanBinary", str(args.binary), "-Canary", str(args.canary),
                   "-Version", args.harness_version]
    else:
        command = ["bash", str(probe), args.harness]
    try:
        status = private_command(command, args.directory, timeout=600, live=True,
                                 allow_failure=True, environment=environment)
        result = windows_probe_result(marker) if windows else probe_result(marker)
    finally:
        marker.unlink(missing_ok=True)
    if status == 0 and result is not None and result["status"] == "passed":
        return 1
    if result is not None and result["stage"] == "cleanup":
        raise ProbeCleanupError("live probe workspace cleanup is unproven")
    if status != 0 and result is not None and result["stage"] in LIVE_MISMATCH_STAGES:
        raise CompatibilityMismatch("live:" + result["stage"])
    if result is None:
        raise ProbeFailure("marker-missing", status)
    diagnostic = result.get("diagnostic")
    if diagnostic is None and windows:
        # The PowerShell reader admits only closed diagnostic codes; never forward
        # arbitrary child output from the private capture files.
        codes = result.get("diagnostics") or []
        diagnostic = codes[0] if len(codes) == 1 else None
    if diagnostic is not None and args.harness != "aider" and not (
            windows and diagnostic in WINDOWS_LIVE_DIAGNOSTICS):
        diagnostic = None
    raise ProbeFailure(result["stage"], status, diagnostic)


def initial_state(args):
    detected_platform, detected_architecture = detected_identity()
    platform = getattr(args, "system", None) or detected_platform
    try:
        expected_architecture = HOSTED_PLATFORMS[platform]["architecture"]
    except KeyError:
        raise RuntimeError("this gate requires a supported hosted runner") from None
    architecture = getattr(args, "architecture", None) or detected_architecture
    if (platform != detected_platform or architecture != detected_architecture
            or architecture != expected_architecture):
        raise RuntimeError("requested hosted identity does not match the native runner")
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
        "nanHarness": {"version": getattr(args, "nan_version", None) or args.tag[1:],
                       "source": source_identity(getattr(args, "source_sha", None) or "0" * 40),
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
        write_json(args.directory / "binding.json", {
            "runId": args.run_id, "model": args.model,
            "source": state["nanHarness"]["source"], "operatingSystem": state["environment"]["operatingSystem"],
            "architecture": state["environment"]["architecture"]})
    binding_path = args.directory / "binding.json"
    if binding_path.exists():
        binding = json.loads(binding_path.read_bytes())
        expected_binding = {"runId": args.run_id, "model": args.model,
                            "source": source_identity(getattr(args, 'source_sha', None) or "0" * 40),
                            "operatingSystem": state["environment"]["operatingSystem"],
                            "architecture": state["environment"]["architecture"]}
        if binding != expected_binding:
            raise RuntimeError("hosted run identity changed between stages")
    expected_source = None
    if hasattr(args, "tag"):
        expected_source = source_identity(getattr(args, "source_sha", None)) if getattr(args, "source_sha", None) else None
    if (state["nanHarness"]["sha256"] != digest(args.binary)
            or (expected_source is not None and state["nanHarness"].get("source") is not None
                and state["nanHarness"]["source"] != expected_source)
            or state["harness"]["id"] != args.harness):
        raise RuntimeError("cell identity changed between stages")
    steps = {"install": ("install-and-diagnose", install),
             "conformance": ("deterministic-conformance", conformance),
             "live": ("live-tool", live)}
    if args.stage == "report":
        required = ["install-and-diagnose", "deterministic-conformance"]
        live_required = getattr(args, "mode", None) == "live" or (
            getattr(args, "mode", None) is None and args.trigger in ("release", "weekly"))
        if live_required:
            required.append("live-tool")
        if [check["name"] for check in state["checks"]] != required:
            raise RuntimeError("cell has incomplete or repeated stages")
        state["completedAt"] = timestamp()
        if "live-tool" in required:
            state["model"] = args.model
            if args.trigger in ("daily", "manual"):
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
    attempts = execute(args, state) or 1
    duration = int((time.monotonic() - started) * 1000)
    state["checks"].append({"name": name, "status": "passed",
                            "durationMilliseconds": duration, "attempts": attempts})
    state["durationMilliseconds"] += duration
    write_json(state_path, state)


def failed_report(args, mismatch=None):
    """Write a closed failed report while retaining every earlier passed check.

    Only a typed ``CompatibilityMismatch`` from conformance or live evidence is a
    harness failure. Nonzero processes, timeouts, provider/auth errors and missing
    upstream metadata stay retryable infrastructure blocks.
    """
    state_path = args.directory / "state.json"
    state = json.loads(state_path.read_bytes()) if state_path.exists() else initial_state(args)
    phase, failure_class = {
        "unresolved": ("resolve-official-version", "infrastructure"),
        "install": ("install-and-diagnose", "installation"),
        "conformance": ("deterministic-conformance", "infrastructure"),
        "live": ("live-tool", "infrastructure"),
        "report": ("report-validation", "test-contract"),
    }[args.stage]
    if isinstance(mismatch, ProbeCleanupError):
        # Do not let projection mistake a failed live stage for a provider-only
        # failure: cleanup is a terminal boundary for all prior evidence.
        phase = "cleanup"
    elif isinstance(mismatch, ProbeFailure):
        phase = "live-tool"
    elif isinstance(mismatch, InstallFailure):
        phase = mismatch.phase
    code = None
    summary = "Hosted check did not complete successfully."
    if isinstance(mismatch, InstallFailure):
        code = mismatch.code
    if isinstance(mismatch, ProbeFailure):
        code = (PROBE_DIAGNOSTIC_CODES.get(mismatch.diagnostic)
                if args.harness == "aider" and mismatch.stage == "completion-marker"
                and mismatch.status == 1 else None)
        if mismatch.diagnostic in WINDOWS_LIVE_DIAGNOSTICS and mismatch.status == 1:
            code = mismatch.diagnostic + "-exit-1"
        if code is None:
            code = f"live-{mismatch.stage}-exit-{mismatch.status}"
        summary = "Hosted live probe closed at stage " + mismatch.stage \
            + " with exit status " + str(mismatch.status) + "."
    if isinstance(mismatch, CompatibilityMismatch) and args.stage in ("conformance", "live"):
        failure_class, code = "harness", mismatch.code
        summary = "Hosted check reproduced a typed compatibility mismatch."
    state["completedAt"] = timestamp()
    elapsed = (datetime.datetime.fromisoformat(state["completedAt"].replace("Z", "+00:00"))
               - datetime.datetime.fromisoformat(state["startedAt"].replace("Z", "+00:00")))
    total = max(state["durationMilliseconds"], int(elapsed.total_seconds() * 1000))
    state["checks"].append({"name": phase, "status": "failed",
                            "durationMilliseconds": total - state["durationMilliseconds"],
                            "attempts": CONFORMANCE_ATTEMPTS if code and args.stage == "conformance" else 1})
    state["durationMilliseconds"] = total
    state["outcome"] = "infrastructure-failure" if failure_class == "infrastructure" else "failed"
    if args.stage == "live" or any(check["name"] == "live-tool" for check in state["checks"]):
        # The selected model identifies live evidence even when it failed or was
        # followed by a report-stage failure.
        state["model"] = args.model
        if args.trigger in ("daily", "manual"):
            state["tier"] = "live-core"
    identity = f"{args.harness}:{phase}:{failure_class}" + (f":{code}" if code else "")
    state["failure"] = {"class": failure_class, "phase": phase, "summary": summary,
                        "fingerprint": hashlib.sha256(identity.encode()).hexdigest()}
    if code:
        state["failure"]["code"] = code
    write_json(args.output, state)
    try:
        private_command([str(args.canary), "validate-report", str(args.output)], args.directory)
    except (OSError, RuntimeError):
        args.output.unlink(missing_ok=True)


def main():
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("stage", choices=("unresolved", "install", "conformance", "live", "report"))
    parser.add_argument("--harness", choices=HARNESSES, required=True)
    parser.add_argument("--trigger", choices=("release", "weekly", "daily", "manual"), required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--canary", type=Path, required=True)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--model", default="")
    parser.add_argument("--mode", choices=("deterministic", "live"), default=None)
    parser.add_argument("--system", choices=tuple(HOSTED_PLATFORMS), default=None)
    parser.add_argument("--architecture", choices=("aarch64", "x86_64"), default=None)
    parser.add_argument("--source-kind", choices=("branch", "release"), default="release")
    parser.add_argument("--source-sha", default=None)
    parser.add_argument("--nan-version", default=None)
    parser.add_argument("--harness-version", default=None)
    parser.add_argument("--harness-ref", default="")
    args = parser.parse_args()
    try:
        args.model = resolve_model(args.model)
    except ValueError as error:
        parser.error(str(error))
    args.mode = args.mode or ("live" if args.trigger in ("release", "weekly") else "deterministic")
    if not args.source_sha or not SHA256.fullmatch(args.source_sha):
        parser.error("source-sha must be a 40-character lowercase commit SHA")
    args.nan_version = args.nan_version or args.tag[1:]
    if args.stage == "unresolved":
        # Official metadata was unavailable: report the closed failure without
        # inventing a version or touching an installer.
        if args.harness_version is not None or args.harness_ref:
            parser.error("an unresolved harness has no frozen version or ref")
    elif not args.harness_version or not SEMVER.fullmatch(args.harness_version):
        parser.error("harness-version must be an exact semantic version from the frozen manifest")
    if args.harness_ref and not SHA256.fullmatch(args.harness_ref):
        parser.error("harness-ref must be a frozen 40-character lowercase commit SHA")
    for field in ("binary", "canary", "directory", "output"):
        setattr(args, field, getattr(args, field).resolve())
    if not args.tag.startswith("v") or not SEMVER.fullmatch(args.tag[1:]):
        parser.error("expected a semantic release tag")
    ensure_private_directory(args.directory, reusable=(args.directory / "state.json").exists())
    try:
        if args.stage == "unresolved":
            raise RuntimeError("official harness version metadata was unavailable")
        run(args)
    except CleanupError:
        args.output.unlink(missing_ok=True)
        print("Hosted CLI cleanup is unproven; abort the native suite.", file=sys.stderr)
        return 3
    except (OSError, ValueError, KeyError, RuntimeError, subprocess.SubprocessError) as error:
        try:
            failed_report(args, error)
        except (OSError, ValueError, KeyError, RuntimeError, subprocess.SubprocessError):
            args.output.unlink(missing_ok=True)
        # Exception messages and child logs can contain provider/user-controlled text.
        print("Hosted CLI cell failed; no private process output was published.", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
