#!/usr/bin/env python3
"""A/B isolation and cleanup failure preserve a partial closed observation."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('experiments', Path(__file__).with_name('run-experiments.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class ExperimentLifecycle(unittest.TestCase):
    def test_arm_credentials_and_fact_paths_are_isolated(self):
        with patch.dict(os.environ, {'NAN_API_KEY': 'synthetic-live-key', 'GH_TOKEN': 'synthetic-github-key',
                                     'FEASIBILITY_ZED_AX_FACTS': '/stale'}):
            first = module.experiment_environment('hermes-desktop', 'without-cdp', Path('/first'), Path('/nanh'))
            second = module.experiment_environment('hermes-desktop', 'with-cdp', Path('/second'), Path('/nanh'))
        self.assertNotIn('NAN_API_KEY', first)
        self.assertNotIn('GH_TOKEN', second)
        self.assertNotIn('FEASIBILITY_ZED_AX_FACTS', second)
        self.assertEqual(first['FEASIBILITY_HERMES_CDP'], 'disabled')
        self.assertEqual(second['FEASIBILITY_HERMES_CDP'], 'enabled')
        self.assertNotEqual(first['FEASIBILITY_FACTS'], second['FEASIBILITY_FACTS'])

    def test_cleanup_failure_prevents_second_arm_and_retains_first(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            frozen = root / 'frozen.json'
            frozen.write_text('{}')
            calls = []
            def run(command, **kwargs):
                calls.append(command)
                if command[1].endswith('desktop_diagnostics.py'):
                    output = Path(command[command.index('--output') + 1])
                    output.write_text(json.dumps(dict(schemaVersion=1, sourceSha='a' * 40,
                                                       platform='linux', events=[], invalidEvents=0)))
                else:
                    output = Path(command[command.index('--output') + 1])
                    output.write_text(json.dumps(dict(appCleanup='failed', reportCleanup='failed')))
                return subprocess.CompletedProcess(command, 1)
            args = ['run-experiments.py', '--app', 'hermes-desktop', '--checker', '/checker',
                    '--real-nanh', '/nanh', '--prepared', '/prepared', '--frozen', str(frozen),
                    '--directory', str(root), '--source-sha', 'a' * 40, '--release-tag', 'v2026.9.24',
                    '--platform', 'linux']
            with patch.object(sys, 'argv', args), \
                 patch.dict(os.environ, {'GITHUB_ACTIONS': 'true', 'RUNNER_ENVIRONMENT': 'github-hosted'}), \
                 patch('subprocess.run', side_effect=run):
                self.assertEqual(module.main(), 1)
            self.assertEqual(len(calls), 2)
            self.assertFalse((root / 'with-cdp').exists())
            combined = json.loads((root / 'summary.json').read_text())
            self.assertEqual([arm['condition'] for arm in combined['arms']], ['without-cdp'])
            self.assertFalse(combined['noOcrQualification'])


if __name__ == '__main__':
    unittest.main()
