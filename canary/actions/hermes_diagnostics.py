"""Closed diagnostics from synthetic managed-profile output; never export text."""
import json
import os
from pathlib import Path
import re

LIMIT = 64 * 1024
WINERRORS = (2, 3, 5, 6, 87, 109, 123, 183, 193, 206, 231, 232, 233, 267)
RPC_REASONS = (
    "invalid_params", "not_found", "admission_conflict", "permission_denied", "profile_mismatch",
    "runtime_draining", "session_superseded", "chain_moved", "stale_generation", "storage_unavailable",
    "workspace_unavailable", "launch_credentials_unavailable", "unknown_skill", "unsupported_managed_policy",
    "unsupported_producer", "worker_not_live", "producer_not_started", "invalid_worker_frame",
    "invalid_worker_result", "managed_worker_failed", "managed_worker_hello_timeout", "managed_worker_lost",
    "managed_worker_stopped", "worker_control_backpressure", "unknown_execution", "request_failed",
    "invalid_managed_worker_bootstrap", "invalid_managed_worker_frame", "managed_worker_frame_too_large",
    "invalid_managed_control", "invalid_execution_id", "managed_attachment_unavailable", "managed_finish_timeout",
)
EXCEPTIONS = {
    "IMPORT": ("ImportError", "ModuleNotFoundError"),
    "PATH": ("FileNotFoundError", "NotADirectoryError"),
    "PERMISSION": ("PermissionError",),
    "ENCODING": ("UnicodeEncodeError", "UnicodeDecodeError"),
    "PIPE": ("BrokenPipeError", "ConnectionResetError"),
    "TIMEOUT": ("TimeoutError",),
    "SQLITE": ("OperationalError", "DatabaseError", "IntegrityError"),
    "ATTRIBUTE": ("AttributeError",), "TYPE": ("TypeError",), "VALUE": ("ValueError",),
    "KEY": ("KeyError",), "RUNTIME": ("RuntimeError",), "OS": ("OSError",),
}
SIGNATURES = {
    "CONSOLE": ("NoConsoleScreenBufferError", "No Windows console found. Are you running cmd.exe?"),
    "URI": ("invalid uri authority:",),
    "CONNECTION": ("Gateway connection failed; no local fallback", "Gateway connection/read failed; no local fallback"),
    "PROTOCOL": ("Gateway bootstrap timed out", "Invalid gateway bootstrap response",
                 "Gateway bootstrap identity changed; retry launch", "Gateway protocol mismatch; update/restart required"),
    "CREDENTIALS": ("Error: credentials or agent init failed",),
    "TURN-FAILED": ("The admitted turn failed.",),
    "WORKER-UNKNOWN": ("Worker execution is unknown.",),
    "SQLITE-OPEN": ("unable to open database file",),
    "SQLITE-READONLY": ("attempt to write a readonly database",),
    "SQLITE-LOCKED": ("database is locked",),
    "SQLITE-SCHEMA": ("no such table:", "no such column:"),
}
for module in ("fcntl", "pwd", "termios", "resource", "hermes_yaml", "pydantic", "websockets", "openai", "rich"):
    SIGNATURES["IMPORT-" + module.replace("_", "-").upper()] = ("No module named '" + module + "'",)
PHASES = ("RESULT-BOUND", "RESULT-UNBOUND", "INIT-ONLY", "NO-INIT", "PROVIDER-OBSERVED", "PROVIDER-ABSENT")
PREFIX = "NH-HERMES-FRONTEND-"
FRAME_MODULES = set(json.loads((Path(__file__).resolve().parents[2]
    / "crates/nan-harness-runtime/src/prepared/hermes/frame_modules.json").read_text()).values())
# Specific native causes precede generic turn failures and progress facts.
DIAGNOSTIC_CODES = tuple(PREFIX + code for code in (
    *("WINERROR-" + str(code) for code in WINERRORS),
    *("EXCEPTION-" + code for code in EXCEPTIONS if code not in {"VALUE", "RUNTIME", "OS"}),
    *("RPC-" + reason.replace("_", "-").upper() for reason in RPC_REASONS),
    "EXCEPTION-VALUE", "EXCEPTION-RUNTIME", "EXCEPTION-OS",
    *SIGNATURES, *PHASES,
))
DIAGNOSTIC_CODES += tuple("NH-HERMES-CONTROL-" + code for code in (
    "OWNER-HOME-MATCH", "OWNER-HOME-MISMATCH", "OWNER-HOME-UNKNOWN", "OWNER-VERBATIM",
    "READY-HOME-MATCH", "READY-HOME-MISMATCH", "READY-HOME-UNKNOWN", "READY-VERBATIM",
    "FRONTEND-HOME-MATCH", "FRONTEND-HOME-MISMATCH", "FRONTEND-HOME-UNKNOWN", "FRONTEND-VERBATIM",
    "OWNER-NAME-MATCH", "OWNER-NAME-MISMATCH", "READY-NAME-MATCH", "READY-NAME-MISMATCH",
    "FRONTEND-NAME-MATCH", "FRONTEND-NAME-MISMATCH",
    "READY-NAMESPACE-MATCH", "READY-NAMESPACE-MISMATCH", "FRONTEND-NAMESPACE-MATCH", "FRONTEND-NAMESPACE-MISMATCH",
    "SERVER-NAME-BEFORE-DEADLINE", "SERVER-NAME-LATE-UNDER-5S", "SERVER-NAME-LATE-UNDER-10S", "SERVER-NAME-LATE-OVER-10S",
    "SERVER-NAME-READY-WAITING", "SERVER-NAME-READY-EXPIRED", "SERVER-NAME-HOST-CLOSED", "SERVER-NAME-PHASE-UNOBSERVED",
))


def bounded_tail(stream):
    stream.seek(0, os.SEEK_END)
    stream.seek(max(0, stream.tell() - LIMIT))
    return stream.read(LIMIT).decode("utf-8", errors="replace")


def closed_frontend_diagnostics(text):
    codes = [PREFIX + "WINERROR-" + str(code) for code in WINERRORS if f"[WinError {code}]" in text]
    codes += [PREFIX + "EXCEPTION-" + code for code, names in EXCEPTIONS.items()
              if any(re.search(r"\b" + name + r":", text) for name in names)]
    codes += [PREFIX + "RPC-" + reason.replace("_", "-").upper() for reason in RPC_REASONS
              if re.search(r"(?<![\w])" + reason + r"(?![\w])", text)]
    codes += [PREFIX + code for code, patterns in SIGNATURES.items() if any(pattern in text for pattern in patterns)]
    return codes


def frontend_phase(text):
    initialized = False
    for line in text.splitlines():
        try:
            value = json.loads(line)
        except ValueError:
            continue
        if not isinstance(value, dict):
            continue
        initialized |= value.get("type") == "system" and value.get("subtype") == "init"
        if value.get("type") == "result":
            return PREFIX + ("RESULT-BOUND" if bool(value.get("session_id")) else "RESULT-UNBOUND")
    return PREFIX + ("INIT-ONLY" if initialized else "NO-INIT")


def managed_profile_diagnostics(config):
    """Read only declared logs physically inside this disposable NaN profile."""
    parent = (config / "hermes-profiles").resolve()
    profiles = list(parent.glob("*"))
    if len(profiles) != 1 or profiles[0].is_symlink() or not profiles[0].resolve().is_relative_to(parent):
        return []
    profile = profiles[0].resolve()
    codes = []
    for name in ("errors.log", "gateway.log", "managed-worker.log"):
        for suffix in ("", ".1"):
            path = profile / "logs" / (name + suffix)
            try:
                if path.is_symlink() or not path.resolve().is_relative_to(profile) or not path.is_file():
                    continue
                with path.open("rb") as stream:
                    codes.extend(closed_frontend_diagnostics(bounded_tail(stream)))
            except OSError:
                continue
    return list(dict.fromkeys(codes))


def safe_frames(values):
    if not isinstance(values, list) or len(values) > 4:
        return []
    result = []
    for value in values:
        if not isinstance(value, dict):
            return []
        error = value.get("action") == "control-error"
        fields = {"action", "sequence", "stacks"} | ({"errorKind", "errno", "winerror"} if error else set())
        if (set(value) != fields
                or not isinstance(value["action"], str) or value["action"] not in {"owner", "ready", "frontend", "control-error"}
                or type(value["sequence"]) is not int or not 0 <= value["sequence"] <= 100000
                or not isinstance(value["stacks"], list) or len(value["stacks"]) > 4):
            return []
        if error and (not isinstance(value["errorKind"], str) or value["errorKind"] not in {"missing", "permission", "timeout", "io", "protocol"}
                      or any(value[name] is not None and (type(value[name]) is not int or not 0 <= value[name] <= 65535)
                             for name in ("errno", "winerror"))):
            return []
        for stack in value["stacks"]:
            if not isinstance(stack, list) or len(stack) > 3:
                return []
            for frame in stack:
                if (not isinstance(frame, dict) or set(frame) != {"module", "line"}
                        or not isinstance(frame["module"], str) or frame["module"] not in FRAME_MODULES
                        or type(frame["line"]) is not int or not 0 < frame["line"] <= 100000):
                    return []
        result.append(value)
    return result


def managed_profile_frames(config, identity):
    parent = (config / "hermes-profiles").resolve()
    profiles = list(parent.glob("*"))
    if len(profiles) != 1 or profiles[0].is_symlink() or not profiles[0].resolve().is_relative_to(parent):
        return []
    profile = profiles[0].resolve()
    records = []
    for action in ("owner", "ready", "frontend"):
        path = profile / (".nanh-frames-" + action + ".json")
        if action == "owner" and (profile / ".nanh-frames-owner-expired.json").is_file():
            path = profile / ".nanh-frames-owner-expired.json"
        try:
            if path.is_symlink() or not path.resolve().is_relative_to(profile) or path.stat().st_size > 16384:
                continue
            value = json.loads(path.read_bytes())
            if (not isinstance(value, dict) or set(value) != {"schemaVersion", "identity", "action", "sequence", "stacks"}
                    or type(value["schemaVersion"]) is not int or value["schemaVersion"] != 1
                    or value["identity"] != identity or value["action"] != action):
                continue
            records.extend(safe_frames([{key: value[key] for key in ("action", "sequence", "stacks")}]))
        except (OSError, ValueError, TypeError):
            continue
    startup = profile / ".nanh-startup-error.json"
    path = startup if startup.is_file() else profile / ".nanh-control-error.json"
    try:
        if not path.is_symlink() and path.resolve().is_relative_to(profile) and path.stat().st_size <= 16384:
            value = json.loads(path.read_bytes())
            fields = {"schemaVersion", "identity", "kind", "errno", "winerror", "frames"}
            if (isinstance(value, dict) and set(value) in (fields, fields | {"deadlineNs", "observedNs"})
                    and type(value["schemaVersion"]) is int and value["schemaVersion"] == 1 and value["identity"] == identity):
                records.extend(safe_frames([{"action": "control-error", "sequence": 0, "stacks": [value["frames"]],
                    "errorKind": value["kind"], "errno": value["errno"], "winerror": value["winerror"]}]))
    except (OSError, ValueError, TypeError):
        pass
    return records


def managed_namespace_diagnostics(config, identity):
    parent = (config / "hermes-profiles").resolve()
    profiles = list(parent.glob("*"))
    if len(profiles) != 1 or profiles[0].is_symlink() or not profiles[0].resolve().is_relative_to(parent):
        return []
    profile = profiles[0].resolve()

    def read(name):
        path = profile / name
        try:
            if path.is_symlink() or not path.resolve().is_relative_to(profile) or path.stat().st_size > 16384:
                return None
            value = json.loads(path.read_bytes())
            if (isinstance(value, dict) and type(value.get("schemaVersion")) is int
                    and value["schemaVersion"] == 1 and value.get("identity") == identity):
                return value
        except (OSError, ValueError, TypeError):
            pass
        return None

    names, codes = {}, []
    failure = read(".nanh-readiness-failure.json")
    reasons = ("CONTROL-ABSENT", "CONTROL-ACCESS", "CONTROL-TIMEOUT", "CONTROL-PROTOCOL",
               "IDENTITY-PID", "IDENTITY-HOME", "IDENTITY-SUPERVISOR", "RUNTIME-PROTOCOL",
               "RUNTIME-PROFILE", "RUNTIME-AUTHORITY", "RUNTIME-ENDPOINT", "RUNTIME-STATE")
    if (failure and set(failure) == {"schemaVersion", "identity", "exitCode"}
            and type(failure["exitCode"]) is int and 90 <= failure["exitCode"] < 90 + len(reasons)):
        codes.append("NH-HERMES-READINESS-" + reasons[failure["exitCode"] - 90])
    fields = {"schemaVersion", "identity", "action", "role", "digest", "matchesManagedName", "samePhysicalHome", "verbatimHome", "observedNs", "readinessState"}
    for action, role in (("owner", "server"), ("ready", "client"), ("frontend", "client")):
        value = read(".nanh-namespace-" + action + "-" + role + ".json")
        if (not value or set(value) != fields or value["action"] != action or value["role"] != role
                or not isinstance(value["digest"], str) or not re.fullmatch(r"[0-9a-f]{64}", value["digest"])
                or type(value["matchesManagedName"]) is not bool or type(value["verbatimHome"]) is not bool
                or not isinstance(value["readinessState"], str)
                or value["readinessState"] not in {"waiting", "expired", "host-closed", "unobserved"}
                or value["samePhysicalHome"] is not None and type(value["samePhysicalHome"]) is not bool
                or type(value["observedNs"]) is not int or not 0 <= value["observedNs"] < 2**63):
            continue
        names[action] = value
        match = "UNKNOWN" if value["samePhysicalHome"] is None else "MATCH" if value["samePhysicalHome"] else "MISMATCH"
        codes.append("NH-HERMES-CONTROL-" + action.upper() + "-HOME-" + match)
        codes.append("NH-HERMES-CONTROL-" + action.upper() + "-NAME-"
                     + ("MATCH" if value["matchesManagedName"] else "MISMATCH"))
        if value["verbatimHome"]:
            codes.append("NH-HERMES-CONTROL-" + action.upper() + "-VERBATIM")
    if "owner" in names:
        phase = {"waiting": "READY-WAITING", "expired": "READY-EXPIRED",
                 "host-closed": "HOST-CLOSED", "unobserved": "PHASE-UNOBSERVED"}[names["owner"]["readinessState"]]
        codes.append("NH-HERMES-CONTROL-SERVER-NAME-" + phase)
        for action in ("ready", "frontend"):
            if action in names:
                match = "MATCH" if names["owner"]["digest"] == names[action]["digest"] else "MISMATCH"
                codes.append("NH-HERMES-CONTROL-" + action.upper() + "-NAMESPACE-" + match)
        error = read(".nanh-control-error.json")
        error_fields = {"schemaVersion", "identity", "kind", "errno", "winerror", "frames", "deadlineNs", "observedNs"}
        deadline = error.get("deadlineNs") if error and set(error) == error_fields else None
        if type(deadline) is int and 0 <= deadline < 2**63:
            delta = names["owner"]["observedNs"] - deadline
            timing = ("BEFORE-DEADLINE" if delta <= 0 else "LATE-UNDER-5S" if delta <= 5_000_000_000
                      else "LATE-UNDER-10S" if delta <= 10_000_000_000 else "LATE-OVER-10S")
            codes.append("NH-HERMES-CONTROL-SERVER-NAME-" + timing)
    return codes
