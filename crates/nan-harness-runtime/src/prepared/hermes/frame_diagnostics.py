"""Opt-in source-position samples for the disposable native qualification only."""
from contextlib import contextmanager
import hashlib
import json
import os
from pathlib import Path
import re
import sys
import tempfile
import threading
import time

MODULES = json.loads(Path(__file__).with_name("frame_modules.json").read_text(encoding="utf-8"))
ACTIONS = {"owner", "ready", "frontend"}
_last_error = None


def module_identifier(filename):
    filename = filename.replace("\\", "/").lower()
    return next((name for suffix, name in MODULES.items()
                 if filename == suffix or filename.endswith("/" + suffix)), None)


def public_frames(frame):
    positions, standard = [], []
    for _ in range(100):
        if frame is None or len(positions) == 3:
            break
        module = module_identifier(frame.f_code.co_filename)
        if module and 0 < frame.f_lineno <= 100000:
            target = positions if is_sdk_module(module) else standard
            target.append({"module": module, "line": frame.f_lineno})
        frame = frame.f_back
    return (positions or standard)[:3]


def is_sdk_module(module):
    return module not in {"asyncio_windows", "asyncio_proactor", "asyncio_base", "asyncio_events",
                          "thread_pool", "threading", "subprocess", "selectors", "socket", "tls", "ctypes"}


def snapshot(home, action, identity, sequence):
    current = threading.get_ident()
    frames = sys._current_frames()
    main = threading.main_thread().ident
    threads = sorted(frames, key=lambda identity: (identity != main, identity))
    stacks = [positions for thread in threads[:32]
              if thread != current and (positions := public_frames(frames[thread]))][:4]
    value = {"schemaVersion": 1, "identity": identity, "action": action,
             "sequence": sequence, "stacks": stacks}
    private_receipt(home, ".nanh-frames-" + action + ".json", value)


def private_receipt(home, name, value):
    descriptor, temporary = tempfile.mkstemp(prefix=".nanh-frames-", dir=home)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as stream:
            json.dump(value, stream)
        # Replace the directory entry, never open an existing link's target.
        os.replace(temporary, home / name)
    finally:
        try:
            os.unlink(temporary)
        except FileNotFoundError:
            pass


def record_exception(error, deadline=None, filename=".nanh-control-error.json"):
    """Keep only public throw sites and bounded OS codes; never retain the error."""
    global _last_error
    identity = os.environ.get("NAN_HARNESS_HERMES_DIAGNOSTICS", "")
    if not re.fullmatch(r"[0-9a-f]{32}", identity):
        return
    try:
        home = Path(os.environ["NAN_HARNESS_HERMES_MANAGED_HOME"])
        positions = []
        traceback = error.__traceback__
        for _ in range(100):
            if traceback is None:
                break
            module = module_identifier(traceback.tb_frame.f_code.co_filename)
            if module and 0 < traceback.tb_lineno <= 100000:
                positions.append({"module": module, "line": traceback.tb_lineno})
            traceback = traceback.tb_next
        kind = ("missing" if isinstance(error, FileNotFoundError) else "permission" if isinstance(error, PermissionError)
                else "timeout" if isinstance(error, TimeoutError) else "io" if isinstance(error, OSError) else "protocol")
        codes = {"errno": None, "winerror": None}
        cause = error
        for _ in range(4):
            if cause is None:
                break
            if isinstance(cause, OSError):
                for name in codes:
                    value = getattr(cause, name, None)
                    if type(value) is int and 0 <= value <= 65535:
                        codes[name] = value
            cause = cause.__cause__ or cause.__context__
        deadline_ns = int(deadline * 1_000_000_000) if type(deadline) in (int, float) and 0 <= deadline < 9_223_372_036 else None
        value = {"schemaVersion": 1, "identity": identity, "kind": kind, **codes,
                 "frames": positions[-3:], "deadlineNs": deadline_ns}
        signature = (identity, str(home), json.dumps(value, sort_keys=True))
        if signature != _last_error:
            value["observedNs"] = time.monotonic_ns()
            private_receipt(home, filename, value)
            _last_error = signature
    except Exception:
        pass  # Advisory evidence cannot mask the original failure or cleanup.


def clear_exception():
    global _last_error
    if not re.fullmatch(r"[0-9a-f]{32}", os.environ.get("NAN_HARNESS_HERMES_DIAGNOSTICS", "")):
        return
    try:
        (Path(os.environ["NAN_HARNESS_HERMES_MANAGED_HOME"]) / ".nanh-control-error.json").unlink(missing_ok=True)
        _last_error = None
    except Exception:
        pass


def readiness_state(home, state):
    identity = os.environ.get("NAN_HARNESS_HERMES_DIAGNOSTICS", "")
    if not re.fullmatch(r"[0-9a-f]{32}", identity):
        return
    try:
        private_receipt(home, ".nanh-readiness-state.json", {"identity": identity, "state": state})
        if state == "expired":
            path = home / ".nanh-frames-owner.json"
            if not path.is_symlink() and path.stat().st_size <= 16384:
                value = json.loads(path.read_bytes())
                if value.get("identity") == identity and value.get("action") == "owner":
                    private_receipt(home, ".nanh-frames-owner-expired.json", value)
    except Exception:
        pass  # Observation cannot change readiness or shutdown.


def readiness_failure(home, exit_code):
    identity = os.environ.get("NAN_HARNESS_HERMES_DIAGNOSTICS", "")
    if not re.fullmatch(r"[0-9a-f]{32}", identity):
        return
    try:
        private_receipt(home, ".nanh-readiness-failure.json", {
            "schemaVersion": 1, "identity": identity, "exitCode": exit_code})
    except Exception:
        pass


def current_readiness_state(home, identity):
    try:
        path = home / ".nanh-readiness-state.json"
        if not path.is_symlink() and path.stat().st_size <= 256:
            value = json.loads(path.read_bytes())
            if (set(value) == {"identity", "state"} and value["identity"] == identity
                    and value["state"] in {"waiting", "expired", "host-closed"}):
                return value["state"]
    except Exception:
        pass
    return "unobserved"


def trace_control_namespace(managed_home, action):
    """Observe name calculations without changing the native name or inspecting locals."""
    identity = os.environ.get("NAN_HARNESS_HERMES_DIAGNOSTICS", "")
    if action not in ACTIONS or not re.fullmatch(r"[0-9a-f]{32}", identity):
        return
    from gateway import control_socket
    original = control_socket.windows_pipe_name
    if getattr(original, "_nanh_namespace_trace", False):
        return
    seen = set()
    lock = threading.Lock()

    def observed(home):
        name = original(home)
        try:
            caller = sys._getframe(1).f_code
            role = {"_create_pipe": "server", "query_runtime_control": "client"}.get(caller.co_name)
            if role is None or module_identifier(caller.co_filename) != "runtime_windows":
                return name
            digest = hashlib.sha256(name.encode("utf-8")).hexdigest()
            key = (role, digest)
            with lock:
                if key in seen:
                    return name
                # Supported SDK Python3.11+ shares this monotonic clock across processes.
                observed_ns = time.monotonic_ns()
                try:
                    physical_match = Path(home).samefile(managed_home)
                except OSError:
                    physical_match = None
                value = {"schemaVersion": 1, "identity": identity, "action": action, "role": role,
                         "digest": digest, "matchesManagedName": name == original(managed_home),
                         "samePhysicalHome": physical_match,
                         "verbatimHome": str(home).replace(chr(92), "/").startswith("//?/"),
                         "observedNs": observed_ns,
                         "readinessState": current_readiness_state(managed_home, identity)}
                private_receipt(managed_home, ".nanh-namespace-" + action + "-" + role + ".json", value)
                seen.add(key)
        except Exception:
            pass  # Always return the exact native value, even when observation fails.
        return name

    observed._nanh_namespace_trace = True
    control_socket.windows_pipe_name = observed


@contextmanager
def watch(action):
    identity = os.environ.get("NAN_HARNESS_HERMES_DIAGNOSTICS", "")
    if action not in ACTIONS or not re.fullmatch(r"[0-9a-f]{32}", identity):
        yield
        return
    home = Path(os.environ["NAN_HARNESS_HERMES_MANAGED_HOME"])
    stop = threading.Event()

    def sample():
        sequence = 0
        while not stop.is_set():
            try:
                snapshot(home, action, identity, sequence)
            except Exception:
                pass  # Advisory diagnostics cannot change native execution or cleanup.
            sequence += 1
            stop.wait(3)

    thread = threading.Thread(target=sample, name="nanh-source-positions", daemon=True)
    thread.start()
    try:
        yield
    finally:
        stop.set()
        thread.join(timeout=1)
