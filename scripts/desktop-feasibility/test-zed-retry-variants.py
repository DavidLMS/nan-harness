#!/usr/bin/env python3
"""Synthetic orchestration contracts; no native applications are launched."""
import json
import os
from pathlib import Path
import runpy
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

module = runpy.run_path(str(Path(__file__).with_name('zed-retry-variants.py')))


class VariantTests(unittest.TestCase):
    def test_real_session_guard_accepts_only_the_qualification_entry(self):
        repository = Path(__file__).resolve().parents[2]
        with tempfile.TemporaryDirectory() as tmp:
            stub = Path(tmp) / 'dbus-run-session'
            stub.symlink_to(shutil.which('true'))
            env = {**os.environ, 'PATH': tmp + os.pathsep + os.environ['PATH'],
                   'NANH_ZED_SCREEN_POLICY': 'height-1536', 'GITHUB_ACTIONS': 'true',
                   'RUNNER_ENVIRONMENT': 'github-hosted', 'RUNNER_OS': 'Linux',
                   'FEASIBILITY_ZED_MAXIMIZED': '1', 'NANH_ZED_PANEL_LAYOUT': 'fixed-wide'}
            for key in ('NANH_ZED_LAYOUT_POLICY', 'NANH_ZED_PANEL_ZOOM'):
                env.pop(key, None)
            prefix = ['bash', str(repository / 'scripts/run-desktop-check-session.sh'), 'python3']
            for entry, expected in [('run-qualification.py', 0), ('zed-retry-variants.py', 1)]:
                result = subprocess.run([*prefix, 'scripts/desktop-feasibility/' + entry,
                    '--app', 'zed-desktop', '--platform', 'linux'], env=env,
                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=5)
                self.assertEqual(result.returncode, expected)

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
                        self.assertEqual(command[0], 'bash')
                        self.assertEqual(command[2:8], ['python3', 'scripts/desktop-feasibility/run-qualification.py',
                            '--app', 'zed-desktop', '--platform', 'linux'])
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
