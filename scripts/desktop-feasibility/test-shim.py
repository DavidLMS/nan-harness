#!/usr/bin/env python3
"""Exercise delegation and owned-child cleanup on observer startup failure."""
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

    def test_observer_failure_terminates_owned_child(self):
        child = Mock(pid=123, poll=Mock(return_value=None))
        with patch.dict(os.environ, {'FEASIBILITY_REAL_NANH': '/synthetic/nanh',
                                    'FEASIBILITY_FACTS': '/synthetic/facts'}), \
             patch.object(sys, 'argv', [str(SHIM), 'hermes-desktop', '--provider-base-url', 'http://127.0.0.1']), \
             patch('subprocess.Popen', side_effect=[child, OSError('synthetic')]) as spawn, \
             patch('signal.signal'):
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
