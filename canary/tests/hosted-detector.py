#!/usr/bin/env python3
"""Read-only detector resolution never turns an API failure into an empty feed."""

from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "canary/actions"))
import detector
from state import StateError


class DetectorTests(unittest.TestCase):
    def test_current_feed_wins_and_only_model_scoped_backups_are_eligible(self):
        old = {"name": "compatibility-v4.json", "created_at": "2026-09-12"}
        first = {"name": "compatibility-v5.json.backup.a", "created_at": "2026-09-10"}
        last = {"name": "compatibility-v5.json.backup.b", "created_at": "2026-09-11"}
        current = {"name": "compatibility-v5.json"}
        self.assertIsNone(detector.feed_asset([old]))
        self.assertEqual(detector.feed_asset([old, last, first]), last["name"])
        self.assertEqual(detector.feed_asset([last, current]), current["name"])
        with self.assertRaises(StateError):
            detector.feed_asset([current, current])

    def test_api_failure_cannot_fabricate_an_empty_validated_feed(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "feed.json"
            argv = ["detector", "feed", "--repository", "owner/repository",
                    "--output", str(output), "--validator", "/trusted/xtask"]
            with patch.object(sys, "argv", argv), patch.object(detector, "Store") as store, \
                    patch.object(detector, "validate") as validate:
                store.return_value.call.side_effect = StateError("unavailable")
                self.assertEqual(detector.main(), 1)
                validate.assert_not_called()
                self.assertFalse(output.exists())

    def test_legacy_feed_requires_positive_api_observation_and_trusted_validation(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "feed.json"
            argv = ["detector", "feed", "--repository", "owner/repository",
                    "--output", str(output), "--validator", "/trusted/xtask"]
            with patch.object(sys, "argv", argv), patch.object(detector, "Store") as store, \
                    patch.object(detector, "validate") as validate:
                store.return_value.call.return_value = {"assets": [{"name": "compatibility-v4.json"}]}
                self.assertEqual(detector.main(), 0)
                self.assertEqual([call.args[0][1] for call in validate.call_args_list],
                                 ["hosted-compatibility-feed", "validate-hosted-compatibility-feed"])

    def test_workflow_keeps_one_schedule_and_credentials_out_of_setup(self):
        workflow = (ROOT / ".github/workflows/harness-canary.yml").read_text()
        self.assertEqual(workflow.count('cron: "47 5 * * *"'), 1)
        self.assertEqual(workflow.count("cron:"), 1)
        self.assertEqual(workflow.count("NAN_API_KEY:"), 1)
        setup, live = workflow.split("      - name: Check selected harnesses", 1)
        self.assertNotIn("NAN_API_KEY", setup)
        self.assertIn("canary-live", setup)
        self.assertNotIn("contents: write", workflow)
        self.assertIn("hosted-evidence-cli-${{ matrix.system }}", live)
        self.assertIn("evidence.py pack", live)
        self.assertIn("persist-credentials: false", setup)


if __name__ == "__main__":
    unittest.main()
