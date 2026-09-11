import os
import runpy
import sys
import threading
import time


def watch_windows_host():
    if os.name != "nt":
        return

    import ctypes

    synchronize = 0x00100000
    wait_timeout = 0x00000102
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel32.OpenProcess.argtypes = [ctypes.c_uint32, ctypes.c_int, ctypes.c_uint32]
    kernel32.OpenProcess.restype = ctypes.c_void_p
    kernel32.WaitForSingleObject.argtypes = [ctypes.c_void_p, ctypes.c_uint32]
    kernel32.WaitForSingleObject.restype = ctypes.c_uint32
    kernel32.CloseHandle.argtypes = [ctypes.c_void_p]
    kernel32.CloseHandle.restype = ctypes.c_int
    parent = os.getppid()
    while True:
        handle = kernel32.OpenProcess(synchronize, False, parent)
        if not handle:
            os._exit(0)
        try:
            if kernel32.WaitForSingleObject(handle, 0) != wait_timeout:
                os._exit(0)
        finally:
            kernel32.CloseHandle(handle)
        time.sleep(0.1)


def watch_host():
    # The host is the only writer. EOF also covers SIGKILL and a crashed runtime.
    while sys.stdin.buffer.read(1):
        pass
    os._exit(0)


threading.Thread(target=watch_host, daemon=True).start()
threading.Thread(target=watch_windows_host, daemon=True).start()
sys.argv = sys.argv[1:]
runpy.run_module(sys.argv[0], run_name="__main__", alter_sys=True)
