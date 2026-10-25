"""Closed diagnostics from synthetic managed-profile output; never export text."""
import json
import os
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
# Specific native causes precede generic turn failures and progress facts.
DIAGNOSTIC_CODES = tuple(PREFIX + code for code in (
    *("WINERROR-" + str(code) for code in WINERRORS),
    *("EXCEPTION-" + code for code in EXCEPTIONS if code not in {"VALUE", "RUNTIME", "OS"}),
    *("RPC-" + reason.replace("_", "-").upper() for reason in RPC_REASONS),
    "EXCEPTION-VALUE", "EXCEPTION-RUNTIME", "EXCEPTION-OS",
    *SIGNATURES, *PHASES,
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
