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
from zed_retry_trace import Capture, parse_counts, program, read_ready


def maps(retry, native):
    return ('\n'.join(json.dumps({'type': 'map', 'data': {key: value}})
                      for key, value in (('@retry', retry), ('@native', native))) + '\n').encode()


class TraceTests(unittest.TestCase):
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
        self.assertEqual(parse_counts(maps(1, 1)), (0, 0))
        self.assertEqual(parse_counts(maps(4, 2)), (3, 1))
        for raw in (b'', maps(0, 1), maps(True, 1), maps(1026, 1),
                    maps(1, 1) * 2, b'x' * 4097,
                    b'{"type":"map","data":{"@retry":1}}\n',
                    b'{"type":"printf","data":"private output"}\n'):
            with self.subTest(raw=raw[:40]), self.assertRaises(ValueError):
                parse_counts(raw)

    def test_program_has_only_known_entry_counters(self):
        text = program('/tmp/owned/zed-editor')
        self.assertEqual(text.count('uprobe:'), 2)
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
                    def communicate(self, timeout): return maps(1, 1), None
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
