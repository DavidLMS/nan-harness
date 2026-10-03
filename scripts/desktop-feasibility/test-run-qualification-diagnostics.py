#!/usr/bin/env python3
"""Closed diagnostic publication retains executor failures without running applications."""
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('runner', Path(__file__).with_name('run-qualification.py'))
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)
from cell import CleanupError, StageTimeout


class Publication(unittest.TestCase):
    def execute(self, root):
        return runner.execute_with_diagnostics(['synthetic'], root, root, 'windows', 'a' * 40, {})

    @staticmethod
    def captured_failure(error):
        def execute(*_args, diagnostic_callback, **_kwargs):
            event = dict(schemaVersion=1, app='claude-desktop', probeIndex=0, mode='deterministic',
                         launchStage='window-acquired', composer=[], truncated=False,
                         launchExit='unknown', cleanup=dict(stage='restore', originalReason='action-unsupported',
                         reason='cleanup-failed', restore='nonzero-exit'))
            stream = b'DESKTOP_DIAGNOSTIC:' + json.dumps(event).encode() + b'\nDESKTOP_DIAGNOSTIC:{"private":"PRIVATE"}\n'
            diagnostic_callback(io.BytesIO(stream), 1)
            raise error
        return execute

    def test_cleanup_and_timeout_failures_publish_closed_capture_and_still_raise(self):
        for error in (CleanupError('synthetic cleanup failure'), StageTimeout('synthetic timeout')):
            with tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                with patch.object(runner, 'private_command', self.captured_failure(error)):
                    with self.assertRaises(type(error)) as raised:
                        self.execute(root)
                self.assertIs(raised.exception, error)
                published = json.loads((root / 'native-diagnostics.json').read_text())
                self.assertEqual(published['invalidEvents'], 1)
                self.assertEqual(len(published['events']), 1)
                self.assertEqual(published['events'][0]['record']['cleanup']['restore'], 'nonzero-exit')
                self.assertNotIn('PRIVATE', json.dumps(published))

    def test_publication_error_preserves_pending_failure_but_fails_normal_execution(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            original = CleanupError('synthetic cleanup failure')
            with patch.object(runner, 'private_command', self.captured_failure(original)), \
                    patch.object(runner, 'write_json', side_effect=OSError('synthetic write failure')):
                with self.assertRaises(CleanupError) as raised:
                    self.execute(root)
                self.assertIs(raised.exception, original)
            with patch.object(runner, 'private_command', return_value=1), \
                    patch.object(runner, 'write_json', side_effect=OSError('synthetic write failure')):
                with self.assertRaises(OSError):
                    self.execute(root)

    def test_normal_nonzero_status_is_preserved_with_a_closed_empty_bundle(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            with patch.object(runner, 'private_command', return_value=1):
                self.assertEqual(self.execute(root), 1)
            published = json.loads((root / 'native-diagnostics.json').read_text())
            self.assertEqual(published['events'], [])
            self.assertEqual(published['invalidEvents'], 0)


class LinuxChatPolicy(unittest.TestCase):
    def test_chat_trial_requires_hosted_linux_startup_and_waits_for_release_admission(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = dict(GITHUB_ACTIONS='true', RUNNER_ENVIRONMENT='github-hosted',
                          RUNNER_OS='Linux', NANH_DESKTOP_QUALIFICATION_MODE='startup-baseline',
                          NANH_CLAUDE_LINUX_CHAT_ONLY='1')

            def environment(values=source, app='claude-desktop'):
                return runner.qualification_environment(app, root, root / 'nanh', root / 'claude', values)

            # The flag is forwarded only after run() admits the exact release.
            self.assertNotIn('NANH_CLAUDE_LINUX_CHAT_ONLY', environment())
            for change in ({'RUNNER_OS': 'macOS'}, {'GITHUB_ACTIONS': 'false'},
                           {'RUNNER_ENVIRONMENT': 'self-hosted'},
                           {'NANH_DESKTOP_QUALIFICATION_MODE': 'renderer'},
                           {'NANH_CLAUDE_LINUX_CHAT_ONLY': '0'}):
                with self.assertRaises(ValueError):
                    environment({**source, **change})
            with self.assertRaises(ValueError):
                environment(app='chatgpt-desktop')


if __name__ == '__main__':
    unittest.main()
