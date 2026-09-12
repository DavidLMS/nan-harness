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
from selection import CLI_HARNESSES, resolve_model

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
    for location in set(locations.values()):
        ensure_private_directory(location, reusable=True)
    # Keep runner-provisioned runtimes, but do not discover a harness left in the
    # shared user profile or in a sibling cell by an earlier cell or installer.
    hidden = [Path(env[key]).resolve() for key in ("HOME", "USERPROFILE") if env.get(key)]
    hidden.append(directory.resolve().parent)
    inherited = [entry for entry in env.get("PATH", "").split(os.pathsep)
                 if entry and not any(Path(entry).resolve().is_relative_to(old) for old in hidden)]
    bins = [home / ".local/bin", home / ".local", home / ".kimi-code/bin",
            home / ".hermes/bin", home / ".local/share/nan-harness-canary-uv/bin"]
    env.update({key: str(value) for key, value in locations.items()})
    env["PATH"] = os.pathsep.join([str(path) for path in bins] + inherited)
    return env


def private_command(command, directory, timeout=900, output=None, live=False, allow_failure=False,
                    environment=None):
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
                finish_stage(child, job)
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
        command = ["powershell", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File",
                   str(ROOT / "canary/guest/install-harness.ps1"), "-Harness", harness, "-Version", version]
        return command + (["-Ref", ref] if ref else [])
    return ["bash", str(ROOT / "canary/guest/install-harness.sh"), harness, version] + ([ref] if ref else [])


def install(args, state):
    command = installer_command(args.harness, args.harness_version, getattr(args, "harness_ref", "") or "")
    environment = cell_environment(args.directory)
    private_command(command, args.directory, environment=environment)
    doctor = args.directory / "doctor.json"
    private_command([str(args.binary), "doctor", args.harness, "--allow-unsupported",
                     "--allow-untested", "--json"], args.directory, output=doctor, environment=environment)
    try:
        version = json.loads(doctor.read_bytes())["version"]
        if not isinstance(version, str) or not SEMVER.fullmatch(version) or version != args.harness_version:
            raise ValueError()
        state["harness"]["version"] = version
    except (KeyError, ValueError, TypeError):
        raise RuntimeError("installed version could not be verified") from None
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
    if (not isinstance(value, dict) or set(value) != {"schemaVersion", "stage", "status"}
            or value["schemaVersion"] != 1 or value["stage"] not in PROBE_STAGES
            or value["status"] not in ("passed", "failed")
            or (value["status"] == "passed") != (value["stage"] == "complete")):
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
    probe = ROOT / "canary/guest/probe-harness.ps1" if os.name == "nt" else ROOT / "canary/guest/probe-harness.sh"
    command = ["powershell", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File",
               str(probe), args.harness] if os.name == "nt" else ["bash", str(probe), args.harness]
    try:
        status = private_command(command, args.directory, timeout=600, live=True,
                                 allow_failure=True, environment=environment)
        result = probe_result(marker)
    finally:
        marker.unlink(missing_ok=True)
    if status == 0 and result is not None and result["status"] == "passed":
        return 1
    if result is not None and result["stage"] == "cleanup":
        raise ProbeCleanupError("live probe workspace cleanup is unproven")
    if status != 0 and result is not None and result["stage"] in LIVE_MISMATCH_STAGES:
        raise CompatibilityMismatch("live:" + result["stage"])
    raise RuntimeError("live probe did not pass")


def initial_state(args):
    detected_platform = {"linux": "linux", "darwin": "macos", "win32": "windows"}.get(sys.platform)
    platform = getattr(args, "system", None) or detected_platform
    machine = os.environ.get("PROCESSOR_ARCHITECTURE", "") if os.name == "nt" else getattr(os, "uname")().machine
    if platform is None or machine.lower() not in ("arm64", "aarch64", "x86_64", "amd64"):
        raise RuntimeError("this gate requires a supported hosted runner")
    architecture = getattr(args, "architecture", None) or ("x86_64" if machine.lower() in ("x86_64", "amd64") else "aarch64")
    detected_architecture = "x86_64" if machine.lower() in ("x86_64", "amd64") else "aarch64"
    if (getattr(args, "system", None) and args.system != detected_platform) or (
            getattr(args, "architecture", None) and args.architecture != detected_architecture):
        raise RuntimeError("requested hosted identity does not match the native runner")
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
    code = None
    summary = "Hosted check did not complete successfully."
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
    parser.add_argument("--system", choices=("linux", "macos", "windows"), default=None)
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
