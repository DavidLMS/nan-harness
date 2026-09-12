#!/usr/bin/env python3
"""Native Windows-only security contracts for hosted CLI stage supervision."""

import os
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


def main():
    if os.name != "nt":
        print("Windows native qualification must run on windows-2025")
        return 2
    with tempfile.TemporaryDirectory() as temporary:
        directory = Path(temporary) / "private"
        cell.ensure_private_directory(directory)
        sid = cell.windows_current_user_sid()
        acl = subprocess.run(["icacls", str(directory)], capture_output=True, check=True).stdout.decode(errors="replace")
        if sid not in acl or "S-1-5-18" not in acl or "(OI)(CI)" not in acl:
            raise AssertionError("private directory DACL is not owner/SYSTEM inheritable-only")
        child_script = "import os, subprocess, sys, time; p=subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)']); open(sys.argv[1], 'w').write(str(p.pid)); time.sleep(60)"
        marker = directory / "child.pid"
        try:
            cell.private_command([sys.executable, "-c", child_script, str(marker)], directory,
                                 timeout=0.5)
        except RuntimeError:
            pass
        pid = int(marker.read_text())
        assert_gone(pid)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
