import importlib.util, json, pathlib, sys, time, unittest
from unittest import mock
import os
import subprocess
import io
p = pathlib.Path(__file__).with_name('zed-xrecord-supervisor.py')
spec = importlib.util.spec_from_file_location('supervisor', p)
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)

class Tests(unittest.TestCase):

    def fake(self, code):
        return [sys.executable, '-c', code]

    def test_closed_result_from_owned_worker_and_cleanup(self):
        o = m.Observer(10, 20, lambda: True, self.fake('import sys;sys.stdin.readline();print(\'{"stage":"armed"}\',flush=True);sys.stdin.readline();print(\'{"status":"complete","pressCount":1,"releaseCount":1,"orderedPair":true}\',flush=True)'))
        child = o.child
        self.assertIsNone(o.result)
        self.assertTrue(o.finish()['orderedPair'])
        self.assertIsNotNone(child.poll())

    def test_cutoff_receipt_is_consumed_without_renewing_observation(self):
        receipt={'status':'complete','pressCount':0,'releaseCount':0,'orderedPair':False,
            'crossingHeaders':{'status':'observed','ownedNormalEnterCount':1,
                'ownedNonNormalEnterCount':0,'ownedNormalLeaveCount':1,'ownedMotionCount':1}}
        code='import sys;sys.stdin.readline();print(\'{"stage":"armed"}\',flush=True);print('+repr(json.dumps(receipt))+',flush=True)'
        observer=m.Observer(10,20,lambda:True,self.fake(code),budget=0.2)
        child=observer.child;child.wait(timeout=0.2)
        original_end=observer.end;observer.end=time.monotonic()-0.01
        expired_end=observer.end
        self.assertEqual(observer.finish(),receipt)
        self.assertEqual(observer.end,expired_end);self.assertLess(observer.end,original_end)
        self.assertIsNone(observer.child)

    def test_missing_extension_is_unobserved_not_zero_delivery(self):
        o = m.Observer(10, 20, lambda: True, self.fake('print(\'{"status":"unavailable","pressCount":null,"releaseCount":null,"orderedPair":null}\',flush=True)'))
        self.assertEqual(o.finish(), m.unobserved('unavailable'))

    def test_stage_boundary_accepts_only_closed_enum_and_legacy_receipts(self):
        for stage in m.STAGES:
            payload = m.unobserved('unavailable', stage)
            observer = m.Observer(10, 20, lambda: True,
                self.fake('print(' + repr(json.dumps(payload)) + ',flush=True)'))
            self.assertEqual(observer.finish(), payload)
            self.assertIsNone(observer.child)
        for stage in ['PRIVATE', None, 2, {}]:
            with self.assertRaises(ValueError):
                m.validate({**m.unobserved('unavailable'), 'stage': stage})
        self.assertEqual(m.validate(m.unobserved('unavailable')), m.unobserved('unavailable'))

    def test_invalid_budget_and_request_never_spawn(self):
        with mock.patch.object(m.subprocess, 'Popen') as spawn:
            self.assertEqual(m.Observer(10, 20, lambda: True, budget=0).finish(),
                             m.unobserved('unavailable', 'budget-insufficient'))
            self.assertEqual(m.Observer(0, 20, lambda: True).finish(),
                             m.unobserved('unavailable', 'request'))
            spawn.assert_not_called()

    def test_hung_native_arm_is_killed_without_input_or_clock_reset(self):
        start = time.monotonic()
        o = m.Observer(10, 20, lambda: True, self.fake('import time;time.sleep(30)'), budget=0.1)
        self.assertEqual(o.finish()['status'], 'timeout')
        self.assertIsNone(o.child)
        self.assertLess(time.monotonic() - start, 1)

    def test_loss_after_arm_cannot_report_pair(self):
        guard = iter([True, False])
        o = m.Observer(10, 20, lambda: next(guard), self.fake('import sys;sys.stdin.readline();print(\'{"stage":"armed"}\',flush=True);sys.stdin.readline()'))
        self.assertEqual(o.finish(), m.unobserved('identity-failed', 'armed'))

    def test_raw_or_duplicate_payload_rejected_and_worker_reaped(self):
        for payload in ['{"private":"content"}', '{"stage":"armed","stage":"armed"}']:
            o = m.Observer(10, 20, lambda: True, self.fake('print(' + repr(payload) + ',flush=True)'))
            self.assertEqual(o.finish(), m.unobserved('query-failed', 'request'))

    def test_inconsistent_approval_rejected(self):
        with self.assertRaises(ValueError):
            m.validate({'status': 'complete', 'pressCount': 0, 'releaseCount': 1, 'orderedPair': True})
    def test_eof_without_receipt_is_unobserved(self):
        observer = m.Observer(10, 20, lambda: True, self.fake('pass'))
        self.assertEqual(observer.finish(), m.unobserved('query-failed', 'request'))
        self.assertIsNone(observer.child)

    def test_trailing_bytes_are_not_a_second_output_channel(self):
        payload = json.dumps(m.unobserved('unavailable')) + "\nPRIVATE\n"
        observer = m.Observer(10, 20, lambda: True,
                              self.fake('import sys;sys.stdout.write(' + repr(payload) + ');sys.stdout.flush()'))
        self.assertEqual(observer.finish(), m.unobserved('query-failed', 'cleanup'))
        self.assertIsNone(observer.child)

    def test_worker_does_not_inherit_provider_or_python_environment(self):
        code = """import json, os
allowed = {'DISPLAY', 'XAUTHORITY', 'GITHUB_ACTIONS', 'RUNNER_ENVIRONMENT', 'RUNNER_OS', 'LC_CTYPE'}
if any(key in os.environ for key in ('NAN_API_KEY', 'PYTHONPATH')):
    print('{}', flush=True)
else:
    print(json.dumps({'status':'unavailable','pressCount':None,'releaseCount':None,'orderedPair':None}), flush=True)
"""
        with mock.patch.dict(os.environ, {'NAN_API_KEY': 'PRIVATE', 'PYTHONPATH': 'PRIVATE'}):
            with mock.patch.object(m.subprocess, 'Popen', wraps=subprocess.Popen) as spawn:
                observer = m.Observer(10, 20, lambda: True, self.fake(code))
            self.assertLessEqual(set(spawn.call_args.kwargs['env']),
                {'DISPLAY', 'XAUTHORITY', 'GITHUB_ACTIONS', 'RUNNER_ENVIRONMENT', 'RUNNER_OS'})
        self.assertEqual(observer.finish(), m.unobserved('unavailable'))

    def test_failed_reap_retains_owned_child_until_next_cleanup(self):
        observer = m.Observer.__new__(m.Observer)
        child = mock.Mock()
        child.stdin, child.stdout = io.BytesIO(), io.BytesIO()
        child.poll.return_value = None
        child.wait.side_effect = [subprocess.TimeoutExpired('owned', 0), 0]
        observer.child = child
        observer.absolute_end = time.monotonic()
        observer.pending = bytearray()
        observer.result = m.unobserved('unavailable')
        observer.close()
        self.assertIs(observer.child, child)
        self.assertEqual(observer.result, m.unobserved('timeout', 'cleanup'))
        observer.close()
        self.assertIsNone(observer.child)
        self.assertEqual(child.wait.call_count, 2)

    def test_preflight_timeout_after_arm_reaps_owned_worker(self):
        calls = 0
        def guard():
            nonlocal calls
            calls += 1
            if calls == 2:
                raise subprocess.TimeoutExpired('private-query', 0)
            return True
        observer = m.Observer(10, 20, guard,
            self.fake("import sys;sys.stdin.readline();print('{\"stage\":\"armed\"}',flush=True);sys.stdin.readline()"))
        self.assertEqual(observer.finish(), m.unobserved('identity-failed', 'armed'))
        self.assertIsNone(observer.child)

class HeaderContracts(unittest.TestCase):
    def test_optional_header_partition_and_privacy(self):
        h=dict(status='observed',ownedNormalEnterCount=0,ownedNonNormalEnterCount=0,
               ownedNormalLeaveCount=0,ownedMotionCount=9)
        value=dict(status='complete',pressCount=0,releaseCount=0,orderedPair=False,crossingHeaders=h)
        self.assertEqual(m.validate(value),value)
        for change in ({'ownedMotionCount':True},{'ownedMotionCount':65},{'rawWindow':'PRIVATE'},
                       {'status':[]},{'ownedNormalEnterCount':None}):
            with self.assertRaises(ValueError):m.validate({**value,'crossingHeaders':{**h,**change}})
        with self.assertRaises(ValueError):m.validate({**value,'status':'timeout'})

if __name__ == '__main__':
    unittest.main()
