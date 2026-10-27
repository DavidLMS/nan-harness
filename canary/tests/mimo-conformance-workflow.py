#!/usr/bin/env python3
"""Keep canonical MiMo observations isolated from release qualification."""
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]
SOURCE = (ROOT / '.github/workflows/mimo-conformance-diagnostic.yml').read_text()
DISPATCH = (ROOT / '.github/workflows/windows-cli-diagnostic.yml').read_text()


class CanonicalObservationWorkflowTests(unittest.TestCase):
    def test_observation_uses_canonical_host_and_shell_without_live_authority(self):
        for token in ('workflow_call:', 'runs-on: windows-2025', 'shell: bash',
                      'node-version: 24.22.1', 'ref: ${{ github.sha }}',
                      'persist-credentials: false', 'contents: read', 'actions: read'):
            self.assertIn(token, SOURCE)
        for forbidden in ('environment:', 'secrets.', 'NAN_API_KEY', 'cargo ',
                          'setup-python', 'continue-on-error:', 'workflow_dispatch:'):
            self.assertNotIn(forbidden, SOURCE)
        self.assertNotRegex(SOURCE, r'\b(?:contents|actions|id-token): write\b')

    def test_release_pair_is_verified_before_resolving_and_observing(self):
        verify = SOURCE.index('python -B canary/actions/windows_release_assets.py')
        resolve = SOURCE.index('python -B canary/actions/cli-suite.py resolve')
        observe = SOURCE.index('python -B canary/actions/mimo_conformance_diagnostic.py')
        self.assertLess(verify, resolve)
        self.assertLess(resolve, observe)
        for token in ('--binary "$NAN_DIAGNOSTIC_RELEASE_BINARY"',
                      '--canary "$NAN_DIAGNOSTIC_RELEASE_CANARY"',
                      '--directory "$RUNNER_TEMP/cli-cell"',
                      '--manifest "$RUNNER_TEMP/cli-versions.json"',
                      '--harnesses mimo-code', '--system windows --architecture x86_64 --model qwen3.6'):
            self.assertIn(token, SOURCE)
        for script in re.findall(r'python -B (canary/actions/[^ ]+\.py)', SOURCE):
            self.assertTrue((ROOT / script).is_file(), script)

    def test_only_closed_observation_and_provenance_are_uploaded(self):
        paths = SOURCE.split('          path: |\n', 1)[1].splitlines()
        self.assertEqual([path.strip() for path in paths if path.strip()], [
            '${{ runner.temp }}/mimo-canonical-observation.json',
            '${{ runner.temp }}/mimo-conformance-release-provenance.json',
        ])

    def test_dispatch_forwards_only_the_attested_selection(self):
        job = DISPATCH.split('\n  mimo-conformance-observation:\n', 1)[1].split('\n  diagnostic:\n', 1)[0]
        self.assertIn('needs: validate-inputs', job)
        self.assertIn("needs.validate-inputs.result == 'success'", job)
        self.assertIn("inputs.mode == 'mimo-conformance-observation'", job)
        self.assertIn('uses: ./.github/workflows/mimo-conformance-diagnostic.yml', job)
        self.assertEqual(len(re.findall(r'^      [a-z0-9_]+:', job, re.M)), 4)
        for forbidden in ('environment:', 'secrets:', 'secrets.', 'source_sha:', 'model:', 'runs-on:'):
            self.assertNotIn(forbidden, job)


if __name__ == '__main__':
    unittest.main()
