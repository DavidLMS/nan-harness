"""Synthetic native launcher; no Hermes installation or provider is contacted."""
import json
import os
from pathlib import Path
import signal
import socket
import sys
import time
from urllib.parse import urlsplit

root = Path(__file__).parent


def event(name):
    with (root / "events").open("a") as output:
        output.write(name + "\n")


def mode():
    return (root / "mode").read_text()


if sys.argv[1] == "--print-runtime-command":
    if mode() == "metadata-flood":
        print("x" * 70000, flush=True)
        time.sleep(60)
    runtime = root / "runtime"
    runtime.write_text("#!" + sys.executable + "\nimport os,subprocess,runpy,sys\n"
                       "if " + repr(mode()) + " == 'bootstrap-relaunch' and os.environ.get('HERMES_DISABLE_LAZY_INSTALLS') != '1':\n"
                       "    environment=dict(os.environ,HERMES_DISABLE_LAZY_INSTALLS='1')\n"
                       "    raise SystemExit(subprocess.call([sys.executable,__file__,*sys.argv[1:]],env=environment))\n"
                       "sys.argv=[" + repr(__file__) +
                       ", 'native', *sys.argv[1:]]\nrunpy.run_path(" + repr(__file__) + ",run_name='__main__')\n")
    runtime.chmod(0o700)
    print(json.dumps([str(runtime)]))
    raise SystemExit(0)

action = sys.argv[2]
if action == "absent":
    pass
elif action == "stage":
    (root / "ephemeral").write_text("synthetic config")
    event("stage")
    if mode() == "owner-spawn-failure":
        (root / "runtime").unlink()
elif action == "owner":
    (root / "owner.pending").write_text(str(os.getpid()))
    (root / "owner.pending").replace(root / "owner.pid")
    if mode() == "owner-exit":
        while not (root / "frontend-ready").exists():
            time.sleep(0.005)
        event("owner-exited")
        raise SystemExit(7)
    sys.stdin.buffer.read()
    assert Path(os.environ["NAN_HARNESS_HERMES_SCRIPTS"]).exists()
    if os.environ.get("CHECK_BRIDGE"):
        endpoint = urlsplit(os.environ["CHECK_BRIDGE"])
        with socket.create_connection((endpoint.hostname, endpoint.port), timeout=1):
            event("bridge-alive-during-drain")
    event("owner-exited")
elif action == "ready":
    while not (root / "owner.pid").exists():
        time.sleep(0.005)
    assert int((root / "owner.pid").read_text()) == int(sys.argv[3])
    if mode() == "readiness-failure":
        raise SystemExit(5)
    if mode() == "readiness-wait":
        (root / "readiness-waiting").touch()
        while True:
            time.sleep(0.01)
    event("ready")
elif action == "refresh":
    event("refresh")
    if mode() == "spawn-failure":
        (root / "runtime").unlink()
elif action == "frontend":
    (root / "frontend.pid").write_text(str(os.getpid()))
    event("frontend")
    (root / "frontend-ready").touch()
    if mode() in {"wait", "owner-exit"}:
        signal.signal(signal.SIGINT, signal.SIG_IGN)
        while True:
            time.sleep(0.01)
    raise SystemExit(7 if mode() == "nonzero" else 0)
elif action == "cleanup":
    assert "owner-exited" in (root / "events").read_text()
    event("cleanup")
    if mode() == "cleanup-failure":
        raise SystemExit(8)
    (root / "ephemeral").unlink()
else:
    raise SystemExit(9)
