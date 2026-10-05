"""Count-only uprobes for the pinned official Zed binary on hosted Linux."""
import hashlib
import json
import os
from pathlib import Path
import re
import socket
import struct
import subprocess
import sys
import tempfile

from cell import write_json

BINARY_SHA256 = '18f225903713f623e1564a2e1902ef4ba84d2b5fb6aa58fd59af2d62646943b2'
SYMBOLS = (
    '_RNvMs8_NtNtCs49dSLSzPpau_8agent_ui17conversation_view11thread_viewNtB5_10ThreadView16retry_generation',
    '_RNvXsd_Cs5P68OkLbxMe_5agentNtB5_23NativeAgentSessionRetryNtNtCs2R6xX2UfTue_10acp_thread10connection17AgentSessionRetry3run',
)


def program(executable):
    path = str(executable)
    if not re.fullmatch(r'/[A-Za-z0-9_./-]+', path):
        raise ValueError('unsupported executable path')
    # Seed both count maps so zero events is distinguishable from missing output.
    return '\n'.join([
        'BEGIN { @retry = count(); @native = count(); }',
        f'uprobe:{path}:{SYMBOLS[0]} {{ @retry = count(); }}',
        f'uprobe:{path}:{SYMBOLS[1]} {{ @native = count(); }}',
        'interval:s:1800 { exit(); }',
        'END { print(@retry); print(@native); clear(@retry); clear(@native); }',
    ])


def parse_counts(data):
    if len(data) > 4096:
        raise ValueError('trace output limit')
    counts = {}
    for line in data.splitlines():
        if not line.strip():
            continue
        item = json.loads(line)
        if (type(item) is not dict or item.get('type') != 'map'
                or type(item.get('data')) is not dict or len(item['data']) != 1):
            raise ValueError('unexpected trace output')
        key, value = next(iter(item['data'].items()))
        if key not in ('@retry', '@native') or key in counts or type(value) is not int or not 1 <= value <= 1025:
            raise ValueError('invalid trace count')
        counts[key] = value - 1
    if set(counts) != {'@retry', '@native'}:
        raise ValueError('incomplete trace output')
    return counts['@retry'], counts['@native']


def ready_notification(data, credentials):
    # sd_notify runs after attachment; BEGIN runs before it and is insufficient.
    return data.strip() == b'READY=1' and len(credentials) == 12 and struct.unpack('3i', credentials)[1] == 0


class Capture:
    """Trace the owned executable inode, without launching the desktop app as root."""
    def __init__(self, executable, facts):
        self.launcher = Path(executable)
        self.facts = Path(facts)
        self.process = None
        self.receipt = dict(schemaVersion=1, mechanism='zed-retry-entry-counts',
                            diagnosticsOnly=True, status='unavailable', stage='attach', cleanup='passed',
                            retryEntries=None, nativeRetryEntries=None)

    def __enter__(self):
        if (sys.platform != 'linux' or os.environ.get('GITHUB_ACTIONS') != 'true'
                or os.environ.get('RUNNER_ENVIRONMENT') != 'github-hosted'
                or os.environ.get('RUNNER_OS') != 'Linux'):
            raise ValueError('trace requires hosted Linux')
        if (self.launcher.name != 'zed' or self.launcher.parent.name != 'bin'
                or self.launcher.parent.parent.name != 'zed.app'
                or self.launcher.is_symlink() or self.launcher.resolve() != self.launcher):
            raise ValueError('trace launcher identity')
        # The prepared entry is the official CLI; callbacks live in its sibling GUI.
        self.executable = self.launcher.parent.parent / 'libexec' / 'zed-editor'
        if (self.executable.is_symlink() or self.executable.resolve() != self.executable
                or not self.executable.is_file()):
            raise ValueError('trace executable identity')
        with self.executable.open('rb') as source:
            if hashlib.file_digest(source, 'sha256').hexdigest() != BINARY_SHA256:
                raise ValueError('trace executable differs from inspected binary')
        try:
            # A short private pathname stays within AF_UNIX's 108-byte limit.
            with tempfile.TemporaryDirectory(prefix='nanh-trace-', dir='/tmp') as directory:
                with socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM) as notify:
                    notify.setsockopt(socket.SOL_SOCKET, socket.SO_PASSCRED, 1)
                    notify.settimeout(15)
                    address = str(Path(directory) / 'ready')
                    notify.bind(address)
                    self.process = subprocess.Popen(
                        ['/usr/bin/sudo', '-n', '/usr/bin/env', 'NOTIFY_SOCKET=' + address,
                         'BPFTRACE_MISSING_PROBES=error', '/usr/bin/bpftrace', '-q', '-B', 'none',
                         '-f', 'json', '-e', program(self.executable)],
                        stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                        bufsize=0, start_new_session=True, env={'PATH': '/usr/bin:/bin', 'LANG': 'C'})
                    data, ancillary, _, _ = notify.recvmsg(128, socket.CMSG_SPACE(12))
                    credentials = [data for level, kind, data in ancillary
                                   if level == socket.SOL_SOCKET and kind == socket.SCM_CREDENTIALS]
                    if (len(credentials) == 1 and ready_notification(data, credentials[0])
                            and self.process.poll() is None):
                        self.receipt['status'] = 'attached'
        except (OSError, ValueError, subprocess.SubprocessError):
            pass
        return self

    def __exit__(self, *_):
        process = self.process
        try:
            if process is not None:
                alive = process.poll() is None
                if self.receipt['status'] == 'attached':
                    self.receipt['stage'] = 'stop'
                if alive:
                    subprocess.run(['/usr/bin/sudo', '-n', '/bin/kill', '-INT', '--',
                                    str(-process.pid)], timeout=3, check=True,
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                try:
                    # Only two maps can be printed; no probe prints per-event data.
                    output, _ = process.communicate(timeout=5)
                except subprocess.TimeoutExpired:
                    self.receipt['status'] = 'unavailable'
                    subprocess.run(['/usr/bin/sudo', '-n', '/bin/kill', '-KILL', '--',
                                    str(-process.pid)], timeout=3, check=True,
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                    output, _ = process.communicate(timeout=3)
                if self.receipt['status'] == 'attached' and alive and process.returncode == 0:
                    self.receipt['stage'] = 'readback'
                    retry, native = parse_counts(output)
                    self.receipt.update(status='complete', stage='complete', retryEntries=retry, nativeRetryEntries=native)
                else:
                    self.receipt['status'] = 'unavailable'
        except (OSError, ValueError, subprocess.SubprocessError):
            self.receipt['status'] = 'unavailable'
            if process is not None and process.poll() is None:
                self.receipt['cleanup'] = 'failed'
        finally:
            if process is not None and process.stdout is not None:
                process.stdout.close()
            write_json(self.facts / 'zed-retry-entry-counts.json', self.receipt)
        if self.receipt['cleanup'] != 'passed':
            raise RuntimeError('trace cleanup failed')
