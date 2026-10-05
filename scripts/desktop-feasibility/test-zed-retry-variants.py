#!/usr/bin/env python3
"""Synthetic orchestration contracts; no native applications are launched."""
import json
import os
from pathlib import Path
import runpy
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

module = runpy.run_path(str(Path(__file__).with_name('zed-retry-variants.py')))


class VariantTests(unittest.TestCase):
    def test_variants_continue_after_assertion_failure_but_stop_after_uncertain_cleanup(self):
        for clean in (True, False):
            with self.subTest(clean=clean), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp) / 'semantic-feasibility'
                root.mkdir()
                baseline = dict(appCleanup='passed', globalCleanup='passed')
                (root / 'qualification.json').write_text(json.dumps(baseline))
                calls = []

                def execute(command, **kwargs):
                    calls.append(command)
                    if 'reduce' in command:
                        result = {**baseline, 'appCleanup': 'passed' if clean else 'failed'}
                        Path(command[command.index('--output') + 1]).write_text(json.dumps(result))
                    else:
                        self.assertIn(kwargs['env']['NANH_ZED_RETRY_POINT'], ('left-quarter', 'right-quarter'))
                        self.assertIn('--directory', command)
                    self.assertEqual(kwargs['stdout'], subprocess.DEVNULL)
                    self.assertEqual(kwargs['stderr'], subprocess.DEVNULL)
                    return subprocess.CompletedProcess(command, 1)

                env = dict(RUNNER_TEMP=tmp, GITHUB_ACTIONS='true', RUNNER_ENVIRONMENT='github-hosted',
                           NANH_ZED_RETRY_ENTRY_TRACE='1', FEASIBILITY_CHECKER='/synthetic/checker',
                           FEASIBILITY_REAL_NANH='/synthetic/nanh', GITHUB_SHA='a' * 40)
                with patch.dict(os.environ, env), patch.object(sys, 'platform', 'linux'), \
                        patch.object(subprocess, 'run', side_effect=execute):
                    self.assertEqual(module['main'](), 0 if clean else 1)
                self.assertEqual(len(calls), 4 if clean else 2)

    def test_refuses_local_execution_and_unproven_cleanup(self):
        with patch.dict(os.environ, {'GITHUB_ACTIONS': 'false'}):
            with self.assertRaises(ValueError):
                module['main']()
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'qualification.json'
            self.assertFalse(module['cleanup_passed'](path))
            path.write_text('{}')
            self.assertFalse(module['cleanup_passed'](path))


if __name__ == '__main__':
    unittest.main()
