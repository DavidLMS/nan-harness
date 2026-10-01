#!/usr/bin/env python3
"""Closed evidence rejects raw data and impossible successful transitions."""
import copy
import sys
import json
import subprocess
import tempfile
from pathlib import Path
import unittest
sys.path.insert(0, str(Path(__file__).resolve().parent))
from experiment_facts import dom, native_copy

class ClosedFacts(unittest.TestCase):
    def setUp(self):
        self.dom = dict(schemaVersion=1, mechanism='hermes-playwright-dom', endpointOwned=True,
                        attached=True, uniqueComposer=True, inputReadback=True,
                        syntheticTextPresent=True, targetVerified=True, responseVerified=True,
                        inputSubmitted=True, errorCategory=None, playwrightVersion='1.61.1',
                        observedRuntimeVersion='144.0.0.0', providerResponseVerified=True)
        self.copy = dict(schemaVersion=1, mechanism='zed-native-copy', experimentOnly=True,
                         ocrUsed=False, axTextUsed=False, navigation='private-keymap-new-thread',
                         stage='completed', blocker=None, trustControlCount=0, panelControlCount=1,
                         responseControlCount=1, clipboardCleanup='passed',
                         input=dict(entered=True, clipboardVerified=True, submitted=True),
                         response=dict(copyAction=True, clipboardVerified=True, providerVerified=True))

    def test_success_and_interrupted_provider_fact(self):
        dom(self.dom)
        native_copy(self.copy, set())
        del self.dom['providerResponseVerified']
        self.assertFalse(dom(self.dom)['providerResponseVerified'])

    def test_reducer_retains_closed_facts_without_private_connection(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            facts = root / 'facts'
            facts.mkdir()
            for index in range(3):
                (facts / f'dom-{index}.json').write_text(json.dumps(self.dom))
            (facts / 'connection-1.json').write_text(json.dumps({'port': 12345, 'launcherPid': 123456}))
            report = root / 'report.json'
            report.write_text(json.dumps({'cleanup': 'passed', 'results': [{'app': 'hermes-desktop', 'cleanup': 'passed'}]}))
            frozen = root / 'frozen.json'
            frozen.write_text('{}')
            output = root / 'summary.json'
            command = [sys.executable, '-B', str(Path(__file__).with_name('summarize.py')),
                       '--report', str(report), '--frozen', str(frozen), '--real-nanh', '/absent',
                       '--checker', '/absent', '--shim', str(Path(__file__).with_name('nanh-shim.py')),
                       '--output', str(output), '--source-sha', 'a' * 40, '--platform', 'linux-x86_64',
                       '--app', 'hermes-desktop', '--facts', str(facts)]
            subprocess.run(command, check=True)
            result = json.loads(output.read_text())
            self.assertTrue(result['domReadbackObserved'])
            self.assertFalse(result['domDrivenQualification'])
            self.assertNotIn('launcherPid', output.read_text())
            self.assertNotIn('12345', output.read_text())
            report.write_text(json.dumps({'cleanup': 'failed', 'results': []}))
            subprocess.run(command, check=True)
            self.assertFalse(json.loads(output.read_text())['domReadbackObserved'])

    def test_dom_rejects_private_data_and_false_success(self):
        for key, value in [('rawText', 'private'), ('endpointOwned', False),
                           ('inputReadback', False), ('errorCategory', 'response-timeout'),
                           ('schemaVersion', True), ('observedRuntimeVersion', 'private/url')]:
            facts = copy.deepcopy(self.dom)
            facts[key] = value
            with self.assertRaises(ValueError):
                dom(facts)

    def test_copy_rejects_ocr_raw_data_and_unverified_submission(self):
        for mutation in [lambda f: f.update(rawClipboard='private'),
                         lambda f: f.update(ocrUsed=True),
                         lambda f: f['input'].update(clipboardVerified=False),
                         lambda f: f['response'].update(copyAction=False)]:
            facts = copy.deepcopy(self.copy)
            mutation(facts)
            with self.assertRaises(ValueError):
                native_copy(facts, set())

if __name__ == '__main__':
    unittest.main()
