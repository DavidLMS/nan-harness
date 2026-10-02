#!/usr/bin/env python3
"""Exercise delegation and owned-child cleanup on observer startup failure."""
import io
import tempfile
import os
from pathlib import Path
import runpy
import sys
import unittest
from unittest.mock import Mock, patch

SHIM = Path(__file__).with_name('nanh-shim.py')


class ShimLifecycle(unittest.TestCase):
    def invoke(self, args):
        return runpy.run_path(str(SHIM), run_name='__main__')

    def test_help_and_restore_delegate_unchanged(self):
        for args in [['--help'], ['hermes-desktop', '--restore']]:
            with patch.dict(os.environ, {'FEASIBILITY_REAL_NANH': '/synthetic/nanh'}), \
                 patch.object(sys, 'argv', [str(SHIM), *args]), \
                 patch('os.execv', side_effect=SystemExit) as execute:
                with self.assertRaises(SystemExit):
                    self.invoke(args)
                execute.assert_called_once_with('/synthetic/nanh', ['/synthetic/nanh', *args])

    def test_no_cdp_arm_wraps_for_classification_without_switches_or_observer(self):
        args = ['hermes-desktop', '--provider-base-url', 'http://127.0.0.1', '--', '--synthetic']
        child = Mock(pid=123, stderr=io.BytesIO(b'Missing X server'), poll=Mock(return_value=1), wait=Mock(return_value=1))
        with tempfile.TemporaryDirectory() as facts, \
             patch.dict(os.environ, {'FEASIBILITY_REAL_NANH': '/synthetic/nanh',
                                    'FEASIBILITY_HERMES_CDP': 'disabled', 'FEASIBILITY_FACTS': facts}), \
             patch.object(sys, 'argv', [str(SHIM), *args]), \
             patch('subprocess.Popen', return_value=child) as spawn, patch('signal.signal'):
            with self.assertRaises(SystemExit):
                self.invoke(args)
            self.assertEqual(spawn.call_count, 1)
            self.assertEqual(spawn.call_args.args[0], ['/synthetic/nanh', *args])
            self.assertTrue((Path(facts) / 'closed-startup-123.json').exists())

    def test_scoped_namespace_uses_upstream_suid_disable_only(self):
        args = ['hermes-desktop', '--provider-base-url', 'http://127.0.0.1']
        for policy, expected in [('default', False), ('scoped-apparmor-userns', True)]:
            child = Mock(pid=123, stderr=io.BytesIO(b''), poll=Mock(return_value=1), wait=Mock(return_value=1))
            with tempfile.TemporaryDirectory() as facts, \
                 patch.dict(os.environ, {'FEASIBILITY_REAL_NANH': '/synthetic/nanh',
                     'FEASIBILITY_HERMES_CDP': 'disabled', 'FEASIBILITY_FACTS': facts,
                     'FEASIBILITY_HERMES_NAMESPACE_POLICY': policy}), \
                 patch.object(sys, 'argv', [str(SHIM), *args]), \
                 patch('subprocess.Popen', return_value=child) as spawn, patch('signal.signal'):
                with self.assertRaises(SystemExit):
                    self.invoke(args)
                command = spawn.call_args.args[0]
                self.assertEqual('--disable-setuid-sandbox' in command, expected)
                self.assertNotIn('--no-sandbox', command)
                self.assertEqual(command.count('--'), int(expected))

    def test_observer_failure_terminates_owned_child(self):
        child = Mock(pid=123, stderr=io.BytesIO(b""), poll=Mock(return_value=None))
        with tempfile.TemporaryDirectory() as facts, \
             patch.dict(os.environ, {'FEASIBILITY_REAL_NANH': '/synthetic/nanh',
                                    'FEASIBILITY_FACTS': facts, 'FEASIBILITY_HERMES_CDP': 'enabled', 'FEASIBILITY_HERMES_DOM_INPUT': '0'}), \
             patch.object(sys, 'argv', [str(SHIM), 'hermes-desktop', '--provider-base-url', 'http://127.0.0.1']), \
             patch('subprocess.Popen', side_effect=[child, OSError('synthetic')]) as spawn, \
             patch('signal.signal'), patch('socket.socket') as socket:
            socket.return_value.__enter__.return_value.getsockname.return_value = ('127.0.0.1', 43210)
            with self.assertRaises(OSError):
                self.invoke([])
            child.terminate.assert_called_once()
            child.wait.assert_called_once_with(timeout=5)
            command = spawn.call_args_list[0].args[0]
            self.assertEqual(command.count('--'), 1)
            self.assertTrue(command[-2].startswith('--remote-debugging-port='))
            self.assertEqual(command[-1], '--remote-debugging-address=127.0.0.1')


if __name__ == '__main__':
    unittest.main()
