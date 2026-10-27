#!/usr/bin/env python3
"""The incident probe gets one review and no bypass to live credentials."""
import importlib.util
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import textwrap
import unittest

ROOT = Path(__file__).resolve().parents[2]
SOURCE = (ROOT / '.github/workflows/mimo-live-diagnostic.yml').read_text()
SPEC = importlib.util.spec_from_file_location('live_approval_contract', ROOT / 'canary/tests/live-approval.py')
approval = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(approval)
JOBS = approval.jobs(ROOT / '.github/workflows/mimo-live-diagnostic.yml')


class LiveWorkflowTests(unittest.TestCase):
    def test_all_workers_are_main_only_and_require_the_same_successful_review(self):
        self.assertEqual(set(JOBS), {'prepare', 'approve-live', 'observe'})
        self.assertTrue(approval.condition(JOBS['prepare']))
        self.assertFalse(approval.condition(JOBS['prepare'], ref='refs/heads/untrusted'))
        review, worker = JOBS['approve-live'], JOBS['observe']
        self.assertIn('needs: prepare', review)
        self.assertIn('uses: ./.github/workflows/live-check-approval.yml', review)
        self.assertNotIn('secrets.', review)
        self.assertNotIn('environment:', review)
        self.assertIn('needs: [prepare, approve-live]', worker)
        self.assertIn('name: canary-live-workers', worker)
        self.assertIn('deployment: false', worker)
        self.assertTrue(approval.condition(review))
        self.assertTrue(approval.condition(worker))
        for result in ('failure', 'cancelled', 'skipped', 'timed_out'):
            self.assertFalse(approval.condition(review, prerequisite=result))
            self.assertFalse(approval.condition(worker, prerequisite=result))
            self.assertFalse(approval.condition(worker, approval=result))
        for job in (review, worker):
            self.assertFalse(approval.condition(job, cancelled=True))
            self.assertFalse(approval.condition(job, ref='refs/heads/untrusted'))
        self.assertNotIn('inputs.mode', SOURCE)  # This workflow has no deterministic bypass.

    def test_real_input_program_rejects_drift_before_approval(self):
        self.assertEqual(len(re.findall(r'^      [a-z_0-9]+:$', SOURCE.split('permissions:', 1)[0], re.M)), 4)
        program = JOBS['prepare'].split("          python3 -B - <<'PY'\n", 1)[1].split('\n          PY', 1)[0]
        program = "import sys\nsys.path.insert(0, " + repr(str(ROOT)) + ")\n" + textwrap.dedent(program)
        valid = dict(RELEASE_TAG='v0.1.27', RELEASE_COMMIT='7c74af43c6c2917737ad90859c0d2706d674e75f',
                     ASSET_RUN_ID='40520278155', MANIFEST_SHA256='205da655631c75615c33ac2a07520f7663aa3251c712fa9cdb493ba0bb7bf5b8')
        def status(values):
            with tempfile.TemporaryDirectory() as directory:
                env = {key: value for key, value in os.environ.items() if key not in {*valid, 'NAN_API_KEY', 'GITHUB_TOKEN', 'GH_TOKEN'}}
                env.update(values, GITHUB_OUTPUT=str(Path(directory) / 'outputs'))
                result = subprocess.run([sys.executable, '-I', '-c', program], env=env,
                                        capture_output=True, timeout=10)
                self.assertNotIn(b'PRIVATE', result.stderr)
                return result.returncode
        self.assertEqual(status(valid), 0)
        for key, value in (('RELEASE_TAG', 'v0.1.28'), ('RELEASE_COMMIT', 'b'*40),
                           ('MANIFEST_SHA256', 'c'*64), ('ASSET_RUN_ID', '0'),
                           ('ASSET_RUN_ID', '99999999999999999999'), ('ASSET_RUN_ID', 'PRIVATE')):
            self.assertNotEqual(status({**valid, key: value}), 0)

    def test_read_only_verification_and_freezing_finish_before_review(self):
        prepare = JOBS['prepare']
        self.assertNotIn('secrets.', prepare)
        self.assertNotIn('environment:', prepare)
        self.assertIn('--system linux', prepare)
        self.assertIn('windows_release_assets.py', prepare)
        self.assertIn('cli-suite.py resolve', prepare)
        self.assertIn('diagnostic.frozen_selection', prepare)
        self.assertIn('frozen_manifest: ${{ steps.freeze.outputs.manifest }}', prepare)
        self.assertNotRegex(SOURCE, r'\b(?:contents|actions|id-token): write\b')
        self.assertIn('cancel-in-progress: false', SOURCE)

    def test_one_key_scoped_collector_step_uses_attested_assets_and_no_rebuild(self):
        worker = JOBS['observe']
        self.assertIn('runs-on: ubuntu-24.04-arm', worker)
        self.assertIn('node-version: 24.22.1', worker)
        self.assertEqual(worker.count('secrets.NAN_API_KEY'), 1)
        self.assertIn('--binary "$NAN_DIAGNOSTIC_RELEASE_BINARY" --canary "$NAN_DIAGNOSTIC_RELEASE_CANARY"', worker)
        self.assertIn('--directory "$RUNNER_TEMP/cli-cell"', worker)
        self.assertIn('FROZEN_MANIFEST: ${{ needs.prepare.outputs.frozen_manifest }}', worker)
        self.assertIn('--source-sha "$GITHUB_SHA"', worker)
        self.assertNotIn('cargo ', SOURCE)
        self.assertNotIn('setup-python', SOURCE)
        self.assertNotIn('secrets: inherit', SOURCE)
        paths = SOURCE.split('          path: |\n', 1)[1].splitlines()
        self.assertEqual([line.strip() for line in paths if line.strip()], [
            '${{ runner.temp }}/mimo-live-observation.json',
            '${{ runner.temp }}/mimo-live-release-provenance.json'])


if __name__ == '__main__':
    unittest.main()
