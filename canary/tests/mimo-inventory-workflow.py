#!/usr/bin/env python3
"""Keep the native observation separate from release authority and private state."""
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / '.github/workflows/mimo-inventory-diagnostic.yml'


class InventoryWorkflowTests(unittest.TestCase):
    def test_observation_has_no_live_or_publication_authority(self):
        source = WORKFLOW.read_text()
        self.assertIn('  workflow_call:', source)
        self.assertNotIn('  workflow_dispatch:', source)
        self.assertNotIn('environment:', source)
        self.assertNotIn('secrets.', source)
        self.assertNotIn('NAN_API_KEY', source)
        self.assertNotRegex(source, r'\b(?:contents|actions|id-token): write\b')
        self.assertIn('  contents: read', source)
        self.assertIn('  actions: read', source)
        self.assertIn('cancel-in-progress: false', source)

    def test_only_validated_envelope_and_public_provenance_are_uploaded(self):
        source = WORKFLOW.read_text()
        paths = source.split('          path: |\n', 1)[1].splitlines()
        self.assertEqual([path.strip() for path in paths if path.strip()], [
            '${{ runner.temp }}/mimo-inventory-observation/mimo-inventory-diagnostic.json',
            '${{ runner.temp }}/mimo-inventory-release-provenance.json',
        ])
        self.assertNotIn('*', '\n'.join(paths))

    def test_source_and_release_identity_are_checked_before_native_observation(self):
        source = WORKFLOW.read_text()
        self.assertIn('ref: ${{ github.sha }}', source)
        self.assertIn('persist-credentials: false', source)
        self.assertIn('(git rev-parse HEAD) -cne $env:GITHUB_SHA', source)
        verifier = source.index('python -B canary/actions/windows_release_assets.py')
        observer = source.index('python -B canary/actions/mimo_inventory_diagnostic.py')
        self.assertLess(verifier, observer)
        self.assertIn('--binary $env:NAN_DIAGNOSTIC_RELEASE_BINARY', source)
        self.assertIn('--source-sha $env:GITHUB_SHA', source)
        self.assertNotIn('continue-on-error:', source)
        for script in re.findall(r'python -B (canary/actions/[^ ]+\.py)', source):
            self.assertTrue((ROOT / script).is_file(), script)


if __name__ == '__main__':
    unittest.main()
