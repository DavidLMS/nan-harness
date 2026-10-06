#!/usr/bin/env python3
"""Desktop publication contracts. All GitHub operations use a local fake."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
COMMIT = "a" * 40
DIGEST = "b" * 64


def fake_github():
    args = sys.argv[1:]
    directory = Path(os.environ["DESKTOP_TEST_REMOTE"])
    fixtures = Path(os.environ["DESKTOP_TEST_FIXTURES"])
    with (fixtures / "calls.jsonl").open("a") as log:
        log.write(json.dumps(args) + "\n")
    failure = os.environ.get("DESKTOP_TEST_FAILURE")
    if args[:2] == ["attestation", "verify"]:
        assert args[args.index("--source-digest") + 1] == COMMIT
        assert "--deny-self-hosted-runners" in args
        return 1 if failure == "attestation" else 0
    if args[0] == "api":
        endpoint = args[1]
        if "/git/ref/tags/" in endpoint:
            print("commit\t" + COMMIT)
        elif "/contents/" in endpoint:
            if failure == "registry":
                return 1
            assert endpoint.endswith("?ref=" + COMMIT)
            print((fixtures / "registry.json").read_text())
        elif "/releases/tags/compatibility" in endpoint:
            print('HTTP/2 200 OK\n\n{}')
        else:
            return 1
        return 0
    if args[:2] == ["release", "view"]:
        if args[2] == "compatibility":
            print(json.dumps({"assets": [{"name": p.name, "createdAt": "2026-09-08T00:00:00Z"}
                                        for p in directory.iterdir() if p.is_file()]}))
        else:
            print(json.dumps({"tagName": args[2], "isDraft": failure == "draft", "isPrerelease": False}))
        return 0
    if args[:2] == ["release", "download"]:
        name = args[args.index("--pattern") + 1]
        source = fixtures / name if args[2] != "compatibility" else directory / name
        if not source.is_file():
            return 1
        shutil.copyfile(source, args[args.index("--output") + 1])
        return 0
    if args[:2] == ["release", "upload"]:
        source = Path(args[3])
        failed_asset = os.environ.get("DESKTOP_TEST_UPLOAD_ASSET", "compatibility-v4.json")
        if failure == "upload" and source.name == failed_asset and not (fixtures / "failed").exists():
            (fixtures / "failed").touch()
            return 1
        shutil.copyfile(source, directory / source.name)
        return 0
    if args[:2] == ["release", "delete-asset"]:
        (directory / args[3]).unlink()
        return 0
    return 1


class DesktopPublication(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.work = Path(self.temporary.name)
        self.remote = self.work / "remote"
        self.fixtures = self.work / "fixtures"
        self.updates = self.work / "updates"
        self.bin = self.work / "bin"
        for directory in (self.remote, self.fixtures, self.updates, self.bin):
            directory.mkdir()
        shutil.copyfile(__file__, self.bin / "gh")
        (self.bin / "gh").chmod(0o700)
        shutil.copyfile(ROOT / "crates/nan-harness-runtime/resources/desktop-compatibility.json", self.fixtures / "registry.json")
        (self.fixtures / "SHA256SUMS").write_text(DIGEST + "  nan-harness-aarch64-apple-darwin\n")
        self.report = self.work / "report.json"
        self.report.write_text(json.dumps({"nanHarness": {"version": "0.0.1", "sha256": DIGEST},
                                           "platform": "macos", "architecture": "aarch64"}))
        self.update = {"nanHarnessVersion": "0.0.1", "verifications": [], "desktopChecks": [{
            "id": "zed-desktop", "platform": "macos", "architecture": "aarch64",
            "appVersion": "1.19.0", "deterministicAt": "2026-09-08T00:00:00Z"}]}
        self.write_update()
        legacy = {"schemaVersion": 2, "releases": [{"nanHarnessVersion": "0.0.1", "verifications": []}]}
        (self.remote / "compatibility.json").write_text(json.dumps(legacy))
        legacy["schemaVersion"] = 3
        (self.remote / "compatibility-v3.json").write_text(json.dumps(legacy))
        self.legacy_bytes = {name: (self.remote / name).read_bytes() for name in ("compatibility.json", "compatibility-v3.json")}

    def write_update(self):
        (self.updates / "desktop.json").write_text(json.dumps(self.update))

    def publish(self, failure=None, checkpoint=None):
        environment = dict(os.environ, PATH=str(self.bin) + os.pathsep + os.environ["PATH"],
                           DESKTOP_TEST_REMOTE=str(self.remote), DESKTOP_TEST_FIXTURES=str(self.fixtures),
                           NAN_CANARY_RETRY_DELAY_SECONDS="0", NAN_CANARY_WRITER="actions", GITHUB_ACTIONS="true")
        if failure:
            environment["DESKTOP_TEST_FAILURE"] = failure
        if checkpoint:
            environment["NAN_CANARY_PUBLICATION_FAIL_PHASE"] = checkpoint
        return subprocess.run(["bash", str(ROOT / "canary/actions/publish-desktop.sh"),
                               "--report", str(self.report), "--updates", str(self.updates),
                               "--repository", "example/nan-harness"], env=environment,
                              capture_output=True, text=True, timeout=180)

    def assert_legacy_preserved(self):
        for name, expected in self.legacy_bytes.items():
            self.assertEqual((self.remote / name).read_bytes(), expected)

    def test_first_publication_and_replay_preserve_old_assets(self):
        result = self.publish()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((self.remote / "compatibility-v4.json").exists(), result.stdout + (self.fixtures / "calls.jsonl").read_text())
        feed = json.loads((self.remote / "compatibility-v4.json").read_text())
        self.assertEqual(feed["releases"][0]["desktopChecks"], self.update["desktopChecks"])
        self.assert_legacy_preserved()
        before = (self.remote / "compatibility-v4.json").read_bytes()
        result = self.publish()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.remote / "compatibility-v4.json").read_bytes(), before)

    def test_unverified_binary_attestation_and_registry_fail_before_writes(self):
        for failure in ("draft", "attestation", "registry"):
            with self.subTest(failure=failure):
                result = self.publish(failure)
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse((self.remote / "compatibility-v4.json").exists())
                self.assert_legacy_preserved()
        report = json.loads(self.report.read_text())
        report["nanHarness"]["sha256"] = "c" * 64
        self.report.write_text(json.dumps(report))
        self.assertNotEqual(self.publish().returncode, 0)
        self.assertFalse((self.remote / "compatibility-v4.json").exists())

    def test_historical_registry_rejects_incomplete_pair(self):
        check = self.update["desktopChecks"][0]
        check["id"] = "chatgpt-desktop"
        check["appVersion"] = "26.900.0"
        self.write_update()
        self.assertNotEqual(self.publish().returncode, 0)
        self.assertFalse((self.remote / "compatibility-v4.json").exists())

    def test_failed_upload_restores_previous_feed(self):
        self.assertEqual(self.publish().returncode, 0)
        before = (self.remote / "compatibility-v4.json").read_bytes()
        self.update["desktopChecks"][0]["liveVerifiedAt"] = "2026-09-08T01:00:00Z"
        self.write_update()
        self.assertNotEqual(self.publish("upload").returncode, 0)
        self.assertEqual((self.remote / "compatibility-v4.json").read_bytes(), before)
        self.assert_legacy_preserved()

    def test_deleted_stable_asset_recovers_its_backup(self):
        self.assertEqual(self.publish().returncode, 0)
        self.assertNotEqual(self.publish(checkpoint="after-stable-delete").returncode, 0)
        self.assertFalse((self.remote / "compatibility-v4.json").exists())
        result = self.publish()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((self.remote / "compatibility-v4.json").is_file())
        self.assert_legacy_preserved()


if __name__ == "__main__":
    if Path(sys.argv[0]).name == "gh":
        sys.exit(fake_github())
    unittest.main()
