"""Count-only uprobes for the pinned official Zed binary on hosted Linux."""
import hashlib
import json
import os
from pathlib import Path
import re
import selectors
import subprocess
import sys
import time

from cell import write_json

BINARY_SHA256 = '18f225903713f623e1564a2e1902ef4ba84d2b5fb6aa58fd59af2d62646943b2'
SYMBOLS = (
    '_RNvMs8_NtNtCs49dSLSzPpau_8agent_ui17conversation_view11thread_viewNtB5_10ThreadView16retry_generation',
    '_RNvXsd_Cs5P68OkLbxMe_5agentNtB5_23NativeAgentSessionRetryNtNtCs2R6xX2UfTue_10acp_thread10connection17AgentSessionRetry3run',
    '_RNvMst_NtCs1R78ycJoit4_4gpui6windowNtB5_6Window14dispatch_event',
)

CLEAR_SYMBOL = '_RNvMs8_NtNtCs49dSLSzPpau_8agent_ui17conversation_view11thread_viewNtB5_10ThreadView18clear_thread_error'


def program(executable, markers=None):
    path = str(executable)
    if not re.fullmatch(r'/[A-Za-z0-9_./-]+', path):
        raise ValueError('unsupported executable path')
    # Seed every count map so zero events is distinguishable from missing output.
    # Let bpftrace print once after detaching. In 0.20.2 scalar count maps are
    # per-CPU arrays: clear() zeroes them, so an END print/clear also produces
    # a second pair of zero-valued maps during the automatic final print.
    lines = [
        'BEGIN { @retry = count(); @native = count(); @input = count(); }',
        f'uprobe:{path}:{SYMBOLS[0]} {{ @retry = count(); }}',
        f'uprobe:{path}:{SYMBOLS[1]} {{ @native = count(); }}',
        f'uprobe:{path}:{SYMBOLS[2]} {{ @input = count(); }}',
        'interval:s:1800 { exit(); }',
    ]
    if markers is not None:
        start, end = (str(markers / name) for name in ('start', 'end'))
        if not all(len(value.encode()) < 256 and re.fullmatch(r'/[A-Za-z0-9_./-]+', value) for value in (start, end)):
            raise ValueError('unsupported marker path')
        seeds = ' '.join(f'@click{kind}{index} = count();'
                        for index in range(1, 4) for kind in ('Input', 'Retry', 'Native', 'Clear'))
        lines[0] = (lines[0][:-1] + '@slot = 0; @active = 0; '
                    '@clickStarts = count(); @clickEnds = count(); ' + seeds + ' }')
        lines.extend([
            f'tracepoint:syscalls:sys_enter_openat /str(args->filename) == "{start}"/ '
            '{ @slot = @slot + 1; @active = 1; @clickStarts = count(); }',
            f'tracepoint:syscalls:sys_enter_openat /str(args->filename) == "{end}"/ '
            '{ @active = 0; @clickEnds = count(); }',
            # Plain scalar hash entries can be deleted; count maps remain for
            # the single automatic final print, as in the campaign counters.
            'END { delete(@slot); delete(@active); }',
        ])
        for index in range(1, 4):
            for kind, symbol in zip(('Retry', 'Native', 'Input', 'Clear'), (*SYMBOLS, CLEAR_SYMBOL)):
                lines.append(f'uprobe:{path}:{symbol} /@active == 1 && @slot == {index}/ '
                             f'{{ @click{kind}{index} = count(); }}')
    return '\n'.join(lines)


def parse_click_counts(data):
    """Separate bounded per-activation counters from the original maps."""
    if len(data) > 8192:
        raise ValueError('trace output limit')
    counters, ordinary = {}, []
    expected = {'@clickStarts', '@clickEnds'} | {
        f'@click{kind}{index}' for index in range(1, 4) for kind in ('Input', 'Retry', 'Native', 'Clear')}
    for line in data.splitlines():
        if not line.strip():
            continue
        item = json.loads(line)
        if (type(item) is not dict or item.get('type') != 'map'
                or type(item.get('data')) is not dict or len(item['data']) != 1):
            raise ValueError('unexpected trace output')
        key, value = next(iter(item['data'].items()))
        if key not in expected:
            ordinary.append(line)
            continue
        limit = 65537 if key.startswith('@clickInput') else 1025
        if key in counters or type(value) is not int or not 1 <= value <= limit:
            raise ValueError('invalid activation counter')
        counters[key] = value - 1
    if set(counters) != expected or not 0 <= counters['@clickEnds'] == counters['@clickStarts'] <= 3:
        raise ValueError('incomplete activation counters')
    clicks = [dict(inputDispatchEntries=counters[f'@clickInput{i}'],
                   retryEntries=counters[f'@clickRetry{i}'], nativeRetryEntries=counters[f'@clickNative{i}'], errorClearEntries=counters[f'@clickClear{i}'])
              for i in range(1, 4)]
    return parse_counts(b'\n'.join(ordinary)), dict(
        started=counters['@clickStarts'], ended=counters['@clickEnds'], windows=clicks)


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
        limit = 65537 if key == '@input' else 1025
        if key not in ('@retry', '@native', '@input') or key in counts or type(value) is not int or not 1 <= value <= limit:
            raise ValueError('invalid trace count')
        counts[key] = value - 1
    if set(counts) != {'@retry', '@native', '@input'}:
        raise ValueError('incomplete trace output')
    return counts['@retry'], counts['@native'], counts['@input']


def read_ready(stream, timeout=15):
    """Pinned 0.20.2 emits this test notification after all probes attach."""
    marker = b'__BPFTRACE_NOTIFY_PROBES_ATTACHED'
    deadline = time.monotonic() + timeout
    line = bytearray()
    with selectors.DefaultSelector() as selector:
        selector.register(stream, selectors.EVENT_READ)
        for _ in range(65536):
            remaining = deadline - time.monotonic()
            if remaining <= 0 or not selector.select(remaining):
                return False
            byte = os.read(stream.fileno(), 1)
            if not byte:
                return False
            if byte == b'\n':
                if line == marker:
                    return True
                line.clear()
            else:
                line.extend(byte)
    return False


class Capture:
    """Trace the owned executable inode, without launching the desktop app as root."""
    def __init__(self, executable, facts):
        self.launcher = Path(executable)
        self.facts = Path(facts)
        self.process = None
        self.markers = self.facts / 'retry-trace-markers'
        self.receipt = dict(schemaVersion=1, mechanism='zed-retry-entry-counts',
                            diagnosticsOnly=True, status='unavailable', stage='attach', cleanup='passed',
                            retryEntries=None, nativeRetryEntries=None, inputDispatchEntries=None)

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
        self.markers.mkdir(mode=0o700)
        for name in ('start', 'end'):
            descriptor = os.open(self.markers / name, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
            os.close(descriptor)
        try:
            version = subprocess.run(['/usr/bin/bpftrace', '--version'], timeout=3,
                stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, check=True).stdout.strip()
            if version != b'bpftrace v0.20.2':
                return self
            self.process = subprocess.Popen(
                ['/usr/bin/sudo', '-n', '/usr/bin/env', '__BPFTRACE_NOTIFY_PROBES_ATTACHED=1', 'BPFTRACE_STRLEN=256',
                 '/usr/bin/bpftrace', '-q', '-B', 'none', '-f', 'json', '-e', program(self.executable, self.markers)],
                stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                bufsize=0, start_new_session=True, env={'PATH': '/usr/bin:/bin', 'LANG': 'C'})
            if read_ready(self.process.stderr) and self.process.poll() is None:
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
                    # Only fixed counter maps are printed, never per-event data.
                    output, _ = process.communicate(timeout=5)
                except subprocess.TimeoutExpired:
                    self.receipt['status'] = 'unavailable'
                    subprocess.run(['/usr/bin/sudo', '-n', '/bin/kill', '-KILL', '--',
                                    str(-process.pid)], timeout=3, check=True,
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                    output, _ = process.communicate(timeout=3)
                if self.receipt['status'] == 'attached' and alive and process.returncode == 0:
                    self.receipt['stage'] = 'readback'
                    (retry, native, inputs), clicks = parse_click_counts(output)
                    self.receipt.update(status='complete', stage='complete', retryEntries=retry,
                                        nativeRetryEntries=native, inputDispatchEntries=inputs,
                                        activationWindows=clicks)
                else:
                    self.receipt['status'] = 'unavailable'
        except (OSError, ValueError, subprocess.SubprocessError):
            self.receipt['status'] = 'unavailable'
            if process is not None and process.poll() is None:
                self.receipt['cleanup'] = 'failed'
        finally:
            if process is not None:
                for stream in (process.stdout, getattr(process, 'stderr', None)):
                    if stream is not None:
                        stream.close()
            write_json(self.facts / 'zed-retry-entry-counts.json', self.receipt)
        if self.receipt['cleanup'] != 'passed':
            raise RuntimeError('trace cleanup failed')
