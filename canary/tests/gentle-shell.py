#!/usr/bin/env python3
"""Reject parent-only or mismatched child evidence without launching a model."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

PATH = Path(__file__).resolve().parents[1] / 'gentle-shell' / 'scenarios.py'
spec = importlib.util.spec_from_file_location('gentle_scenarios', PATH)
scenarios = importlib.util.module_from_spec(spec)
spec.loader.exec_module(scenarios)


class ChildEvidence(unittest.TestCase):
    def test_actual_read_and_model_are_required_in_same_child_session(self):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            marker = scenarios.reader_fixture(home, 'fixture-model')
            output = json.dumps({'type': 'tool_execution_end', 'toolName': 'subagent_run',
                                 'isError': False, 'result': {'content': marker, 'details': {'gentleAgents': {'agent': 'qualification-reader', 'status': 'completed'}}}})
            model = {'type': 'model_change', 'provider': 'nan', 'modelId': 'fixture-model'}
            read = {'type': 'message', 'message': {'role': 'toolResult', 'toolName': 'read',
                    'isError': False, 'content': [{'type': 'text', 'text': marker}]}}
            sessions = home / 'gentle-agents' / 'sessions'
            sessions.mkdir(parents=True)
            child = sessions / 'child.jsonl'
            with self.assertRaises(AssertionError):
                scenarios.verify_child(home, output, marker, 'fixture-model')
            child.write_text(json.dumps(model))
            (sessions / 'other.jsonl').write_text(json.dumps(read))
            with self.assertRaises(AssertionError):
                scenarios.verify_child(home, output, marker, 'fixture-model')
            child.write_text(json.dumps(model) + '\n' + json.dumps(read))
            scenarios.verify_child(home, output, marker, 'fixture-model')
            with self.assertRaises(AssertionError):
                scenarios.verify_child(home, output, marker, 'different-model')
            with self.assertRaises(AssertionError):
                scenarios.verify_child(home, output.replace('false', 'true'), marker, 'fixture-model')
            read['message']['isError'] = True
            child.write_text(json.dumps(model) + '\n' + json.dumps(read))
            with self.assertRaises(AssertionError):
                scenarios.verify_child(home, output, marker, 'fixture-model')

    def test_fixture_is_fresh_and_gateway_summary_is_observable(self):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            first = scenarios.reader_fixture(home, 'qwen3.6')
            second = scenarios.reader_fixture(home, 'qwen3.6')
            self.assertNotEqual(first, second)
            self.assertNotIn(second, scenarios.child_prompt())
        self.assertEqual(scenarios.gateway_requests('40 tokens (4 requests)'), 4)
        self.assertEqual(scenarios.gateway_requests('model says four requests'), 0)


if __name__ == '__main__':
    unittest.main()
