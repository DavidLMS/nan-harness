"""Bounded terminal transport for the real TUI probe, including Windows ConPTY."""

import os
import select
import subprocess
import time


class Terminal:
    def __init__(self, command, workspace, env):
        self.windows = os.name == "nt"
        if self.windows:
            from winpty import PtyProcess

            self.process = PtyProcess.spawn(
                command, cwd=str(workspace), env=env, dimensions=(40, 120)
            )
            self.handle = self.process.fileobj
        else:
            import fcntl
            import pty
            import struct
            import termios

            self.handle, slave = pty.openpty()
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))
            try:
                self.process = subprocess.Popen(
                    command,
                    cwd=workspace,
                    env=env,
                    stdin=slave,
                    stdout=slave,
                    stderr=slave,
                    start_new_session=True,
                )
            finally:
                os.close(slave)

    def alive(self):
        return self.process.isalive() if self.windows else self.process.poll() is None

    def read(self):
        ready, _, _ = select.select([self.handle], [], [], 0.1)
        if not ready:
            return b""
        return (
            self.process.read(65536).encode("utf8") if self.windows else os.read(self.handle, 65536)
        )

    def write(self, data):
        if self.windows:
            self.process.write(data.decode("utf8"))
        else:
            os.write(self.handle, data)

    def wait_exit(self, timeout=10):
        deadline = time.monotonic() + timeout
        while self.alive() and time.monotonic() < deadline:
            self.read()
        assert not self.alive(), "TUI did not exit after Ctrl-C"

    def close(self):
        if self.windows:
            self.process.close(force=True)
        else:
            if self.alive():
                self.process.kill()
                self.process.wait(timeout=10)
            os.close(self.handle)
