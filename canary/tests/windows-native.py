#!/usr/bin/env python3
"""Native Windows-only security contracts for hosted CLI stage supervision."""

import os
import ctypes
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "actions"))
import cell


def assert_gone(pid):
    for _ in range(20):
        result = subprocess.run(["tasklist", "/FI", f"PID eq {pid}"], capture_output=True, check=False)
        if str(pid).encode() not in result.stdout:
            return
        time.sleep(0.25)
    raise AssertionError("Windows Job Object left a descendant alive")


def acl_entries(path):
    from ctypes import wintypes
    api = ctypes.windll.advapi32
    pointer = wintypes.LPVOID
    api.GetNamedSecurityInfoW.argtypes = [wintypes.LPWSTR, wintypes.DWORD, wintypes.DWORD,
                                         pointer, pointer, ctypes.POINTER(pointer), pointer,
                                         ctypes.POINTER(pointer)]
    api.GetNamedSecurityInfoW.restype = wintypes.DWORD
    api.GetSecurityDescriptorControl.argtypes = [pointer, ctypes.POINTER(wintypes.WORD),
                                                 ctypes.POINTER(wintypes.DWORD)]
    api.GetSecurityDescriptorControl.restype = wintypes.BOOL
    api.GetAclInformation.argtypes = [pointer, pointer, wintypes.DWORD, wintypes.DWORD]
    api.GetAclInformation.restype = wintypes.BOOL
    api.GetAce.argtypes = [pointer, wintypes.DWORD, ctypes.POINTER(pointer)]
    api.GetAce.restype = wintypes.BOOL
    acl, descriptor = pointer(), pointer()
    error = api.GetNamedSecurityInfoW(str(path), 1, 4, None, None, ctypes.byref(acl), None,
                                     ctypes.byref(descriptor))
    if error:
        raise AssertionError(f"native DACL query failed with Windows error {error}")
    try:
        control, revision = wintypes.WORD(), wintypes.DWORD()
        if not api.GetSecurityDescriptorControl(descriptor, ctypes.byref(control), ctypes.byref(revision)):
            raise AssertionError("native DACL control query failed")
        size = (wintypes.DWORD * 3)()
        if not acl or not api.GetAclInformation(acl, size, ctypes.sizeof(size), 2) or size[0] > 16:
            raise AssertionError("native DACL entry count is invalid")
        entries = []
        for index in range(size[0]):
            ace = pointer()
            if not api.GetAce(acl, index, ctypes.byref(ace)):
                raise AssertionError("native DACL entry could not be read")
            header = ctypes.string_at(ace, 4)
            if header[0] != 0 or int.from_bytes(header[2:4], "little") < 12:
                raise AssertionError("native DACL contains a non-allow entry")
            sid = wintypes.LPWSTR()
            if not api.ConvertSidToStringSidW(ace.value + 8, ctypes.byref(sid)):
                raise AssertionError("native DACL SID could not be decoded")
            try:
                entries.append({"sid": sid.value, "type": 0, "inherit": header[1] & 3,
                                "prop": bool(header[1] & 16), "protected": bool(control.value & 0x1000)})
            finally:
                ctypes.windll.kernel32.LocalFree(sid)
        return entries
    finally:
        ctypes.windll.kernel32.LocalFree(descriptor)


def main():
    if os.name != "nt":
        print("Windows native qualification must run on windows-2025")
        return 2
    with tempfile.TemporaryDirectory() as temporary:
        if ctypes.sizeof(cell.IOCounters) != 48:
            raise AssertionError("IO_COUNTERS ABI size is not six ULONGLONG fields")
        if [getattr(cell.IOCounters, name).offset for name in (
                "ReadOperationCount", "WriteOperationCount", "OtherOperationCount",
                "ReadTransferCount", "WriteTransferCount", "OtherTransferCount")] != [0, 8, 16, 24, 32, 40]:
            raise AssertionError("IO_COUNTERS ABI offsets are not SDK-compatible")
        if cell.ExtendedLimitInformation.io.offset != ctypes.sizeof(cell.BasicLimitInformation):
            raise AssertionError("extended job limit structure has an invalid IO_COUNTERS offset")
        try:
            cell.WindowsJob(0)
        except RuntimeError:
            pass
        else:
            raise AssertionError("invalid process assignment unexpectedly succeeded")
        suspended = subprocess.Popen(["cmd", "/c", "exit", "0"],
                                     creationflags=0x00000004)  # CREATE_SUSPENDED
        try:
            owner = cell.WindowsJob(suspended.pid)
            try:
                try:
                    owner.resume(suspended.pid + 1)
                except RuntimeError:
                    pass
                else:
                    raise AssertionError("invalid suspended-thread resume unexpectedly succeeded")
            finally:
                owner.close()
        finally:
            suspended.kill()
            suspended.wait(timeout=10)
        directory = Path(temporary) / "private"
        cell.ensure_private_directory(directory)
        sid = cell.windows_current_user_sid()
        entries = acl_entries(directory)
        allowed = {sid, "S-1-5-18"}
        if {item["sid"] for item in entries} != allowed or any(
                item["type"] != 0 or item["prop"] or item["inherit"] != 3 or not item["protected"]
                for item in entries):
            print(json.dumps({"stage": "private-directory-acl", "principalsMatch":
                              {item["sid"] for item in entries} == allowed,
                              "entries": [{"principal": "owner" if item["sid"] == sid else
                                           "system" if item["sid"] == "S-1-5-18" else "other",
                                           "allow": item["type"] == 0, "inherited": item["prop"],
                                           "inheritance": item["inherit"], "protected": item["protected"]}
                                          for item in entries[:8]]}, sort_keys=True))
            raise AssertionError("private directory DACL is not owner/SYSTEM inheritable-only")
        child_script = "import os, subprocess, sys, time; p=subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)']); open(sys.argv[1], 'w').write(str(p.pid)); time.sleep(60)"
        marker = directory / "child.pid"
        try:
            cell.private_command([sys.executable, "-c", child_script, str(marker)], directory,
                                 timeout=0.5)
        except RuntimeError:
            pass
        pid = int(marker.read_text())
        file_entries = acl_entries(marker)
        if {item["sid"] for item in file_entries} != allowed or any(
                item["type"] != 0 or item["inherit"] != 0
                for item in file_entries):
            raise AssertionError("child payload file DACL is not owner/SYSTEM-only")
        with cell.private_log(directory) as log:
            covered = acl_entries(Path(log.name))
            if {item["sid"] for item in covered} != allowed or any(
                    item["type"] != 0 or item["prop"] or not item["protected"] for item in covered):
                raise AssertionError("launcher log was not protected before its first payload")
            log.write(b"synthetic payload")
        assert_gone(pid)
        foreground = directory / "foreground.pid"
        foreground_script = "import subprocess,sys,time; p=subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)']); open(sys.argv[1], 'w').write(str(p.pid))"
        cell.private_command([sys.executable, "-c", foreground_script, str(foreground)], directory)
        assert_gone(int(foreground.read_text()))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
