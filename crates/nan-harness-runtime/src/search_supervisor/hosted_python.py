import os
import runpy
import sys
import threading


def watch_host():
    # The host is the only writer. EOF also covers SIGKILL and a crashed runtime.
    while sys.stdin.buffer.read(1):
        pass
    os._exit(0)


threading.Thread(target=watch_host, daemon=True).start()
sys.argv = sys.argv[1:]
runpy.run_module(sys.argv[0], run_name="__main__", alter_sys=True)
