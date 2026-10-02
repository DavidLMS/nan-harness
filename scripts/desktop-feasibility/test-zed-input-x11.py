#!/usr/bin/env python3
"""The native key helper accepts fixed actions only and never echoes content."""
import io
import json
import runpy
from pathlib import Path
import subprocess
import sys
import unittest
from unittest.mock import patch

module = runpy.run_path(str(Path(__file__).with_name('zed-input-x11.py')))


class Transport(unittest.TestCase):
    def call(self, mode, payload=b''):
        stdin = io.TextIOWrapper(io.BytesIO(payload))
        with patch.object(sys, 'argv', ['helper', mode]), patch.object(sys, 'stdin', stdin):
            return module['main']()

    def test_foreign_keys_or_payload_cannot_send_input(self):
        with patch('subprocess.run') as run:
            self.assertEqual(self.call('arbitrary-command'), 2)
            self.assertEqual(self.call('submit', b'private synthetic prompt'), 2)
            run.assert_not_called()

    def test_fixed_actions_are_bounded_and_silent(self):
        with patch('subprocess.run', return_value=subprocess.CompletedProcess([], 0)) as run:
            for mode, key in module['KEYS'].items():
                self.assertEqual(self.call(mode), 0)
                args, kwargs = run.call_args
                self.assertEqual(args[0], ['/usr/bin/xdotool', 'key', '--clearmodifiers', key])
                self.assertEqual(kwargs['timeout'], 2)
                self.assertEqual(kwargs['stdout'], subprocess.DEVNULL)
                self.assertEqual(kwargs['stderr'], subprocess.DEVNULL)
        with patch('subprocess.run', side_effect=subprocess.TimeoutExpired('fixed-helper', 2)):
            self.assertEqual(self.call('submit'), 3)

    def test_pointer_checks_foreground_before_one_activation(self):
        request = json.dumps(dict(pid=20, window=40, x=100, y=200)).encode()
        calls = []
        def execute(args, **kwargs):
            calls.append(args)
            self.assertLessEqual(kwargs['timeout'], 2)
            self.assertEqual(kwargs['stderr'], subprocess.DEVNULL)
            output = b'40\n' if args[1] == 'getactivewindow' else b'20\n'
            return subprocess.CompletedProcess(args, 0, stdout=output)
        with patch('subprocess.run', side_effect=execute):
            self.assertEqual(self.call('retry-click', request), 0)
        self.assertEqual([args[1] for args in calls],
                         ['getactivewindow', 'getwindowpid', 'mousemove',
                          'getactivewindow', 'getwindowpid', 'click'])
        self.assertEqual(calls[-1], ['/usr/bin/xdotool', 'click', '--clearmodifiers', '1'])
        with patch('subprocess.run', return_value=subprocess.CompletedProcess([], 0, stdout=b'99')) as run:
            self.assertEqual(self.call('retry-click', request), 11)
            self.assertEqual(run.call_count, 1)
        with patch('subprocess.run', side_effect=[
                subprocess.CompletedProcess([], 0, stdout=b'40'),
                subprocess.CompletedProcess([], 0, stdout=b'20'),
                subprocess.CompletedProcess([], 0),
                subprocess.CompletedProcess([], 0, stdout=b'99')]) as run:
            self.assertEqual(self.call('retry-click', request), 11)
            self.assertEqual(run.call_count, 4)

    def test_pointer_failure_stage_never_replays_input(self):
        request = json.dumps(dict(pid=20, window=40, x=100, y=200)).encode()
        for failed, code in [('getactivewindow', 13), ('mousemove', 14), ('click', 16)]:
            calls = []
            def execute(args, **kwargs):
                calls.append(args[1])
                if args[1] == failed:
                    raise subprocess.TimeoutExpired('fixed-helper', 2)
                output = b'40' if args[1] == 'getactivewindow' else b'20'
                return subprocess.CompletedProcess(args, 0, stdout=output)
            with patch('subprocess.run', side_effect=execute):
                self.assertEqual(self.call('retry-click', request), code)
            self.assertLessEqual(calls.count('click'), 1)
            self.assertEqual(calls[-1], failed)

    def test_invalid_pointer_data_never_reaches_native_input(self):
        request = dict(pid=20, window=40, x=100, y=200)
        with patch('subprocess.run') as run:
            for changed in ({**request, 'pid': True}, {**request, 'window': 0},
                            {**request, 'x': 32768}, {**request, 'command': 'PRIVATE'}):
                self.assertNotEqual(self.call('retry-click', json.dumps(changed).encode()), 0)
            self.assertNotEqual(self.call('retry-click', b'x' * 4097), 0)
            run.assert_not_called()


if __name__ == '__main__':
    unittest.main()
