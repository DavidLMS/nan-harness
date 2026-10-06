#!/usr/bin/env python3
"""Offline regression contracts for desktop maintenance provenance and selection."""
import hashlib
import io
import json
from pathlib import Path
import sys
import unittest
from unittest.mock import patch
import zipfile

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'canary/actions'))
import desktop_maintenance as d
from desktop_qualification import envelope

SHA = 'a' * 40
EXPECTED = dict(status='frozen', app='chatgpt-desktop', version='1.2.3', digest='sha256:' + 'b' * 64)


def receipt(expected=EXPECTED):
    value = envelope(expected['app'], 'linux', 'x86_64', SHA)
    value.update(qualification='deterministic-full', outcome='passed', appVersion=expected['version'],
                 appCleanup='passed', globalCleanup='passed', upstreamRevision=expected.get('revision'),
                 upstreamArtifactSha256=expected.get('digest', '').removeprefix('sha256:') or None,
                 probes=[dict(status='passed', steps=sorted(d.STEPS)) for _ in range(3)])
    for key in ('checkerSha256', 'launcherSha256', 'realNanhSha256', 'frozenManifestSha256',
                'preparedSha256', 'reportSha256', 'applicationSha256'):
        value[key] = 'b' * 64
    return value


def run(identifier, workflow='desktop-check-qualification.yml', **overrides):
    return dict(id=identifier, head_sha=SHA, head_branch='main', status='completed',
                event='schedule', path='.github/workflows/' + workflow, conclusion='success',
                created_at=f'2026-10-{identifier:02d}T00:00:00Z', **overrides)


class MaintenanceTests(unittest.TestCase):
    def test_only_exact_supported_missing_evidence_is_selected(self):
        state = lambda entry, prior=None, force=False: d.plan_cell(entry, EXPECTED, 'linux', SHA, prior, force)['state']
        self.assertEqual(state(EXPECTED), 'qualification-pending')
        self.assertEqual(state(EXPECTED, receipt()), 'already-qualified')
        self.assertEqual(state(EXPECTED, receipt(), True), 'qualification-pending')
        self.assertEqual(state({**EXPECTED, 'version': '1.2.4'}, receipt()), 'adaptation-required')
        self.assertEqual(state({**EXPECTED, 'digest': 'sha256:' + 'c' * 64}, receipt()), 'adaptation-required')
        self.assertEqual(state(None, receipt()), 'resolution-failed')
        self.assertEqual(state(dict(status='blocked', app=EXPECTED['app'])), 'resolution-failed')
        for changed in [dict(sourceSha='c' * 40), dict(globalCleanup='failed'), dict(source='release'),
                        dict(upstreamArtifactSha256='c' * 64), dict(probes=receipt()['probes'][:2]),
                        dict(probes=[dict(status='passed', steps=['launched'])] * 3)]:
            self.assertEqual(state(EXPECTED, {**receipt(), **changed}), 'qualification-pending')

    def test_baseline_catalog_covers_exactly_the_twelve_qualified_cells(self):
        for platform in d.TARGETS:
            entries = d.baseline(platform)
            self.assertEqual(set(entries), set(d.APPS))
            for entry in entries.values():
                self.assertTrue(d.same_release(entry, entry))
                self.assertNotIn('pen-desktop', entries)

    def test_release_gate_requires_attested_release_binaries(self):
        workflow = (ROOT / '.github/workflows/release-gate.yml').read_text()
        source = workflow.split('  desktop:', 1)[1].split('  publish:', 1)[0]
        self.assertIn('release_commit: ${{ needs.prepare.outputs.tag_commit }}', source)
        self.assertIn('needs: [prepare, aggregate, desktop]', workflow)
        self.assertIn("needs.desktop.result == 'success'", workflow)
        shared = (ROOT / '.github/workflows/desktop-automation-feasibility.yml').read_text()
        self.assertIn('QUALIFICATION_SOURCE_SHA: ${{ github.sha }}', shared)
        self.assertIn('release_manifest_sha256: ${{ needs.prepare.outputs.desktop_manifest_sha256 }}', source)
        self.assertIn('name: desktop-release-assets', workflow)
        self.assertIn('--verified-manifest-sha256 "$RELEASE_MANIFEST_SHA"', shared)
        native = shared.split('  native:', 1)[1].split('  qualification-matrix:', 1)[0]
        self.assertIn('actions: read', native)
        self.assertNotIn('contents: write', native)
        self.assertIn('desktop_release_binding.py bind', shared)
        self.assertIn('desktop_release.py --repository', shared)
        self.assertIn('--run-id "$GITHUB_RUN_ID" --source-sha "$GITHUB_SHA"', shared)
        self.assertIn('--source-sha "$QUALIFICATION_SOURCE_SHA" --exclude-app pen-desktop', shared)

    def test_source_revision_is_bound_for_hermes(self):
        expected = dict(status='frozen', app='hermes-desktop', version='1.2.3', revision='b' * 40)
        self.assertTrue(d.accepted(receipt(expected), expected, 'linux', SHA))
        self.assertFalse(d.accepted(receipt(expected), {**expected, 'revision': 'c' * 40}, 'linux', SHA))

    def history(self, runs, artifacts):
        def fetch(endpoint):
            if '/workflows/' in endpoint:
                return {'workflow_runs': [r for r in runs if r['path'].split('/')[-1] in endpoint]}
            identifier = int(endpoint.split('/runs/')[1].split('/')[0])
            return {'artifacts': artifacts.get(identifier, [])}
        return fetch

    def test_newer_failed_run_never_falls_back_to_old_success(self):
        old, new = run(1), {**run(2), 'conclusion': 'failure'}
        calls = []
        self.assertEqual(d.previous('owner/repo', 'main', SHA, self.history([old, new], {}),
                                    lambda *a: calls.append(a)), {})
        self.assertFalse(calls)

    def test_empty_successful_daily_does_not_erase_current_qualified_cells(self):
        old, new = run(1), run(2, 'desktop-check-daily.yml')
        artifact = dict(id=11, name='deterministic-qualification-chatgpt-desktop-linux', created_at='2026-10-01T00:00:00Z')
        found = d.previous('owner/repo', 'main', SHA, self.history([old, new], {1: [artifact]}), lambda *a: receipt())
        self.assertEqual(found[('chatgpt-desktop', 'linux')], receipt())

    def test_latest_attempt_is_selected_independently_of_outcome(self):
        name = 'deterministic-qualification-chatgpt-desktop-linux'
        artifacts = [dict(id=5, name=name, created_at='2026-10-01T00:00:00Z'),
                     dict(id=2, name=name, created_at='2026-10-01T01:00:00Z')]
        ids = []
        d.previous('owner/repo', 'main', SHA, self.history([run(1)], {1: artifacts}),
                   lambda repo, artifact, sha, run_id: ids.append(artifact['id']))
        self.assertEqual(ids, [2])

    def test_untrusted_branch_event_commit_and_workflow_are_ignored(self):
        for change in [dict(head_branch='topic'), dict(head_sha='c' * 40),
                       dict(event='pull_request'), dict(path='.github/workflows/foreign.yml')]:
            self.assertEqual(d.previous('owner/repo', 'main', SHA,
                                       self.history([{**run(1), **change}], {})), {})

    def test_artifact_digest_identity_and_exact_membership_are_required(self):
        stream = io.BytesIO()
        with zipfile.ZipFile(stream, 'w') as archive:
            archive.writestr('qualification.json', json.dumps(receipt()))
        raw = stream.getvalue()
        artifact = dict(id=1, expired=False, workflow_run=dict(id=2, head_sha=SHA),
                        size_in_bytes=len(raw), digest='sha256:' + hashlib.sha256(raw).hexdigest())
        with patch.object(d, 'command', return_value=raw):
            self.assertEqual(d.read_artifact('owner/repo', artifact, SHA, 2), receipt())
            for changed in [dict(expired=True), dict(size_in_bytes=len(raw) + 1),
                            dict(digest='sha256:' + 'c' * 64), dict(workflow_run=dict(id=3, head_sha=SHA))]:
                with self.assertRaises(ValueError):
                    d.read_artifact('owner/repo', {**artifact, **changed}, SHA, 2)
        stream = io.BytesIO()
        with zipfile.ZipFile(stream, 'w') as archive:
            archive.writestr('../qualification.json', '{}')
        raw = stream.getvalue()
        with patch.object(d, 'command', return_value=raw), self.assertRaises(ValueError):
            d.read_artifact('owner/repo', {**artifact, 'size_in_bytes': len(raw),
                'digest': 'sha256:' + hashlib.sha256(raw).hexdigest()}, SHA, 2)

    def test_daily_is_read_only_and_does_not_publish_or_supply_provider_credentials(self):
        workflow = (ROOT / '.github/workflows/desktop-check-daily.yml').read_text()
        self.assertIn("cron: '23 6 * * *'", workflow)
        self.assertIn('cells: ${{ needs.plan.outputs.cells }}', workflow)
        self.assertIn("needs.plan.outputs.has_cells == 'true'", workflow)
        for forbidden in ['contents: write', 'secrets:', 'compatibility-publisher', 'NAN_API_KEY']:
            self.assertNotIn(forbidden, workflow)
        full = (ROOT / '.github/workflows/desktop-check-qualification.yml').read_text()
        self.assertIn("cron: '43 7 * * 1'", full)
        self.assertIn("if: github.event_name != 'pull_request'", full)


if __name__ == '__main__':
    unittest.main()
