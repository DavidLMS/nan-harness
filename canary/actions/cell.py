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
            entry.permissions = 0x10000000  # GENERIC_ALL
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


def private_command(command, directory, timeout=900, output=None, live=False, allow_failure=False):
    env = os.environ.copy()
    if not live:
        env.pop("NAN_API_KEY", None)
    env["NAN_CANARY_REDACT_FAILURE_OUTPUT"] = "1"
    env["CI"] = "1"
    with tempfile.TemporaryFile(dir=directory) as log:
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
                raise RuntimeError("stage exceeded its execution limit") from None
            finally:
                # A harness may leave descendants after its foreground process exits.
                try:
                    terminate_process_tree(child.pid)
                except (OSError, ProcessLookupError):
                    pass
                try:
                    child.wait(timeout=10)
                except (subprocess.TimeoutExpired, OSError):
                    pass
                if job:
                    job.close()
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


class WindowsJob:
    """Typed kill-on-close Job Object attached before a Windows stage resumes."""

    def __init__(self, pid):
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
            terminated = ctypes.windll.kernel32.TerminateJobObject(self.handle, 1)
            error = ctypes.windll.kernel32.GetLastError() if not terminated else 0
            # A completed job may report no live process; closing still
            # enforces KILL_ON_JOB_CLOSE for any descendants.
            closed = ctypes.windll.kernel32.CloseHandle(self.handle)
            self.handle = None
            if not closed:
                raise RuntimeError("Windows process cleanup could not close its Job Object")
            if not terminated and error not in (5, 87):
                raise RuntimeError("Windows process cleanup could not terminate its Job Object")


def install(args, state):
    installer = ROOT / "canary/guest/install-harness.ps1" if os.name == "nt" else ROOT / "canary/guest/install-harness.sh"
    command = ["powershell", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File",
               str(installer), args.harness, args.harness_version] if os.name == "nt" else ["bash", str(installer), args.harness, args.harness_version]
    private_command(command, args.directory)
    doctor = args.directory / "doctor.json"
    private_command([str(args.binary), "doctor", args.harness, "--allow-unsupported",
                     "--allow-untested", "--json"], args.directory, output=doctor)
    try:
        version = json.loads(doctor.read_bytes())["version"]
        if not isinstance(version, str) or not SEMVER.fullmatch(version) or version != args.harness_version:
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
    parser.add_argument("--mode", choices=("deterministic", "live"), default=None)
    parser.add_argument("--system", choices=("linux", "macos", "windows"), default=None)
    parser.add_argument("--architecture", choices=("aarch64", "x86_64"), default=None)
    parser.add_argument("--source-kind", choices=("branch", "release"), default="release")
    parser.add_argument("--source-sha", default=None)
    parser.add_argument("--nan-version", default=None)
    parser.add_argument("--harness-version", default=None)
    args = parser.parse_args()
    try:
        args.model = resolve_model(args.model)
    except ValueError as error:
        parser.error(str(error))
    args.mode = args.mode or ("live" if args.trigger in ("release", "weekly") else "deterministic")
    if not args.source_sha or not SHA256.fullmatch(args.source_sha):
        parser.error("source-sha must be a 40-character lowercase commit SHA")
    args.nan_version = args.nan_version or args.tag[1:]
    if not args.harness_version or not SEMVER.fullmatch(args.harness_version):
        parser.error("harness-version must be an exact semantic version from the frozen manifest")
    for field in ("binary", "canary", "directory", "output"):
        setattr(args, field, getattr(args, field).resolve())
    if not args.tag.startswith("v") or not SEMVER.fullmatch(args.tag[1:]):
        parser.error("expected a semantic release tag")
    ensure_private_directory(args.directory, reusable=(args.directory / "state.json").exists())
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
