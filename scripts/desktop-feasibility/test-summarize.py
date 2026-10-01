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


if __name__ == '__main__':
    unittest.main()
