#!/usr/bin/env python3
"""Closed projections must never retain synthetic private stream contents."""
import copy
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "actions"))
import mimo_live_observation as observation


class ObservationTests(unittest.TestCase):
    def test_noninteractive_prompt_binding_matches_the_pinned_native_transformation(self):
        self.assertEqual(observation.noninteractive_prompt("Read the file"), '\"Read the file\"\n')
        self.assertEqual(observation.noninteractive_prompt("marker"), "marker\n")
        self.assertEqual(observation.noninteractive_prompt('Read "file"'), '\"Read \\\"file\\\"\"\n')
        self.assertEqual(observation.noninteractive_prompt(""), "")

    def test_documented_events_count_only_fixed_predicates(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "capture"
            path.write_text('\n'.join(json.dumps(item) for item in (
                {"type": "tool_use", "part": {"tool": "read", "input": "PRIVATE"}},
                {"type": "tool_use", "part": {"tool": "PRIVATE"}},
                {"type": "text", "part": {"text": "PRIVATE MARKER NAN_CANARY_OK"}},
                {"type": "error", "error": "PRIVATE"},
                {"type": "PRIVATE"})))
            value = observation.stream(path, b"MARKER")
            self.assertEqual(value, dict(status="valid", events=dict(toolUse=2, read=1, text=1, error=1, other=1),
                                         readMarker=True, completionMarker=True, bridgeSentinel=False))
            self.assertNotIn("PRIVATE", json.dumps(value))

    def test_stream_boundaries_cannot_be_confused_with_valid_events(self):
        cases = [(b"", "empty"), (b"   \n", "empty"), (b"PRIVATE", "invalid"),
                 (b'[]', "invalid"), (b'{"type":"text","type":"error"}', "invalid"),
                 (b'\xff', "invalid"), (b'[' * 2000, "invalid"),
                 (b'x' * (observation.MAX_BYTES + 1), "too-large"),
                 (b'{}\n' * (observation.MAX_EVENTS + 1), "too-large")]
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "capture"
            for payload, status in cases:
                with self.subTest(status=status, size=len(payload)):
                    path.write_bytes(payload)
                    value = observation.stream(path, b"MARKER")
                    self.assertEqual(value["status"], status)
                    if status != "empty":
                        self.assertIsNone(value["events"])
                    self.assertNotIn("PRIVATE", json.dumps(value))
            path.unlink()
            self.assertEqual(observation.stream(path, b"MARKER")["status"], "unavailable")
            path.symlink_to(Path(temporary) / "missing")
            self.assertEqual(observation.stream(path, b"MARKER")["status"], "unavailable")

    def test_usage_projects_only_the_existing_schema_and_status(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "usage"
            self.assertEqual(observation.usage(path), "missing")
            for payload, expected in (
                ('{"schemaVersion":1,"status":"observed","private":"PRIVATE"}', 'observed'),
                ('{"schemaVersion":1,"status":"PRIVATE"}', 'other'),
                ('{"schemaVersion":true,"status":"observed"}', 'invalid'),
                ('{"schemaVersion":1,"schemaVersion":1}', 'invalid'), ('PRIVATE', 'invalid')):
                path.write_text(payload)
                self.assertEqual(observation.usage(path), expected)

    def test_consumer_rejects_unknown_fields_types_and_inconsistent_counts(self):
        stream = dict(status="valid", events=dict(toolUse=1, read=1, text=0, error=0, other=0),
                      readMarker=False, completionMarker=False, bridgeSentinel=False)
        value = dict(schemaVersion=1, stage="tool-evidence", stdout=stream, stderr=copy.deepcopy(stream), usage="observed")
        self.assertEqual(observation.validate(value), value)
        changes = [("extra", "PRIVATE"), ("schemaVersion", True), ("stage", "PRIVATE"), ("usage", [])]
        for key, item in changes:
            invalid = {**value, key: item}
            with self.assertRaises(ValueError):
                observation.validate(invalid)
        for key, item in (("events", {**stream["events"], "read": 2}), ("events", {**stream["events"], "text": True}),
                          ("status", "invalid"), ("readMarker", "PRIVATE"), ("extra", "PRIVATE")):
            invalid = {**value, "stdout": {**stream, key: item}}
            with self.assertRaises(ValueError):
                observation.validate(invalid)
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "observation"
            path.write_text(json.dumps(value))
            self.assertEqual(observation.read_observation(path), value)
            path.write_text('{"schemaVersion":1,"schemaVersion":1}')
            self.assertIsNone(observation.read_observation(path))
            path.write_text(" " * 4097)
            self.assertIsNone(observation.read_observation(path))


if __name__ == "__main__":
    unittest.main()
