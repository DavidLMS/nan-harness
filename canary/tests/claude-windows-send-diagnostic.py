#!/usr/bin/env python3
"""Closed, observation-only Windows Send transport evidence."""
import copy
import json
from pathlib import Path
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'canary/actions'))
import desktop_qualification as q


def facts(observation):
    return dict(schemaVersion=1, mechanism='claude-windows-native-chat', diagnosticsOnly=True,
                stage='action-uncertain', actionPhase='transport', transportFailure='timeout',
                nativeAction='input-replace-owned', nativeProgress='send', nativeWorkProgress='send',
                submittedTurns=2, inputVerifiedTurns=2, copiedResponses=2, retryAttempted=False,
                clipboardCleared=True, nativeSendObservation=observation)


class WindowsSendDiagnosticTests(unittest.TestCase):
    def project(self, value, platform='windows'):
        with tempfile.TemporaryDirectory() as directory:
            (Path(directory) / 'closed.json').write_text(json.dumps(value))
            return q.semantic_observations(directory, 'claude-desktop', platform)

    def test_closed_phases_do_not_certify_a_submission(self):
        for phase in ('paste-check', 'paste-checked', 'tree-scan', 'tree-scanned',
                      'candidate-check', 'target-check', 'release', 'released'):
            value = facts(dict(phase=phase, completedScans=2, pendingScans=1))
            for failure in ('timeout', 'nonzero-exit', 'output', 'pipe'):
                value['transportFailure'] = failure
                self.assertEqual(self.project(value), [value])
        legacy = facts(None)
        del legacy['nativeSendObservation']
        self.assertEqual(self.project(legacy), [legacy])

    def test_bad_wire_objects_counts_and_private_values_are_rejected(self):
        valid = dict(phase='tree-scan', completedScans=2, pendingScans=1)
        malformed = [None, [], {}, valid | {'rawText': 'PRIVATE'}, valid | {'phase': {'tree-scan': None}},
                     valid | {'phase': 'PRIVATE'}, valid | {'completedScans': True},
                     valid | {'completedScans': -1}, valid | {'pendingScans': 3},
                     valid | {'completedScans': 1025}, valid | {'pendingScans': None}]
        for observation in malformed:
            with self.subTest(observation=observation), self.assertRaises(ValueError):
                self.project(facts(observation))

    def test_other_actions_success_receipts_and_platforms_cannot_adopt_observation(self):
        value = facts(dict(phase='release', completedScans=2, pendingScans=1))
        changes = [dict(mechanism='claude-native-chat'), dict(nativeAction='retry'),
                   dict(nativeStage='sent'), dict(nativeStage=None), dict(stage='completed'),
                   dict(transportFailure=None), dict(actionPhase='completed'), dict(retryAttempted=True),
                   dict(submittedTurns=3, inputVerifiedTurns=3, copiedResponses=3), dict(copiedResponses=1)]
        for change in changes:
            with self.subTest(change=change), self.assertRaises(ValueError):
                self.project(copy.deepcopy(value) | change)
        for platform in (None, 'linux', 'macos'):
            with self.assertRaises(ValueError):
                self.project(value, platform)

    def test_actual_cpp_producer_through_rust_projection(self):
        fixture = ROOT / 'canary/tests/fixtures/claude-windows-send-observation.json'
        observations = json.loads(fixture.read_text())
        self.assertEqual(len(observations), 14)
        self.assertEqual(observations[2], dict(phase='tree-scan', completedScans=0, pendingScans=0))
        self.assertEqual(observations[8], dict(phase='tree-scan', completedScans=1, pendingScans=1))
        self.assertEqual(observations[-1], dict(phase='released', completedScans=2, pendingScans=1))
        for observation in observations:
            value = facts(observation)
            self.assertEqual(self.project(value), [value])
        self.assertNotIn('PRIVATE', fixture.read_text())


if __name__ == '__main__':
    unittest.main()
