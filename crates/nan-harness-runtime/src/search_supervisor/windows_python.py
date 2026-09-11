import os
import runpy
import sys
import types
from collections import namedtuple


def install_posix_compatibility():
    if os.name != "nt":
        return

    # SearXNG only uses these POSIX APIs while reporting a failed Valkey connection. Keep the
    # compatibility layer in this child process instead of adding platform files to its source.
    passwd = namedtuple(
        "passwd", "pw_name pw_passwd pw_uid pw_gid pw_gecos pw_dir pw_shell"
    )
    pwd = types.ModuleType("pwd")
    pwd.getpwuid = lambda uid: passwd(str(uid), "", uid, -1, "", "", "")
    sys.modules.setdefault("pwd", pwd)
    if not hasattr(os, "getuid"):
        os.getuid = lambda: 0


install_posix_compatibility()
sys.argv = sys.argv[1:]
runpy.run_module(sys.argv[0], run_name="__main__", alter_sys=True)
