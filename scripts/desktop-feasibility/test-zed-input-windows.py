#!/usr/bin/env python3
"""Verify fixed batches and refusal boundaries without sending native input."""
import io
from pathlib import Path
import runpy
import sys
import unittest
from unittest.mock import patch

module = runpy.run_path(str(Path(__file__).with_name('zed-input-windows.py')))


class Transport(unittest.TestCase):
    def test_batches_release_their_fixed_keys_in_reverse_order(self):
        for mode, (modifiers, key) in module['KEYS'].items():
            batch = module['events'](mode)
            down = [code for code, up in batch if not up]
            up = [code for code, up in batch if up]
            self.assertEqual(down, modifiers + [key])
            self.assertEqual(up, list(reversed(down)))
        self.assertEqual(module['events']('submit'), [(13, False), (13, True)])

    def test_foreign_actions_or_payload_do_not_call_native_input(self):
        for mode, payload in [('arbitrary-command', b''), ('submit', b'private synthetic')]:
            with patch.object(sys, 'argv', ['helper', mode]), \
                 patch.object(sys, 'stdin', io.TextIOWrapper(io.BytesIO(payload))), \
                 patch('ctypes.WinDLL', create=True) as native:
                self.assertEqual(module['main'](), 2)
                native.assert_not_called()


if __name__ == '__main__':
    unittest.main()
