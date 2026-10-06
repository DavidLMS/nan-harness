#!/usr/bin/env python3
"""Owned synthetic AT-SPI cache contracts; no native applications."""
from pathlib import Path
import runpy
import sys
import unittest

module = runpy.run_path(str(Path(__file__).with_name('zed-overlay-census.py')))
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'canary/actions'))
from desktop_qualification import validate_overlay_census


def item(path, name='PRIVATE', role=43, states=(0, 0)):
    return ((':1.2', path), (':1.2', '/root'), (':1.2', '/root'), 0, 0,
            ['org.a11y.atspi.Accessible'], name, role, 'PRIVATE description', states)


class CensusTests(unittest.TestCase):
    def test_classifies_labels_roles_and_state_without_retaining_unknown_text(self):
        result = module['classify']([item('/retry', 'Retry'), item('/cancel', 'Cancel'),
            item('/unknown'), item('/dialog', role=16, states=(1 << 16, 0)),
            item('/alert', role=2, states=(1 << 12, 0))], ':1.2', '/retry')
        self.assertEqual(result['counts'], dict(nodes=5, dialogs=1, alerts=1,
            modalNodes=1, focusedNodes=1, buttons=3))
        self.assertEqual(result['actions']['retry'], 1)
        self.assertEqual(result['actions']['cancel'], 1)
        self.assertEqual(result['actions']['other'], 1)
        self.assertNotIn('PRIVATE', str(result))
        self.assertEqual(validate_overlay_census(result), result)

    def test_rejects_mixed_owners_missing_target_duplicates_and_unbounded_cache(self):
        original = item('/retry', 'Retry')
        for values, owner, held in [([], ':1.2', '/retry'), ([original] * 1025, ':1.2', '/retry'),
                ([original], ':1.3', '/retry'), ([original], ':1.2', '/absent'),
                ([original, original], ':1.2', '/retry'), ([original[:-1]], ':1.2', '/retry'),
                ([item('/retry', states=(0xffffffff + 1, 0))], ':1.2', '/retry')]:
            with self.assertRaises(ValueError):
                module['classify'](values, owner, held)

    def test_reducer_rejects_raw_content_inconsistent_counts_and_partial_success(self):
        absent = module['unavailable']()
        self.assertEqual(validate_overlay_census(absent), absent)
        good = module['classify']([item('/retry', 'Retry')], ':1.2', '/retry')
        for changed in ({**absent, 'actions': {}}, {**good, 'raw': 'PRIVATE'},
                {**good, 'counts': {**good['counts'], 'nodes': True}},
                {**good, 'counts': {**good['counts'], 'buttons': 0}},
                {**good, 'actions': {**good['actions'], 'PRIVATE': 1}}):
            with self.assertRaises(ValueError):
                validate_overlay_census(changed)


if __name__ == '__main__':
    unittest.main()
