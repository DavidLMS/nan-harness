"""Read-only observation of private fixture gateways; never signal discovered PIDs."""
import json
import os
import threading
import time


class Gateways:
    def __init__(self, config):
        self.config = config
        self.seen = {}
        self.stop = threading.Event()
        if os.name == "nt":
            import ctypes
            from ctypes import wintypes
            self.kernel = ctypes.WinDLL("kernel32", use_last_error=True)
            self.kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
            self.kernel.OpenProcess.restype = wintypes.HANDLE
            self.kernel.WaitForSingleObject.argtypes = [wintypes.HANDLE, wintypes.DWORD]
            self.kernel.WaitForSingleObject.restype = wintypes.DWORD
            self.kernel.CloseHandle.argtypes = [wintypes.HANDLE]
            self.kernel.CloseHandle.restype = wintypes.BOOL
        self.thread = threading.Thread(target=self.observe, daemon=True)
        self.thread.start()

    def observe(self):
        while not self.stop.wait(0.02):
            for path in self.config.glob("hermes-profiles/*/gateway.pid"):
                try:
                    if path.stat().st_size > 16384:
                        raise ValueError()
                    record = json.loads(path.read_bytes())
                    pid, started = record["pid"], record["start_time"]
                    if type(pid) is not int or pid <= 0 or started is None:
                        raise ValueError()
                    key = (pid, str(started))
                    if key in self.seen:
                        continue
                    handle = None
                    if os.name == "nt":
                        # Retain the actual process handle; PID reuse cannot change
                        # the process whose final exit this observer waits for.
                        handle = self.kernel.OpenProcess(0x00100000, False, pid)
                        if not handle:
                            import ctypes
                            if ctypes.get_last_error() != 87:
                                raise OSError()
                    self.seen[key] = handle
                except FileNotFoundError:
                    pass
                except (OSError, ValueError, KeyError, TypeError):
                    # Publication can race a read of the JSON file. Try again;
                    # at least one complete identity per launch is required below.
                    pass

    def alive(self, pid, handle):
        if os.name == "nt":
            if not handle:
                return False
            result = self.kernel.WaitForSingleObject(handle, 0)
            if result not in (0, 258):
                raise OSError()
            return result == 258
        try:
            os.kill(pid, 0)
            return True
        except ProcessLookupError:
            return False

    def finish(self, launches):
        self.stop.set()
        self.thread.join(timeout=1)
        if self.thread.is_alive():
            return False
        deadline = time.monotonic() + 25
        proven = False
        try:
            while time.monotonic() < deadline:
                if not any(self.alive(pid, handle) for (pid, _), handle in self.seen.items()):
                    proven = len(self.seen) >= launches
                    break
                time.sleep(0.05)
        except OSError:
            proven = False
        finally:
            if os.name == "nt":
                for handle in self.seen.values():
                    if handle:
                        self.kernel.CloseHandle(handle)
        return proven
