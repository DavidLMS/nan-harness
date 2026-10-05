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
import zed_hit_geometry
import zed_render_counts

BINARY_SHA256 = '18f225903713f623e1564a2e1902ef4ba84d2b5fb6aa58fd59af2d62646943b2'
SYMBOLS = (
    '_RNvMs8_NtNtCs49dSLSzPpau_8agent_ui17conversation_view11thread_viewNtB5_10ThreadView16retry_generation',
    '_RNvXsd_Cs5P68OkLbxMe_5agentNtB5_23NativeAgentSessionRetryNtNtCs2R6xX2UfTue_10acp_thread10connection17AgentSessionRetry3run',
    '_RNvMst_NtCs1R78ycJoit4_4gpui6windowNtB5_6Window14dispatch_event',
)

CLEAR_SYMBOL = '_RNvMs8_NtNtCs49dSLSzPpau_8agent_ui17conversation_view11thread_viewNtB5_10ThreadView18clear_thread_error'
HOVER_SYMBOL = '_RNvMsj_NtCs1R78ycJoit4_4gpui6windowNtB5_6Hitbox10is_hovered'
# Exact instructions in BINARY_SHA256, verified by static disassembly. The
# dispatch epilogue returns propagate in AL and default_prevented in DL;
# Hitbox::is_hovered returns its boolean in AL. Observe function returns rather
# than requiring the runner's bpftrace to support instruction-offset probes.
# Never read arguments, hitbox identities, coordinates or application memory.
CLICK_FIELDS = {
    'Input': 'inputDispatchEntries', 'Retry': 'retryEntries',
    'Native': 'nativeRetryEntries', 'Clear': 'errorClearEntries',
    'Return': 'inputDispatchReturns', 'Stopped': 'inputPropagationStops',
    'Prevented': 'inputDefaultPreventions', 'Invalid': 'inputInvalidReturns',
    'HoverTrue': 'hoverTrueReturns', 'HoverFalse': 'hoverFalseReturns',
    'HoverInvalid': 'hoverInvalidReturns',
}

READBACK_FAILURES = {
    'geometry output budget': 'geometry-output-budget',
    'unexpected geometry output': 'geometry-output-shape',
    'duplicate geometry map': 'geometry-map-duplicate',
    'geometry map budget': 'geometry-map-budget',
    'invalid geometry tuple': 'geometry-tuple',
    'invalid geometry fields': 'geometry-fields',
    'geometry user read fault': 'user-memory-read',
    'geometry marker read fault': 'marker-read',
    'geometry marker fault budget': 'marker-read-budget',
    'geometry helper failure': 'helper-error',
    'geometry lost events': 'lost-events',
    'trace output limit': 'counter-output-budget',
    'unexpected trace output': 'counter-output-shape',
    'invalid activation counter': 'activation-counter',
    'incomplete activation counters': 'activation-incomplete',
    'inconsistent dispatch returns': 'dispatch-returns',
    'invalid trace count': 'counter-value',
    'incomplete trace output': 'counter-incomplete',
}


def program(executable, markers=None, geometry=False):
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
        if not all(len(value.encode()) < 128 and re.fullmatch(r'/[A-Za-z0-9_./-]+', value) for value in (start, end)):
            raise ValueError('unsupported marker path')
        seeds = ' '.join(f'@click{kind}{index} = count();'
                        for index in range(1, 4) for kind in CLICK_FIELDS)
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
            lines.append(f'uretprobe:{path}:{SYMBOLS[2]} '
                         f'/@active == 1 && @slot == {index}/ {{ '
                         '$propagate = reg("ax") & 255; $prevented = reg("dx") & 255; '
                         f'@clickReturn{index} = count(); '
                         f'if ($propagate > 1 || $prevented > 1) {{ @clickInvalid{index} = count(); }} '
                         'else { '
                         f'if ($propagate == 0) {{ @clickStopped{index} = count(); }} '
                         f'if ($prevented == 1) {{ @clickPrevented{index} = count(); }} '
                         '} }')
            lines.append(f'uretprobe:{path}:{HOVER_SYMBOL} /@active == 1 && @slot == {index}/ '
                         '{ $hover = reg("ax") & 255; '
                         f'if ($hover == 0) {{ @clickHoverFalse{index} = count(); }} '
                         f'else if ($hover == 1) {{ @clickHoverTrue{index} = count(); }} '
                         f'else {{ @clickHoverInvalid{index} = count(); }} }}')
    if geometry:
        if markers is None:
            raise ValueError('geometry requires activation markers')
        lines[0] = lines[0][:-1] + zed_hit_geometry.seeds() + ' }'
        lines.append(zed_hit_geometry.probe(path, SYMBOLS[2]))
        lines[0] = lines[0][:-1] + zed_render_counts.seeds() + ' }'
        lines.append(zed_render_counts.probes(path))
        lines = [line.replace('delete(@active);', 'delete(@active); ' + zed_hit_geometry.cleanup()) for line in lines]
    return '\n'.join(lines)


def parse_click_counts(data):
    """Separate bounded per-activation counters from the original maps."""
    if len(data) > 8192:
        raise ValueError('trace output limit')
    counters, ordinary = {}, []
    expected = {'@clickStarts', '@clickEnds'} | {
        f'@click{kind}{index}' for index in range(1, 4) for kind in CLICK_FIELDS}
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
        limit = 1025 if any(key.startswith('@click' + kind) for kind in ('Retry', 'Native', 'Clear')) else 65537
        if key in counters or type(value) is not int or not 1 <= value <= limit:
            raise ValueError('invalid activation counter')
        counters[key] = value - 1
    if set(counters) != expected or not 0 <= counters['@clickEnds'] == counters['@clickStarts'] <= 3:
        raise ValueError('incomplete activation counters')
    clicks = [{field: counters[f'@click{kind}{i}'] for kind, field in CLICK_FIELDS.items()}
              for i in range(1, 4)]
    for click in clicks:
        if any(click[key] > click['inputDispatchReturns'] for key in (
                'inputPropagationStops', 'inputDefaultPreventions', 'inputInvalidReturns')):
            raise ValueError('inconsistent dispatch returns')
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


def attach_failure(line):
    """Discard tracer text; retain only a fixed compiler/attachment category."""
    line = line.lower()
    for phrase, category in ((b'stack limit', 'compiler-stack'),
                             (b'syntax error', 'compiler-syntax'),
                             (b'tracepoint not found', 'tracepoint-unavailable'),
                             (b'permission denied', 'permission'),
                             (b'operation not permitted', 'permission'),
                             (b'failed to load program', 'program-load'),
                             (b'could not resolve symbol', 'symbol-unavailable'),
                             (b'error:', 'tracer-error')):
        if phrase in line:
            return category
    return None


def read_ready(stream, timeout=15, observe=None, max_bytes=65536):
    """Pinned 0.20.2 emits this test notification after all probes attach."""
    marker = b'__BPFTRACE_NOTIFY_PROBES_ATTACHED'
    deadline = time.monotonic() + timeout
    line = bytearray()
    with selectors.DefaultSelector() as selector:
        selector.register(stream, selectors.EVENT_READ)
        for _ in range(max_bytes):
            remaining = deadline - time.monotonic()
            if remaining <= 0 or not selector.select(remaining):
                return False
            byte = os.read(stream.fileno(), 1)
            if not byte:
                if line and observe is not None:
                    observe(bytes(line))
                return False
            if byte == b'\n':
                if line == marker:
                    return True
                if observe is not None:
                    observe(bytes(line))
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
        self.marker_lines = ()
        self.geometry = os.environ.get('NANH_ZED_HIT_GEOMETRY') == '1'
        self.markers = self.facts / 'retry-trace-markers'
        self.receipt = dict(schemaVersion=1, mechanism='zed-retry-entry-counts',
                            diagnosticsOnly=True, status='unavailable', stage='attach', cleanup='passed',
                            retryEntries=None, nativeRetryEntries=None, inputDispatchEntries=None,
                            attachFailure='unclassified')

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
                self.receipt['attachFailure'] = 'version-mismatch'
                return self
            source = program(self.executable, self.markers, self.geometry)
            self.marker_lines = tuple(index for index, line in enumerate(source.splitlines(), 1)
                if line.startswith('tracepoint:syscalls:sys_enter_openat /str(args->filename)'))
            self.process = subprocess.Popen(
                ['/usr/bin/sudo', '-n', '/usr/bin/env', '__BPFTRACE_NOTIFY_PROBES_ATTACHED=1', 'BPFTRACE_STRLEN=128',
                 '/usr/bin/bpftrace', '-q', *(['-kk'] if self.geometry else []), '-B', 'none', '-f', 'json',
                 '-e', source],
                stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                bufsize=0, start_new_session=True, env={'PATH': '/usr/bin:/bin', 'LANG': 'C'})
            def observe(line):
                category = attach_failure(line)
                if category is not None and (category != 'tracer-error'
                                             or self.receipt['attachFailure'] == 'unclassified'):
                    self.receipt['attachFailure'] = category
            if (read_ready(self.process.stderr, timeout=45 if self.geometry else 15, observe=observe)
                    and self.process.poll() is None):
                self.receipt['status'] = 'attached'
                self.receipt['attachFailure'] = None
            elif self.receipt['attachFailure'] == 'unclassified':
                self.receipt['attachFailure'] = ('readiness-incomplete' if self.process.poll() is None
                                                 else 'tracer-exited')
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
                    # Numeric geometry maps, when enabled, remain in this private
                    # pipe and are reduced before publishing any receipt.
                    output, errors = process.communicate(timeout=5)
                except subprocess.TimeoutExpired:
                    self.receipt['status'] = 'unavailable'
                    subprocess.run(['/usr/bin/sudo', '-n', '/bin/kill', '-KILL', '--',
                                    str(-process.pid)], timeout=3, check=True,
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                    output, errors = process.communicate(timeout=3)
                # Compilation can finish after the readiness observation ended.
                # Retain only closed categories from the remaining private pipe.
                if self.receipt['stage'] == 'attach':
                    for line in (errors or b'').splitlines():
                        category = attach_failure(line)
                        if category is not None and (category != 'tracer-error' or self.receipt['attachFailure'] in {
                                'unclassified', 'readiness-incomplete', 'tracer-exited'}):
                            self.receipt['attachFailure'] = category
                if self.receipt['status'] == 'attached' and alive and process.returncode == 0:
                    self.receipt['stage'] = 'readback'
                    geometry_maps = None
                    if self.geometry:
                        output, ignored = zed_hit_geometry.strip_marker_faults(output, self.marker_lines)
                        self.receipt['markerReadFaults'] = ignored
                        output, geometry_maps = zed_hit_geometry.split_maps(output)
                        output, renders = zed_render_counts.split_counts(output)
                    (retry, native, inputs), clicks = parse_click_counts(output)
                    self.receipt.update(status='complete', stage='complete', retryEntries=retry,
                                        nativeRetryEntries=native, inputDispatchEntries=inputs,
                                        activationWindows=clicks)
                    if geometry_maps is not None:
                        self.receipt['overlayRenderEntries'] = dict(totals=renders['totals'],
                            windows=renders['windows'][:clicks['started']])
                        try:
                            windows = zed_hit_geometry.observations(geometry_maps, self.markers, clicks['started'])
                            self.receipt['hitTestGeometry'] = dict(status='complete', windows=windows)
                        except (OSError, ValueError):
                            self.receipt['hitTestGeometry'] = dict(status='unavailable', windows=[])
                else:
                    self.receipt['status'] = 'unavailable'
        except (OSError, ValueError, subprocess.SubprocessError) as error:
            self.receipt['status'] = 'unavailable'
            if self.receipt['stage'] == 'readback':
                self.receipt['readbackFailure'] = READBACK_FAILURES.get(str(error), 'rejected')
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
