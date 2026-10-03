#!/usr/bin/env python3
import runpy
from pathlib import Path
import unittest
import json
from unittest.mock import patch
import types

m = runpy.run_path(str(Path(__file__).with_name('zed-transient-dialogs.py')))


class Tree:
    def __init__(self):
        # Openbox frames hide clients one level below root; unmapped clients
        # still appear in the XQueryTree walk.
        self.tree = {1:[2,3],2:[10],3:[11,12,13],10:[],11:[],12:[],13:[]}
        self.parents = {11:10,12:10,13:10}
        self.pids = {10:20,11:20,12:20,13:99}
        self.map = {11:1,12:0,13:1}
        self.reads = []
    def root(self):
        return 1
    def children(self, window):
        self.reads.append(window)
        return self.tree[window]
    def dialog_parent(self, window):
        return self.parents.get(window)
    def pid(self, window):
        return self.pids[window]
    def mapped(self, window):
        return self.map[window]


class Tests(unittest.TestCase):
    def collect(self, tree, guard=lambda:True, clock=lambda:0):
        return m['observe'](10,20,tree,guard,10,clock)
    def test_mapped_unmapped_owned_dialogs_count_foreign_ignored(self):
        result = self.collect(Tree())
        self.assertEqual(result, {'state':'complete','ownedTransientDialogs':2,
                                  'mappedOwnedTransientDialogs':1})
        self.assertNotIn('20',json.dumps(result))
    def test_no_dialog_is_measured_zero_not_unknown(self):
        tree = Tree()
        tree.parents = {}
        self.assertEqual(self.collect(tree)['ownedTransientDialogs'],0)
    def test_deadline_owner_change_cycle_and_budget_never_publish_partial_counts(self):
        for case in ('deadline','owner','cycle','limit','query'):
            tree = Tree()
            clock = lambda:0
            guard = lambda:True
            if case == 'deadline': clock = lambda:11
            if case == 'owner':
                states = iter([True,False])
                guard = lambda:next(states)
            if case == 'cycle': tree.tree[12] = [1]
            if case == 'limit': tree.tree[1] = list(range(513))
            if case == 'query':
                tree.mapped = lambda _: (_ for _ in ()).throw(OSError('PRIVATE'))
            result = self.collect(tree,guard,clock)
            self.assertNotEqual(result['state'],'complete')
            self.assertIsNone(result['ownedTransientDialogs'])
            self.assertIsNone(result['mappedOwnedTransientDialogs'])
            self.assertNotIn('PRIVATE',json.dumps(result))
    def test_client_pid_recheck_and_dialog_cap_leave_counts_unknown(self):
        tree = Tree()
        original_pid = tree.pid
        calls = []
        def pid(window):
            if window == 10:
                calls.append(True)
                return 20 if len(calls) == 1 else 99
            return original_pid(window)
        tree.pid = pid
        self.assertEqual(self.collect(tree)['state'],'identity-rejected')
        tree = Tree()
        tree.tree = {1:list(range(11,44)), **{n:[] for n in range(11,44)}}
        tree.parents = {n:10 for n in range(11,44)}
        tree.pids = {10:20, **{n:20 for n in range(11,44)}}
        tree.map = {n:0 for n in range(11,44)}
        result = self.collect(tree)
        self.assertEqual(result['state'],'limit')
        self.assertIsNone(result['ownedTransientDialogs'])

    def test_late_query_does_not_extend_original_deadline(self):
        tree = Tree()
        clock = iter([0,0,0,0,0,11])
        result = self.collect(tree,clock=lambda:next(clock))
        self.assertEqual(result['state'],'deadline')
        self.assertIsNone(result['ownedTransientDialogs'])


class NativeBoundary(unittest.TestCase):
    def test_expired_capture_does_not_prepare_or_issue_query(self):
        with patch.dict(m['capture'].__globals__, NativeTree=lambda: self.fail('native query')):
            self.assertEqual(m['capture'](10,20,lambda:True,0)['state'],'deadline')

    def test_native_tree_never_queries_after_elapsed_original_deadline(self):
        tree = m['NativeTree'].__new__(m['NativeTree'])
        tree.deadline, tree.error = 0, False
        tree.x = types.SimpleNamespace(XDefaultRootWindow=lambda *_: self.fail('late query'))
        with self.assertRaises(m['Unavailable']) as rejected:
            tree.root()
        self.assertEqual(rejected.exception.state,'deadline')

    def test_initialization_failure_restores_error_handler_and_closes_connection(self):
        calls = []
        class Function:
            def __init__(self, callback):
                self.callback = callback
            def __call__(self, *args):
                return self.callback(*args)
        class Library:
            def __init__(self, res=False):
                self.res = res
            def __getattr__(self, name):
                def execute(*args):
                    calls.append(name)
                    if name == 'XOpenDisplay': return 17
                    if name == 'XSetErrorHandler': return 99
                    if name == 'XResQueryVersion': return 0
                    return 1
                value = Function(execute)
                setattr(self,name,value)
                return value
        x, res = Library(), Library(True)
        with patch.object(m['C'],'CDLL',side_effect=[x,res]):
            result = m['capture'](10,20,lambda:True,m['time'].monotonic()+1)
        self.assertEqual(result['state'],'unavailable')
        self.assertEqual(calls.count('XSetErrorHandler'),2)
        self.assertEqual(calls.count('XCloseDisplay'),1)
        self.assertNotIn('XQueryTree',calls)
        self.assertLess(calls.index('XCloseDisplay'),
                        calls.index('XSetErrorHandler',calls.index('XSetErrorHandler')+1))
    def test_close_failure_still_restores_handler_and_clears_owned_connection(self):
        calls = []
        tree = m['NativeTree'].__new__(m['NativeTree'])
        tree.display, tree.error_handler, tree.old_error_handler = 17, object(), 99
        def close(_):
            calls.append('close')
            raise OSError('PRIVATE')
        tree.x = types.SimpleNamespace(XCloseDisplay=close,
            XSetErrorHandler=lambda old: calls.append(('restore',old)))
        with self.assertRaises(OSError):
            tree.close()
        self.assertEqual(calls,['close',('restore',99)])
        self.assertIsNone(tree.display)
        tree.close()
        self.assertEqual(len(calls),2)



if __name__ == '__main__':
    unittest.main()
