#!/usr/bin/env python3
"""Producer and ingress agree on exact native release evidence."""

import datetime
import io
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "actions"))
import evidence
import hosted
import hosted_publication
from state import StateError, canonical

COMMIT = "a" * 40
RELEASE = "b" * 40
SPEC = "c" * 64
BINARY = "d" * 64


def report():
    return {"nanHarness": {"version": "1.2.3", "sha256": BINARY, "source": "commit:" + RELEASE},
            "harness": {"id": "codex", "version": "2.0.0"},
            "environment": {"operatingSystem": "linux", "architecture": "aarch64"},
            "checks": [{"name": name, "status": "passed"} for name in
                       ("install-and-diagnose", "deterministic-conformance", "live-tool")],
            "model": "selected-model", "outcome": "passed", "completedAt": "2026-09-12T00:00:00Z"}


class ProducerTests(unittest.TestCase):
    def pack(self, value, command=lambda _: None):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "report.json"
            path.write_bytes(canonical(value))
            return evidence.pack_reports([path], "validator", "cli", "linux", "aarch64",
                                         COMMIT, "v1.2.3", RELEASE, "selected-model", BINARY, SPEC, command)

    def test_data_only_envelope_round_trips_through_actual_ingress(self):
        observed = []
        def validate(argv):
            self.assertEqual(argv[:2], ["validator", "validate-report"])
            observed.append(json.loads(Path(argv[-1]).read_bytes()))
        value = self.pack(report(), validate)
        self.assertEqual(observed, [report()])
        archive = io.BytesIO()
        with zipfile.ZipFile(archive, "w") as output:
            output.writestr("evidence.json", canonical(value))
        received = hosted_publication.artifact_bundle(archive.getvalue())
        self.assertEqual(received, value)
        self.assertEqual(hosted_publication.validate_bundle(received, COMMIT)["architecture"], "aarch64")

    def test_report_identity_drift_and_validator_failure_cannot_produce_an_envelope(self):
        for section, field, replacement in (("nanHarness", "sha256", "e" * 64),
                                             ("nanHarness", "source", "commit:" + COMMIT),
                                             ("environment", "architecture", "x86_64")):
            value = report()
            value[section][field] = replacement
            with self.assertRaises(StateError):
                self.pack(value)
        value = report()
        value["model"] = "another-model"
        with self.assertRaises(StateError):
            self.pack(value)
        def reject(_argv):
            raise StateError("invalid report")
        with self.assertRaises(StateError):
            self.pack(report(), reject)

    def test_selective_driver_requires_both_deterministic_and_selected_live_identity(self):
        item = SimpleNamespace(harness="codex", version="2.0.0", system="linux",
                               architecture="aarch64", model="selected-model")
        updates = hosted.report_update("cli", canonical(report()), SPEC, 123)
        feed = {"schemaVersion": 5, "releases": [updates]}
        now = hosted.instant("2026-09-12T12:00:00Z")
        self.assertEqual(evidence.pending_harnesses(feed, [item], "1.2.3", BINARY, SPEC, "live", now), [])
        item.model = "another-model"
        self.assertEqual(evidence.pending_harnesses(feed, [item], "1.2.3", BINARY, SPEC, "live", now), [item])
        self.assertEqual(evidence.pending_harnesses(feed, [item], "1.2.3", BINARY, SPEC, "deterministic", now), [])
        item.model = "selected-model"
        updates["hostedChecks"][-1]["outcome"] = "blocked"
        self.assertEqual(evidence.pending_harnesses(feed, [item], "1.2.3", BINARY, SPEC, "live", now), [])
        self.assertEqual(evidence.pending_harnesses(feed, [item], "1.2.3", BINARY, SPEC, "live",
                                                  now + datetime.timedelta(days=1)), [item])


if __name__ == "__main__":
    unittest.main()
