#!/usr/bin/env python3
"""Exercise the v5 writer against a local fake, including interrupted replacement."""

import importlib.util
import json
import os
from pathlib import Path
import subprocess
import unittest

SPEC = importlib.util.spec_from_file_location("desktop_publication", Path(__file__).with_name("desktop-publication.py"))
fixtures = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(fixtures)


class HostedPublication(unittest.TestCase):
    def setUp(self):
        self.fixture = fixtures.DesktopPublication()
        self.fixture.setUp()
        self.addCleanup(self.fixture.doCleanups)
        self.check = {"suite": "cli", "id": "codex", "platform": "linux", "architecture": "aarch64",
                      "harnessVersion": "0.155.0", "model": "qwen3.6", "outcome": "passed",
                      "checkedAt": "2026-09-12T00:00:00Z", "nanHarnessSha256": "a" * 64,
                      "specSha256": "b" * 64, "evidenceSha256": "c" * 64, "sourceRun": 1234}
        self.write_update()

    def write_update(self):
        self.fixture.update = {"nanHarnessVersion": "0.0.1", "verifications": [],
                               "hostedChecks": [self.check]}
        self.fixture.write_update()

    def run_writer(self, publish=False, failure="", checkpoint=""):
        fixture = self.fixture
        environment = dict(os.environ, PATH=str(fixture.bin) + os.pathsep + os.environ["PATH"],
                           DESKTOP_TEST_REMOTE=str(fixture.remote), DESKTOP_TEST_FIXTURES=str(fixture.fixtures),
                           DESKTOP_TEST_FAILURE=failure, DESKTOP_TEST_UPLOAD_ASSET="compatibility-v5.json",
                           NAN_CANARY_RETRY_DELAY_SECONDS="0", NAN_CANARY_WRITER="actions", GITHUB_ACTIONS="true",
                           NAN_CANARY_PUBLICATION_FAIL_PHASE=checkpoint)
        command = ["bash", str(fixtures.ROOT / "canary/actions/publish-hosted.sh"),
                   "--updates", str(fixture.updates), "--registry", str(fixture.fixtures / "registry.json"),
                   "--version", "0.0.1", "--repository", "example/nan-harness",
                   "--output", str(fixture.work / "candidate.json")]
        if publish:
            command.append("--publish")
        return subprocess.run(command, env=environment, capture_output=True, text=True, timeout=180)

    def test_default_dry_run_cannot_mutate_remote_assets(self):
        before = {p.name: p.read_bytes() for p in self.fixture.remote.iterdir()}
        result = self.run_writer()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(before, {p.name: p.read_bytes() for p in self.fixture.remote.iterdir()})
        calls = [json.loads(line) for line in (self.fixture.fixtures / "calls.jsonl").read_text().splitlines()]
        self.assertTrue(all(call[:2] not in (["release", "upload"], ["release", "delete-asset"],
                                           ["release", "create"]) for call in calls))

    def test_replay_and_failed_replacement_preserve_last_good_and_old_clients(self):
        result = self.run_writer(publish=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        stable = self.fixture.remote / "compatibility-v5.json"
        before = stable.read_bytes()
        self.assertEqual(self.run_writer(publish=True).returncode, 0)
        self.assertEqual(stable.read_bytes(), before)
        self.check.update(outcome="failed", checkedAt="2026-09-12T01:00:00Z", evidenceSha256="d" * 64)
        self.write_update()
        self.assertNotEqual(self.run_writer(publish=True, failure="upload").returncode, 0)
        self.assertEqual(stable.read_bytes(), before)
        self.fixture.assert_legacy_preserved()

    def test_interrupted_swap_uses_backup_instead_of_discarding_v5_history(self):
        self.assertEqual(self.run_writer(publish=True).returncode, 0)
        self.assertNotEqual(self.run_writer(publish=True, checkpoint="after-stable-delete").returncode, 0)
        stable = self.fixture.remote / "compatibility-v5.json"
        self.assertFalse(stable.exists())
        self.check.update(model="another-model", evidenceSha256="d" * 64)
        self.write_update()
        result = self.run_writer(publish=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        checks = json.loads(stable.read_bytes())["releases"][0]["hostedChecks"]
        self.assertEqual({check["model"] for check in checks}, {"qwen3.6", "another-model"})
        self.fixture.assert_legacy_preserved()


if __name__ == "__main__":
    unittest.main()
