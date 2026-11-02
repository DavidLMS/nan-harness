#!/usr/bin/env python3
"""Reject parent-only or mismatched child evidence without launching a model."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
import sys
import io
import threading
import urllib.error
from types import SimpleNamespace
from unittest.mock import patch
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "gentle-shell"))
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "actions"))

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


class LiveTransport(unittest.TestCase):
    def test_complete_sse_does_not_wait_for_transport_eof(self):
        from live_provider import forward_body
        class Response:
            headers = {"Content-Type": "text/event-stream"}
            chunks = iter([b'data: {"choices": []}\r\n\r\n', b'data: [DO', b'NE]\r', b'\n\r\n'])
            def read1(self, _size):
                # Reading after DONE would wait for an irrelevant transport EOF.
                return next(self.chunks)
        output = io.BytesIO()
        self.assertTrue(forward_body(Response(), output))
        self.assertTrue(output.getvalue().endswith(b"[DONE]\r\n\r\n"))

    def test_truncated_stream_remains_a_failure(self):
        from live_provider import forward_body
        response = SimpleNamespace(headers={"Content-Type": "text/event-stream"},
                                   read1=io.BytesIO(b'data: {"text":"not done"}\n\n').read)
        self.assertFalse(forward_body(response, io.BytesIO()))

    def test_probe_waits_for_observation_after_cli_receives_final_bytes(self):
        from live_provider import Observation
        observation = Observation()
        forwarded, release = threading.Event(), threading.Event()
        def request():
            with observation.request():
                forwarded.set()
                release.wait(3)
                observation.parent_seen = True
        worker = threading.Thread(target=request)
        worker.start()
        try:
            self.assertTrue(forwarded.wait(3))
            self.assertFalse(observation.wait(timeout=0))
            self.assertFalse(observation.parent_seen)
            release.set()
            self.assertTrue(observation.wait(timeout=3))
            self.assertTrue(observation.parent_seen)
        finally:
            release.set()
            worker.join()

    def test_provider_rejections_reach_client_and_closed_diagnostics(self):
        from live_provider import LiveProvider, Observation
        for status, code in [(401, "provider-auth"), (402, "provider-quota"),
                             (429, "provider-throttled"), (503, "provider-http")]:
            handler = object.__new__(LiveProvider)
            handler.server = SimpleNamespace(key="synthetic", observation=Observation())
            handler.headers = {"Authorization": "Bearer synthetic"}
            handler.path = "/v1/models"
            observed = []
            handler.send_error = observed.append
            error = urllib.error.HTTPError("https://example.test", status, "PRIVATE_SENTINEL", {}, None)
            with patch("urllib.request.urlopen", side_effect=error):
                handler.forward(None)
            self.assertEqual(observed, [status])
            self.assertEqual(handler.server.observation.failures, {code})
            self.assertTrue(handler.server.observation.wait(0))


if __name__ == '__main__':
    unittest.main()
