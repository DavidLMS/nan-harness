#!/usr/bin/env python3
"""Synthetic startup classification, bounded drain and privacy tests."""
import io
import json
from pathlib import Path
import runpy
import tempfile
import unittest
import os
from unittest.mock import patch

MODULE = runpy.run_path(str(Path(__file__).with_name('hermes-startup.py')))
Capture, classify, LIMIT = MODULE['Capture'], MODULE['classify'], MODULE['CAPTURE_LIMIT']


class StartupTests(unittest.TestCase):
    def test_classifies_fatal_categories_without_signal_guessing(self):
        cases = {'sandbox-helper': b'The SUID sandbox helper binary was found, but is not configured correctly.',
                 'namespace-denied': b'Failed to move to new namespace: Operation not permitted',
                 'root-without-sandbox': b'Running as root without --no-sandbox is not supported',
                 'display-unavailable': b'Missing X server or $DISPLAY',
                 'missing-library': b'error while loading shared libraries: libsecret.so',
                 'gpu-fatal': b'GPU process isn\'t usable. Goodbye.',
                 'native-module': b'compiled against a different NODE_MODULE_VERSION'}
        for expected, data in cases.items():
            self.assertEqual(classify(data), expected)
        self.assertEqual(classify(b'childExit signal 5 private prompt'), 'unclassified')

    def test_namespace_policy_is_closed(self):
        for policy, expected in [('scoped-apparmor-userns', 'scoped-apparmor-userns'),
                                 ('private unknown value', 'default')]:
            with tempfile.TemporaryDirectory() as root, patch.dict(os.environ, {'FEASIBILITY_HERMES_NAMESPACE_POLICY': policy}):
                capture = Capture(io.BytesIO(b'Missing X server'))
                capture.start()
                output = Path(root) / 'facts.json'
                capture.save(output, 1)
                self.assertEqual(json.loads(output.read_text())['namespacePolicy'], expected)
                self.assertNotIn('private unknown value', output.read_text())

    def test_drains_after_budget_and_never_serializes_private_bytes(self):
        private = b'PRIVATE_SYNTHETIC_VALUE'
        capture = Capture(io.BytesIO(b'Missing X server\n' + private * LIMIT))
        capture.start()
        with tempfile.TemporaryDirectory() as root:
            output = Path(root) / 'facts.json'
            capture.save(output, 1)
            text = output.read_text()
            facts = json.loads(text)
            self.assertNotIn(private.decode(), text)
            self.assertEqual(len(capture.data), LIMIT)
            self.assertTrue(facts['captureTruncated'])
            self.assertTrue(facts['drainComplete'])
            self.assertEqual(facts['startupCategory'], 'unclassified')
            self.assertEqual(output.stat().st_mode & 0o777, 0o600)


if __name__ == '__main__':
    unittest.main()
