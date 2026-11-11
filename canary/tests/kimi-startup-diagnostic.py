#!/usr/bin/env python3
"""Exercise startup experiment lifecycle without installing or launching a vendor."""
import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

ACTIONS = Path(__file__).resolve().parents[1] / 'actions'
sys.path.insert(0, str(ACTIONS))
spec = importlib.util.spec_from_file_location('kimi_startup', ACTIONS / 'kimi_startup_diagnostic.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class StartupDiagnosticTests(unittest.TestCase):
    def run_case(self, directory, install_code=0, experiment_code=0):
        root = Path(directory)
        (root / 'kimi-startup-test-build.jsonl').write_text(json.dumps({
            'reason': 'compiler-artifact', 'target': {'name': 'nan_harness_test_support'},
            'profile': {'test': True}, 'executable': str(root / 'test.exe'),
        }) + '\n')
        calls = []

        def execute(arguments, cell, env, timeout):
            calls.append(arguments)
            if len(calls) == 1:
                self.assertEqual(arguments[-2:], ['-Version', '2.5.3'])
                return install_code, 'nonzero' if install_code else 'exit'
            Path(env['NAN_KIMI_STARTUP_OUTPUT']).write_text(json.dumps({
                'schemaVersion': 1, 'diagnosticsOnly': True, 'expectedVersion': '2.5.3',
                'records': [{'case': case, 'process': {'status': 'nonzero-exit', 'exitCode': 1,
                        'stdoutBytes': 0, 'stderrBytes': 0, 'marker': False, 'errorSignals': []},
                    **({'versionVerified': False} if index < 2 else {'provider': {
                        'requestCount': 0, 'completed': False, 'bounded': True, 'shutdown': True}})}
                    for index, case in enumerate(module.CASES)],
            }))
            (cell / 'private-output').write_text('private-secret')
            return experiment_code, 'nonzero' if experiment_code else 'exit'

        with patch.dict(os.environ, {'RUNNER_TEMP': directory, 'GITHUB_SHA': 'a' * 40}), \
                patch.object(module, 'protect_private'), \
                patch.object(module, 'isolated_environment', return_value={}), \
                patch.object(module, 'run_bounded', side_effect=execute):
            if install_code:
                with self.assertRaises(RuntimeError):
                    module.main()
            elif experiment_code:
                with self.assertRaises(SystemExit):
                    module.main()
            else:
                module.main()
        report = (root / 'kimi-startup-report.json').read_text()
        self.assertNotIn('private-secret', report)
        self.assertFalse((root / 'kimi-startup-private').exists())
        self.assertEqual(json.loads(report)['cleanup'], 'passed')
        return json.loads(report), calls

    def test_reducer_rejects_private_fields_and_invalid_counters(self):
        base = {'schemaVersion': 1, 'diagnosticsOnly': True, 'expectedVersion': '2.5.3', 'records': []}
        self.assertIsNotNone(module.closed_observation(base, False))
        self.assertIsNone(module.closed_observation(base, True))
        self.assertIsNone(module.closed_observation({**base, 'stderr': 'private-secret'}, False))
        invalid = {**base, 'records': [{'case': 'fresh-version', 'versionVerified': False,
                   'process': {'status': 'process-error', 'output': 'private-secret'}}]}
        self.assertIsNone(module.closed_observation(invalid, False))
        process = {'status': 'nonzero-exit', 'exitCode': 1, 'marker': False,
                   'stdoutBytes': 0, 'stderrBytes': 42, 'errorSignals': ['private-secret']}
        self.assertFalse(module.safe_process(process))
        process['errorSignals'] = ['type-error']
        self.assertTrue(module.safe_process(process))
        self.assertFalse(module.safe_process({'status': 'passed', 'exitCode': 0, 'marker': True,
                                             'stdoutBytes': True, 'stderrBytes': 0}))

    def test_pinned_closed_experiment_cleans_private_output(self):
        with tempfile.TemporaryDirectory() as directory:
            report, calls = self.run_case(directory)
        self.assertEqual(report['experiment'], 'completed')
        self.assertEqual(calls[1][-1], '--ignored')

    def test_install_failure_retains_closed_report_without_running_test(self):
        with tempfile.TemporaryDirectory() as directory:
            report, calls = self.run_case(directory, install_code=1)
        self.assertEqual(len(calls), 1)
        self.assertEqual(report['install'], 'failed')
        self.assertEqual(report['experiment'], 'not-started')

    def test_experiment_failure_retains_checkpoint_and_cleans(self):
        with tempfile.TemporaryDirectory() as directory:
            report, _ = self.run_case(directory, experiment_code=1)
        self.assertEqual(report['experiment'], 'failed')
        self.assertIn('observation', report)


if __name__ == '__main__':
    unittest.main()
