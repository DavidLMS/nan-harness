#!/usr/bin/env python3
"""Reject OCR-assisted, partial, and unclean probes as semantic evidence."""
import copy
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('summarize', Path(__file__).with_name('summarize.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class SemanticEvidence(unittest.TestCase):
    def setUp(self):
        self.report = {'cleanup': 'passed', 'results': [{'app': 'zed-desktop', 'cleanup': 'passed',
            'deterministic': [dict(status='passed', inputMode='accessibility-and-keyboard',
                                   responseVerification='accessibility') for _ in range(3)]}]}

    def test_all_semantic_stages_and_cleanup(self):
        self.assertTrue(module.semantic_zed(self.report))

    def test_ocr_or_partial_or_unclean_is_not_semantic(self):
        for field, value in [('inputMode', 'visual-and-keyboard'),
                             ('responseVerification', 'local-ocr'), ('status', 'blocked')]:
            report = copy.deepcopy(self.report)
            report['results'][0]['deterministic'][1][field] = value
            self.assertFalse(module.semantic_zed(report))
        self.report['results'][0]['deterministic'].pop()
        self.assertFalse(module.semantic_zed(self.report))

    def test_missing_and_unclean_fail_closed(self):
        self.assertFalse(module.semantic_zed({}))
        self.report['cleanup'] = 'failed'
        self.assertFalse(module.semantic_zed(self.report))


class AccessibilityPrivacy(unittest.TestCase):
    def setUp(self):
        self.facts = dict(schemaVersion=1, mechanism='zed-native-accessibility', experimentOnly=True,
                          noOcrQualification=False, appByPid=True, appError=None,
                          stage='after-panel', trustControlCount=0, trustControlError=None,
                          blocker='selector-not-matched', keyboardEntered=False,
                          semanticInputVerified=False, semanticResponseVerified=False,
                          providerResponseVerified=False, inventories=[
                              dict(stage='after-panel', roleCounts={'window': 1}, rolesError=None,
                                   namedComposerCount=0, namedComposerError=None,
                                   editableCount=0, editableError=None,
                                   valueReadback='no-matching-control', responseMatches=0, responseError=None)])

    def test_zero_matches_are_distinct_from_failed_query(self):
        module.validate_ax(self.facts)
        observation = self.facts['inventories'][0]
        observation['editableCount'] = None
        observation['editableError'] = 'action-unsupported'
        module.validate_ax(self.facts)
        self.assertIsNone(observation['editableCount'])

    def test_raw_text_false_qualification_and_overflow_are_rejected(self):
        mutations = [lambda facts: facts.update(rawTree='synthetic-private-text'),
                     lambda facts: facts.update(noOcrQualification=True),
                     lambda facts: facts.update(schemaVersion=True),
                     lambda facts: facts['inventories'][0]['roleCounts'].update(synthetic_private_label=1),
                     lambda facts: facts['inventories'][0].update(editableCount=4097)]
        for mutation in mutations:
            facts = copy.deepcopy(self.facts)
            mutation(facts)
            with self.assertRaises(ValueError):
                module.validate_ax(facts)


if __name__ == '__main__':
    unittest.main()
