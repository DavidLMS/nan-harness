#!/usr/bin/env python3
"""Synthetic count-only tracing contracts; never attach to a desktop app."""
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'canary/actions'))
from zed_retry_trace import Capture, attach_failure, parse_counts, parse_click_counts, program, read_ready


def maps(retry, native, inputs=1):
    return ('\n'.join(json.dumps({'type': 'map', 'data': {key: value}})
                      for key, value in (('@retry', retry), ('@native', native), ('@input', inputs))) + '\n').encode()


def click_maps(started=3, ended=3):
    values = {'@clickStarts': started + 1, '@clickEnds': ended + 1}
    values.update({f'@click{kind}{index}': 3 if kind == 'Input' else 1
                   for index in range(1, 4) for kind in ('Input', 'Retry', 'Native', 'Clear')})
    return b''.join((json.dumps({'type': 'map', 'data': {key: value}}) + '\n').encode()
                    for key, value in values.items())


class TraceTests(unittest.TestCase):
    def test_attach_errors_never_export_diagnostic_text(self):
        self.assertEqual(attach_failure(b'PRIVATE: BPF stack limit of 512 bytes exceeded'), 'compiler-stack')
        self.assertEqual(attach_failure(b'PRIVATE: Operation not permitted'), 'permission')
        self.assertIsNone(attach_failure(b'PRIVATE unknown failure'))

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
