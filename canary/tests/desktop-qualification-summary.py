#!/usr/bin/env python3
"""Closed Desktop summary semantics and retry-policy integration."""
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'canary/actions'))
import desktop_qualification_summary as summary


def report():
    return dict(schemaVersion=1, app='hermes-desktop', platform='windows', outcome='blocked',
                reason='incomplete-acceptance', appCleanup='passed', globalCleanup='passed',
                probes=[dict(status='passed'), dict(status='blocked', reason='isolation-unavailable'),
                        dict(status='passed')], nativeDiagnostics=[dict(kind='native', record=dict(
                            probeIndex=1, launchStage='not-started'))], semanticObservations=[
                    dict(mechanism='hermes-windows-catalog-readiness', stage='menu', errorCategory='menu-unavailable'),
                    dict(mechanism='semantic-provider-oracle', stage='failure', failureObserved=True,
                         fixtureResponseVerified=False),
                    dict(mechanism='semantic-provider-oracle', stage='recovery', failureObserved=True,
                         fixtureResponseVerified=True)])


class SummaryTests(unittest.TestCase):
    def test_failed_probe_and_expected_injection_are_distinct(self):
        text = summary.render(report())
        self.assertIn('| 2 | blocked | isolation-unavailable | not-started |', text)
        self.assertIn('Stage **menu**; category **menu-unavailable**.', text)
        self.assertIn('1 expected injected failures observed; 1 verified recovery responses; 0 unverified recovery responses', text)
        self.assertIn('Retry policy: **ineligible**', text)
        self.assertIn('Cleanup: application **passed**; global **passed**', text)

    def test_failed_recovery_is_never_reported_as_expected_injection(self):
        value = report()
        value['semanticObservations'][-1]['fixtureResponseVerified'] = False
        text = summary.render(value)
        self.assertIn('0 verified recovery responses; 1 unverified recovery responses', text)

    def test_retry_eligibility_uses_runner_policy(self):
        value = report()
        value['probes'][1] = dict(status='failed', reason='action-unsupported')
        self.assertIn('Retry policy: **eligible**', summary.render(value))
        value['globalCleanup'] = 'failed'
        self.assertIn('Retry policy: **ineligible**', summary.render(value))
        with patch.object(summary._runner, 'retryable_qualification', return_value=True) as policy:
            self.assertIn('Retry policy: **eligible**', summary.render(value))
            policy.assert_called_once_with(value)

    def test_private_and_unrecognized_values_never_reach_markdown(self):
        value = report()
        value['probes'][1]['reason'] = '<PRIVATE_MARKDOWN>'
        value['nativeDiagnostics'][0]['record']['launchStage'] = '<PRIVATE_MARKDOWN>'
        value['semanticObservations'][0].update(stage='<PRIVATE_MARKDOWN>', errorCategory='<PRIVATE_MARKDOWN>')
        value['semanticObservations'].append(dict(mechanism='unknown', text='<PRIVATE_MARKDOWN>'))
        value['globalCleanup'] = '<PRIVATE_MARKDOWN>'
        value['unrecognized'] = '<PRIVATE_MARKDOWN>'
        self.assertNotIn('PRIVATE', summary.render(value))
        self.assertIn('| 2 | blocked | unavailable | unavailable |', summary.render(value))

    def test_invalid_shapes_and_budgets_are_rejected(self):
        for change in ({'app': 'PRIVATE'}, {'schemaVersion': True}, {'probes': ['PRIVATE']},
                       {'probes': [{}] * 4}, {'semanticObservations': [{}] * 97}):
            with self.subTest(change=change), self.assertRaises(ValueError):
                summary.render({**report(), **change})

    def test_menu_timeout_is_distinct_from_isolation_loss(self):
        value = report()
        value['semanticObservations'][0]['errorCategory'] = 'menu-timeout'
        self.assertIn('category **menu-timeout**', summary.render(value))

    def test_missing_or_invalid_report_writes_only_fixed_fallback(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            report_path, output = root / 'report.json', root / 'summary.md'
            argv = ['summary', '--report', str(report_path), '--output', str(output)]
            for content in (None, '{PRIVATE_INVALID_JSON', '"PRIVATE_VALUE"'):
                if content is not None:
                    report_path.write_text(content)
                with patch.object(sys, 'argv', argv):
                    summary.main()
            self.assertNotIn('PRIVATE', output.read_text())
            self.assertEqual(output.read_text().count('Reduced evidence is absent'), 3)

    def test_missing_evidence_is_not_reported_as_success(self):
        value = report()
        value.update(probes=[], semanticObservations=[], nativeDiagnostics=None,
                     appCleanup=None, globalCleanup=None)
        text = summary.render(value)
        self.assertIn('No probe evidence was produced.', text)
        self.assertIn('application **unavailable**; global **unavailable**', text)
        self.assertIn('Retry policy: **ineligible**', text)


if __name__ == '__main__':
    unittest.main()
