#!/usr/bin/env python3
"""Synthetic count-only tracing contracts; never attach to a desktop app."""
import io
import json
from pathlib import Path
import subprocess
import struct
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'canary/actions'))
from zed_retry_trace import CLICK_FIELDS, Capture, attach_failure, parse_counts, parse_click_counts, program, read_ready
from zed_render_counts import SYMBOLS as RENDER_SYMBOLS


def maps(retry, native, inputs=1):
    return ('\n'.join(json.dumps({'type': 'map', 'data': {key: value}})
                      for key, value in (('@retry', retry), ('@native', native), ('@input', inputs))) + '\n').encode()


def render_maps():
    return b''.join((json.dumps(dict(type='map', data={f'@render{key}{slot}': 1})) + '\n').encode()
                    for key in RENDER_SYMBOLS for slot in range(4))


def click_maps(started=3, ended=3):
    values = {'@clickStarts': started + 1, '@clickEnds': ended + 1}
    values.update({f'@click{kind}{index}': 3 if kind in ('Input', 'Return') else 1
                   for index in range(1, 4) for kind in CLICK_FIELDS})
    return b''.join((json.dumps({'type': 'map', 'data': {key: value}}) + '\n').encode()
                    for key, value in values.items())


class TraceTests(unittest.TestCase):
    def test_native_geometry_rejection_retains_only_closed_readback_reason(self):
        for raw, reason in (
                (b'{"type":"helper_error","helper":"probe_read_user","retcode":-14,"msg":"PRIVATE"}', 'user-memory-read'),
                (b'{"type":"lost_events","data":{"events":1}}', 'lost-events'),
                (b'{"type":"map","data":{"@geometryRects":{"1,-1,0":1}}}', 'geometry-tuple')):
            with tempfile.TemporaryDirectory() as directory:
                capture = Capture('/unused', directory)
                capture.geometry = True
                capture.receipt['status'] = 'attached'
                class Process:
                    stdout = io.BytesIO()
                    returncode = 0
                    pid = 123
                    finished = False
                    def poll(self): return 0 if self.finished else None
                    def communicate(self, timeout):
                        self.finished = True
                        return raw, b''
                capture.process = Process()
                with patch('zed_retry_trace.subprocess.run'):
                    capture.__exit__()
                public = json.loads((Path(directory) / 'zed-retry-entry-counts.json').read_text())
                self.assertEqual(public['status'], 'unavailable')
                self.assertEqual(public['readbackFailure'], reason)
                self.assertIsNone(public['retryEntries'])
                self.assertNotIn('PRIVATE', str(public))

    def test_geometry_capture_reduces_private_numeric_maps_before_publication(self):
        def bits(value):
            return struct.unpack('<I', struct.pack('<f', value))[0]
        with tempfile.TemporaryDirectory() as directory:
            capture = Capture('/unused', directory)
            capture.geometry = True
            capture.receipt['status'] = 'attached'
            capture.marker_lines = (6, 7)
            capture.markers.mkdir(mode=0o700)
            headers, rectangles = {}, {}
            for slot in range(1, 4):
                target = dict(point=[30, 30], bounds=[10, 20, 40, 20], viewport=[200, 200])
                (capture.markers / f'target-{slot}.json').write_text(json.dumps(target))
                headers[','.join(map(str, [slot, 1, bits(30), bits(30), bits(200), bits(200)]))] = 1
                rectangles[','.join(map(str, [slot, 0, *(bits(n) for n in (10, 20, 40, 20, 0, 0, 200, 200)), 0]))] = 1
            raw = b''.join((json.dumps(dict(type='map', data={name: values})) + '\n').encode()
                           for name, values in (('@geometryHeaders', headers), ('@geometryRects', rectangles)))
            raw += json.dumps(dict(type='helper_error', msg='PRIVATE', helper='probe_read_user_str',
                                   retcode=-14, line=6, col=55)).encode() + b'\n'
            class Process:
                stdout = io.BytesIO()
                returncode = 0
                pid = 123
                def poll(self): return None
                def communicate(self, timeout): return maps(1, 1) + click_maps() + raw + render_maps(), None
            capture.process = Process()
            with patch('zed_retry_trace.subprocess.run'):
                capture.__exit__()
            public = json.loads((Path(directory) / 'zed-retry-entry-counts.json').read_text())
            self.assertEqual(public['hitTestGeometry']['status'], 'complete')
            self.assertEqual(public['markerReadFaults'], 1)
            self.assertNotIn('PRIVATE', str(public))
            self.assertEqual(len(public['hitTestGeometry']['windows']), 3)
            self.assertTrue(all(item['targetWouldBeHovered'] for item in public['hitTestGeometry']['windows']))
            self.assertNotIn('geometryHeaders', str(public))
            self.assertNotIn('viewport', str(public))

    def test_attach_errors_never_export_diagnostic_text(self):
        self.assertEqual(attach_failure(b'PRIVATE: BPF stack limit of 512 bytes exceeded'), 'compiler-stack')
        self.assertEqual(attach_failure(b'PRIVATE: Operation not permitted'), 'permission')
        self.assertEqual(attach_failure(b'PRIVATE: ERROR: unknown compiler rejection'), 'tracer-error')
        self.assertIsNone(attach_failure(b'PRIVATE unknown failure'))

    def test_late_attach_error_is_classified_without_exporting_text(self):
        with tempfile.TemporaryDirectory() as directory:
            capture = Capture('/unused', directory)
            capture.receipt['attachFailure'] = 'readiness-incomplete'
            class Process:
                stdout = io.BytesIO()
                returncode = 1
                def poll(self): return 1
                def communicate(self, timeout):
                    return b'', b'PRIVATE: failed to load program\nPRIVATE ERROR: final rejection\n'
            capture.process = Process()
            capture.__exit__()
            receipt = json.loads((Path(directory) / 'zed-retry-entry-counts.json').read_text())
            self.assertEqual(receipt['status'], 'unavailable')
            self.assertEqual(receipt['attachFailure'], 'program-load')
            self.assertNotIn('PRIVATE', str(receipt))

    def test_unterminated_readiness_error_is_observed(self):
        errors = []
        code = 'import sys;sys.stderr.buffer.write(b"ERROR: synthetic")'
        with subprocess.Popen([sys.executable, '-c', code], stderr=subprocess.PIPE) as child:
            self.assertFalse(read_ready(child.stderr, 2, observe=errors.append))
            child.wait(timeout=2)
        self.assertEqual(errors, [b'ERROR: synthetic'])

    def test_synthetic_verbose_budget_can_reach_notification_after_large_diagnostic(self):
        sizes = []
        code = ('import sys;sys.stderr.buffer.write(b"v"*70000 + '
                'b"\\n__BPFTRACE_NOTIFY_PROBES_ATTACHED\\n")')
        with subprocess.Popen([sys.executable, '-c', code], stderr=subprocess.PIPE) as child:
            self.assertTrue(read_ready(child.stderr, 5, observe=lambda line: sizes.append(len(line)),
                                       max_bytes=100000))
            child.wait(timeout=2)
        self.assertEqual(sizes, [70000])

    def test_activation_intervals_require_complete_bounded_marker_pairs(self):
        total, clicks = parse_click_counts(maps(1, 1, 20) + click_maps())
        self.assertEqual(total, (0, 0, 19))
        self.assertEqual(clicks['started'], 3)
        self.assertEqual([item['inputDispatchEntries'] for item in clicks['windows']], [2, 2, 2])
        for data in (maps(1, 1), maps(1, 1) + click_maps(3, 2),
                     maps(1, 1) + click_maps(4, 4), maps(1, 1) + click_maps() * 2):
            with self.assertRaises(ValueError):
                parse_click_counts(data)
        source = program('/tmp/owned/zed-editor', Path('/tmp/owned/markers'))
        self.assertEqual(source.count('BEGIN {'), 1)
        self.assertEqual(source.count('END {'), 1)
        self.assertIn('sys_enter_openat', source)
        self.assertIn('delete(@slot); delete(@active);', source)
        self.assertNotIn('printf(', source)
        self.assertNotIn('ustack', source)
        self.assertIn('reg("ax") & 255', source)
        self.assertIn('reg("dx") & 255', source)
        self.assertEqual(source.count('uretprobe:'), 6)
        for forbidden in ('arg0', 'arg1', 'buf(', 'printf(', 'retval', 'ustack'):
            self.assertNotIn(forbidden, source)

    def test_return_counters_require_complete_consistent_closed_evidence(self):
        raw = maps(1, 1, 20) + click_maps()
        records = [json.loads(line) for line in raw.splitlines()]
        def replace(key, value):
            return b'\n'.join(json.dumps({'type': 'map', 'data': {
                name: value if name == key else count for name, count in record['data'].items()
            }}).encode() for record in records)
        _, clicks = parse_click_counts(replace('@clickStopped1', 2))
        self.assertEqual(clicks['windows'][0]['inputPropagationStops'], 1)
        for key, value in (('@clickStopped1', 4), ('@clickPrevented1', 4),
                           ('@clickInvalid1', 4), ('@clickHoverTrue1', True),
                           ('@clickHoverFalse1', 65538)):
            with self.subTest(key=key), self.assertRaises(ValueError):
                parse_click_counts(replace(key, value))
        missing = b'\n'.join(json.dumps(record).encode() for record in records
                             if '@clickHoverTrue1' not in record['data'])
        with self.assertRaises(ValueError):
            parse_click_counts(missing)

    def test_gui_identity_is_checked_before_any_privileged_process(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            launcher = root / 'zed.app/bin/zed'
            gui = root / 'zed.app/libexec/zed-editor'
            launcher.parent.mkdir(parents=True)
            gui.parent.mkdir()
            launcher.write_bytes(b'fixture cli')
            gui.write_bytes(b'wrong gui')
            with patch('zed_retry_trace.sys.platform', 'linux'), patch.dict('os.environ', {
                'GITHUB_ACTIONS': 'true', 'RUNNER_ENVIRONMENT': 'github-hosted', 'RUNNER_OS': 'Linux'
            }), patch('zed_retry_trace.subprocess.Popen') as spawn:
                with self.assertRaises(ValueError):
                    Capture(launcher, root).__enter__()
                spawn.assert_not_called()

    def test_zero_requires_both_seeded_maps(self):
        self.assertEqual(parse_counts(maps(1, 1)), (0, 0, 0))
        self.assertEqual(parse_counts(maps(4, 2, 301)), (3, 1, 300))
        self.assertEqual(parse_counts(maps(1, 1, 65537)), (0, 0, 65536))
        for raw in (b'', maps(0, 1), maps(True, 1), maps(1026, 1),
                    maps(1, 1) * 2, maps(1, 1, True), maps(1, 1, 65538), b'x' * 4097,
                    b'{"type":"map","data":{"@retry":1}}\n',
                    b'{"type":"printf","data":"private output"}\n'):
            with self.subTest(raw=raw[:40]), self.assertRaises(ValueError):
                parse_counts(raw)

    def test_program_has_only_known_entry_counters(self):
        text = program('/tmp/owned/zed-editor')
        self.assertEqual(text.count('uprobe:'), 3)
        for forbidden in ('arg0', 'ustack', 'str(', 'system(', 'pid', 'comm'):
            self.assertNotIn(forbidden, text)
        for path in ('relative', '/tmp/name";exit()', '/tmp/a*b', '/tmp/a:b'):
            with self.subTest(path=path), self.assertRaises(ValueError):
                program(path)

    def test_readiness_requires_the_exact_post_attach_notification(self):
        for line, expected in ((b'__BPFTRACE_NOTIFY_PROBES_ATTACHED\n', True),
                               (b'BEGIN\n', False), (b'__BPFTRACE_NOTIFY_PROBES_ATTACHED extra\n', False),
                               (b'private diagnostic\n__BPFTRACE_NOTIFY_PROBES_ATTACHED\n', True)):
            with self.subTest(line=line):
                code = 'import sys;sys.stderr.buffer.write(' + repr(line) + ')'
                with subprocess.Popen([sys.executable, '-c', code], stderr=subprocess.PIPE) as child:
                    self.assertEqual(read_ready(child.stderr, 2), expected)
                    child.wait(timeout=2)

    def test_complete_and_premature_exit_are_distinct(self):
        for alive, expected in ((True, 'complete'), (False, 'unavailable')):
            with tempfile.TemporaryDirectory() as directory:
                capture = Capture('/unused', directory)
                capture.receipt['status'] = 'attached'
                class Process:
                    stdin = io.BytesIO()
                    stdout = io.BytesIO()
                    returncode = 0
                    pid = 123
                    def poll(self): return None if alive else 0
                    def communicate(self, timeout): return maps(1, 1) + click_maps(), None
                capture.process = Process()
                with patch('zed_retry_trace.subprocess.run'):
                    capture.__exit__()
                receipt = json.loads((Path(directory) / 'zed-retry-entry-counts.json').read_text())
                self.assertEqual(receipt['status'], expected)
                self.assertEqual(receipt['retryEntries'], 0 if alive else None)

    def test_timeout_reaps_trace_group_and_never_reports_zero(self):
        with tempfile.TemporaryDirectory() as directory:
            capture = Capture('/unused', directory)
            capture.receipt['status'] = 'attached'
            class Process:
                stdin = io.BytesIO()
                stdout = io.BytesIO()
                pid = 123
                returncode = None
                calls = 0
                def poll(self): return self.returncode
                def communicate(self, timeout):
                    self.calls += 1
                    if self.calls == 1:
                        raise subprocess.TimeoutExpired('fixture', timeout)
                    self.returncode = -9
                    return b'', None
            capture.process = Process()
            with patch('zed_retry_trace.subprocess.run') as kill:
                capture.__exit__()
                self.assertEqual(kill.call_args.args[0][-1], '-123')
            self.assertEqual(capture.receipt['status'], 'unavailable')
            self.assertIsNone(capture.receipt['retryEntries'])
            self.assertEqual(capture.receipt['cleanup'], 'passed')

    def test_unreaped_tracer_fails_the_driver(self):
        with tempfile.TemporaryDirectory() as directory:
            capture = Capture('/unused', directory)
            class Process:
                pid = 123
                stdout = io.BytesIO()
                def poll(self): return None
            capture.process = Process()
            with patch('zed_retry_trace.subprocess.run', side_effect=OSError('synthetic')):
                with self.assertRaises(RuntimeError):
                    capture.__exit__()
            receipt = json.loads((Path(directory) / 'zed-retry-entry-counts.json').read_text())
            self.assertEqual(receipt['cleanup'], 'failed')
            self.assertIsNone(receipt['retryEntries'])


if __name__ == '__main__':
    unittest.main()
