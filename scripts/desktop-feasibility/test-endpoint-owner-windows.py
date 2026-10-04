#!/usr/bin/env python3
"""Ownership contracts use synthetic metadata; native Windows tests own only this process."""
import os
from pathlib import Path
import runpy
import socket
import sys
import unittest

module = runpy.run_path(str(Path(__file__).with_name('endpoint-owner-windows.py')))


class Ownership(unittest.TestCase):
    def test_ancestry_rejects_reuse_sessions_missing_and_cycles(self):
        ancestry = module['ancestry']
        parents = {40: 30, 30: 20, 20: 10}
        identities = {40: (300, 1), 30: (200, 1), 20: (100, 1)}
        self.assertEqual(ancestry(40, 20, parents, identities.get), 'true')
        self.assertEqual(ancestry(40, 20, parents, {**identities, 30: (400, 1)}.get), 'parent-reused')
        self.assertEqual(ancestry(40, 20, parents, {**identities, 30: (200, 2)}.get), 'session-mismatch')
        self.assertEqual(ancestry(40, 20, parents, {40: identities[40]}.get), 'parent-unavailable')
        self.assertEqual(ancestry(41, 20, parents, identities.get), 'process-unavailable')
        self.assertEqual(ancestry(40, 20, {40: 30, 30: 40}, lambda pid: (100, 1)), 'ancestry-cycle')
        self.assertEqual(ancestry(100, 20, {pid: pid - 1 for pid in range(21, 101)},
                                  lambda pid: (pid, 1)), 'ancestry-limit')

    def test_endpoint_is_unique_loopback_and_rechecked_after_ancestry(self):
        class Native:
            def __init__(self, initial, final=None):
                self.values = [initial, initial if final is None else final]
            def listeners(self, port):
                return self.values.pop(0)
            def parents(self):
                return {40: 20, 20: 10}
            def identity(self, pid):
                return pid, 1
        prove = module['prove']
        self.assertEqual(prove('endpoint', 43210, 20, Native([(40, True)])), 'true')
        for listeners in [[], [(40, False)], [(40, True), (41, True)]]:
            self.assertEqual(prove('endpoint', 43210, 20, Native(listeners)), 'listener-unavailable')
        self.assertEqual(prove('endpoint', 43210, 20, Native([(40, True)], [(41, True)])), 'listener-unavailable')

    def test_bridge_requires_exact_listener_and_original_launcher_ancestry(self):
        class Native:
            def __init__(self, listener, final=None, parents=None):
                self.values = [listener, listener if final is None else final]
                self.tree = {40: 20, 20: 10} if parents is None else parents
            def listeners(self, port):
                return self.values.pop(0)
            def parents(self):
                return self.tree
            def identity(self, pid):
                return (pid, 1) if pid in self.tree else None
        prove = module['prove_bridge']
        self.assertEqual(prove(43210, 40, 20, Native([(40, True)])), 'true')
        for listener in ([], [(20, True)], [(41, True)], [(40, False)], [(40, True), (41, True)]):
            self.assertEqual(prove(43210, 40, 20, Native(listener)), 'listener-unavailable')
        self.assertEqual(prove(43210, 40, 20, Native([(40, True)], [(41, True)])), 'listener-unavailable')
        self.assertNotEqual(prove(43210, 40, 20, Native([(40, True)], parents={40: 10,10: 1,20:10})), 'true')

    @unittest.skipUnless(sys.platform == 'win32', 'requires native Windows metadata')
    def test_native_snapshot_and_listener_belong_to_this_test_process(self):
        main = module['main']
        pid = str(os.getpid())
        self.assertEqual(main(['descendant', pid, pid]), 'true')
        with socket.socket() as listener:
            listener.bind(('127.0.0.1', 0))
            listener.listen(1)
            port = str(listener.getsockname()[1])
            self.assertEqual(main(['endpoint', port, pid]), 'true')


if __name__ == '__main__':
    unittest.main()
