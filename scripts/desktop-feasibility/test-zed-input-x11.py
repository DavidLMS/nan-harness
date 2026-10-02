#!/usr/bin/env python3
"""The native key helper accepts fixed actions only and never echoes content."""
import io
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


if __name__ == '__main__':
    unittest.main()
