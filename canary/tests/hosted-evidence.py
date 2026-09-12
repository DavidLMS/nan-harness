#!/usr/bin/env python3
"""Behavioral contracts for selective, model-scoped compatibility updates."""

import copy
import datetime
import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "actions"))
import hosted

SPEC = "b" * 64


def report():
    return {"nanHarness": {"version": "0.1.6", "sha256": "a" * 64},
            "harness": {"id": "codex", "version": "0.155.0"},
            "environment": {"operatingSystem": "linux", "architecture": "aarch64"},
            "completedAt": "2026-09-12T00:00:00Z", "model": "qwen3.6", "outcome": "passed",
            "checks": [{"name": name, "status": "passed"} for name in
                       ("install-and-diagnose", "deterministic-conformance", "live-tool")]}


def update(value=None):
    return hosted.report_update("cli", json.dumps(value or report()).encode(), SPEC, 1234)


class HostedEvidenceTests(unittest.TestCase):
    def test_live_evidence_keeps_model_and_never_updates_global_legacy_fields(self):
        value = update()
        self.assertEqual(value["verifications"], [])
        self.assertEqual(len(value["hostedChecks"]), 2)
        self.assertNotIn("model", value["hostedChecks"][0])
        self.assertEqual(value["hostedChecks"][1]["model"], "qwen3.6")
        self.assertTrue(all(check["outcome"] == "passed" for check in value["hostedChecks"]))

    def test_failed_live_or_cleanup_never_promotes_a_success(self):
        value = report()
        value["outcome"] = "failed"
        value["failure"] = {"class": "provider"}
        value["checks"][-1]["status"] = "failed"
        self.assertTrue(all(check["outcome"] == "blocked" for check in update(value)["hostedChecks"]))

    def test_unknown_installed_version_is_not_a_certification(self):
        value = report()
        value["harness"]["version"] = "unknown"
        self.assertEqual(update(value)["hostedChecks"], [])

    def test_selective_checks_bind_every_identity_dimension(self):
        value = update()
        feed = {"schemaVersion": 5, "releases": [value]}
        target = value["hostedChecks"][1]
        now = hosted.instant("2026-09-12T12:00:00Z")
        self.assertFalse(hosted.should_probe(feed, "0.1.6", target, now))
        for field, replacement in (("model", "another-model"), ("harnessVersion", "0.156.0"),
                                   ("platform", "windows"), ("architecture", "x86_64"),
                                   ("nanHarnessSha256", "d" * 64), ("specSha256", "e" * 64)):
            different = {**target, field: replacement}
            self.assertTrue(hosted.should_probe(feed, "0.1.6", different, now), field)
        self.assertTrue(hosted.should_probe(feed, "0.1.7", target, now))

    def test_infrastructure_retry_is_bounded_and_known_failure_waits_for_change(self):
        value = update()
        target = value["hostedChecks"][1]
        target["outcome"] = "blocked"
        feed = {"schemaVersion": 5, "releases": [value]}
        now = hosted.instant(target["checkedAt"])
        self.assertFalse(hosted.should_probe(feed, "0.1.6", target, now + datetime.timedelta(hours=23)))
        self.assertTrue(hosted.should_probe(feed, "0.1.6", target, now + datetime.timedelta(days=1)))
        target["outcome"] = "failed"
        self.assertFalse(hosted.should_probe(feed, "0.1.6", target, now + datetime.timedelta(days=5)))

    def test_old_desktop_live_reports_cannot_invent_a_model(self):
        probe = {"status": "passed"}
        value = {"schemaVersion": 2, "nanHarness": report()["nanHarness"], "platform": "linux",
                 "architecture": "x86_64", "startedAt": "2026-09-12T00:00:00Z", "cleanup": "passed",
                 "results": [{"app": "zed-desktop", "appVersion": "0.204.0", "cleanup": "passed",
                              "deterministic": [probe] * 3, "live": probe}]}
        with self.assertRaises(ValueError):
            hosted.report_update("desktop", json.dumps(value).encode(), SPEC, 1234)
        value["schemaVersion"], value["model"] = 3, "selected-model"
        result = hosted.report_update("desktop", json.dumps(value).encode(), SPEC, 1234)
        self.assertEqual(result["hostedChecks"][1]["model"], "selected-model")
        bad = copy.deepcopy(value)
        bad["cleanup"] = "failed"
        result = hosted.report_update("desktop", json.dumps(bad).encode(), SPEC, 1234)
        self.assertTrue(all(check["outcome"] == "blocked" for check in result["hostedChecks"]))


if __name__ == "__main__":
    unittest.main()
