"""Exercise the real shared host under a job that prohibits breakaway."""

import ctypes
from ctypes import wintypes
import os
import subprocess
import sys


kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
kernel32.CreateJobObjectW.argtypes = [ctypes.c_void_p, wintypes.LPCWSTR]
kernel32.CreateJobObjectW.restype = wintypes.HANDLE
kernel32.GetCurrentProcess.argtypes = []
kernel32.GetCurrentProcess.restype = wintypes.HANDLE
kernel32.AssignProcessToJobObject.argtypes = [wintypes.HANDLE, wintypes.HANDLE]
kernel32.AssignProcessToJobObject.restype = wintypes.BOOL
kernel32.GetConsoleProcessList.argtypes = [ctypes.POINTER(wintypes.DWORD), wintypes.DWORD]
kernel32.GetConsoleProcessList.restype = wintypes.DWORD
kernel32.CloseHandle.argtypes = [wintypes.HANDLE]
kernel32.CloseHandle.restype = wintypes.BOOL


def check(result):
    if not result:
        raise ctypes.WinError(ctypes.get_last_error())
    return result


# A new job has neither BREAKAWAY_OK nor SILENT_BREAKAWAY_OK. Descendants must
# therefore take the production host's PermissionDenied fallback, even outside CI.
job = check(kernel32.CreateJobObjectW(None, None))
try:
    check(kernel32.AssignProcessToJobObject(job, kernel32.GetCurrentProcess()))
    with subprocess.Popen(
        [sys.argv[1], "--exact", "child_session", "--nocapture"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    ) as client:
        try:
            for line in client.stdout:
                if line.strip() == "READY":
                    break
            else:
                raise AssertionError("client failed before readiness: " + client.stderr.read())

            processes = (wintypes.DWORD * 64)()
            count = check(kernel32.GetConsoleProcessList(processes, len(processes)))
            assert count <= len(processes), "unexpected console process count"
            attached = set(processes[:count])
            # The client inherits this fresh console. The shared host and its
            # backend must not receive control events belonging to that client.
            assert attached == {os.getpid(), client.pid}, (
                "shared search processes inherited the client console: " + str(attached)
            )
        finally:
            client.stdin.close()
            try:
                client.wait(timeout=10)
            except subprocess.TimeoutExpired:
                client.kill()
                client.wait(timeout=5)
        assert client.returncode == 0, client.stderr.read()
finally:
    kernel32.CloseHandle(job)
