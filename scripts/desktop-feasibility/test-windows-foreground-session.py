"""Synthetic lifecycle tests; never access the local desktop."""
import json
from pathlib import Path
import tempfile
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'canary/actions'))
import windows_foreground_session as session


class FakeTimeout:
    def __init__(self, value=200000):
        self.value = value
        self.writes = []
        self.reject_restore = False

    def read(self):
        return self.value

    def set(self, value):
        self.writes.append(value)
        if self.reject_restore and value != 0:
            raise RuntimeError('synthetic restore failure')
        self.value = value


class ForegroundSessionTests(unittest.TestCase):
    def setUp(self):
        self.env = patch.dict('os.environ', {'GITHUB_ACTIONS': 'true',
            'RUNNER_ENVIRONMENT': 'github-hosted', 'RUNNER_OS': 'Windows'})
        self.env.start()
        self.platform = patch.object(session.sys, 'platform', 'win32')
        self.platform.start()
        self.addCleanup(self.env.stop)
        self.addCleanup(self.platform.stop)

    def run_session(self, api, action):
        with tempfile.TemporaryDirectory() as tmp, patch.object(session, 'ForegroundTimeout', return_value=api):
            failure = None
            try:
                with session.prepare(tmp):
                    action(api)
            except RuntimeError as error:
                failure = str(error)
            receipt = json.loads((Path(tmp) / 'windows-foreground-session.json').read_text())
            return receipt, failure

    def test_restores_original_after_success_and_executor_failure(self):
        for fails in (False, True):
            api = FakeTimeout()
            def action(api):
                self.assertEqual(api.value, 0)
                if fails:
                    raise RuntimeError('synthetic executor failure')
            receipt, error = self.run_session(api, action)
            self.assertEqual(api.writes, [0, 200000])
            self.assertEqual(api.value, 200000)
            self.assertEqual(receipt['stage'], 'completed')
            self.assertTrue(receipt['restored'])
            self.assertEqual(error, 'synthetic executor failure' if fails else None)

    def test_already_zero_does_not_mutate(self):
        api = FakeTimeout(0)
        receipt, error = self.run_session(api, lambda _: None)
        self.assertEqual(api.writes, [])
        self.assertIsNone(error)
        self.assertTrue(receipt['restored'])

    def test_failed_restoration_remains_failure(self):
        api = FakeTimeout()
        receipt, error = self.run_session(api, lambda api: setattr(api, 'reject_restore', True))
        self.assertEqual(error, 'synthetic restore failure')
        self.assertFalse(receipt['restored'])
        self.assertEqual(receipt['stage'], 'restore')
        self.assertEqual(receipt['failureStage'], 'restore')

    def test_failed_preparation_never_executes_and_attempts_restore(self):
        api = FakeTimeout()
        original = api.set
        def uncertain(value):
            original(value)
            if value == 0:
                raise RuntimeError('synthetic uncertain set')
        api.set = uncertain
        receipt, error = self.run_session(api, lambda _: self.fail('must not execute'))
        self.assertEqual(error, 'synthetic uncertain set')
        self.assertEqual(api.value, 200000)
        self.assertFalse(receipt['prepared'])
        self.assertTrue(receipt['restored'])
        self.assertEqual(receipt['failureStage'], 'prepare')

    def test_local_and_self_hosted_sessions_rejected_before_native_access(self):
        for change in ({'RUNNER_ENVIRONMENT': 'self-hosted'}, {'RUNNER_OS': 'Linux'}, {'GITHUB_ACTIONS': 'false'}):
            with patch.dict('os.environ', change), patch.object(session, 'ForegroundTimeout') as native:
                with self.assertRaises(ValueError):
                    with session.prepare('/unused'):
                        self.fail('must not execute')
                native.assert_not_called()


if __name__ == '__main__':
    unittest.main()
