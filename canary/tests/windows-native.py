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
    escaped = str(path).replace("'", "''")
    script = "$ErrorActionPreference='Stop'; $a=Get-Acl -LiteralPath '%s'; $a.Access | %% { $sid=$_.IdentityReference.Translate([System.Security.Principal.SecurityIdentifier]).Value; [pscustomobject]@{sid=$sid;type=[int]$_.AccessControlType;inherit=[int]$_.InheritanceFlags;prop=$_.IsInherited;protected=$a.AreAccessRulesProtected} } | ConvertTo-Json -Compress" % escaped
    raw = subprocess.run(["powershell", "-NoProfile", "-NonInteractive", "-Command", script],
                         capture_output=True, check=True).stdout
    value = json.loads(raw.decode(errors="strict"))
    return value if isinstance(value, list) else [value]


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
