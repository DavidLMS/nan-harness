#!/usr/bin/env python3
"""Exercise diagnostic staging with the real checker, without launching an app."""

import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parent
CHECKER = Path(os.environ.get("CHECKER", ROOT.parent / "target/debug/nanh-desktop-check")).resolve()
REDUCER = ROOT / "chatgpt-wave12-reducer.py"
spec = importlib.util.spec_from_file_location("wave12_reducer", REDUCER)
reducer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reducer)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


class StagingTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="chatgpt-stage-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.wrapper = self.root / "wrapper.sh"
        self.nanh = self.root / "nanh"
        for executable in (self.wrapper, self.nanh):
            executable.write_text("#!/bin/sh\nexit 0\n")
            executable.chmod(0o700)
        bounds = {"maxStreamBytes": 1048576, "maxLineBytes": 8192,
                  "maxLines": 65535, "deadlineSeconds": 60, "graceSeconds": 5}
        self.facts = reducer.refusal_facts("runtime-refused", bounds, None)
        self.facts.update(failure="none", observation="complete", classification="no-signature",
                          launcherDisposition="exited", launcherExit=0)
        self.facts["identity"] = {"realNanhSha256": digest(self.nanh),
                                  "shimSha256": digest(self.wrapper),
                                  "reducerSha256": digest(REDUCER)}
        self.assertIsNone(reducer.validate_facts(self.facts))
        probe = {"status": "blocked", "reason": "not-run", "steps": [],
                 "durationMilliseconds": 0}
        report = {
            "schemaVersion": 2, "checkerVersion": "0.1.0",
            "runId": "22222222222222222222222222222222", "startedAt": "2026-09-10T12:00:00Z",
            "platform": "linux", "architecture": "x86_64",
            "nanHarness": {"version": "0.1.6", "sha256": digest(self.nanh)},
            "results": [{"app": "chatgpt-desktop",
                         "deterministic": [copy.deepcopy(probe) for _ in range(3)],
                         "live": {**probe, "status": "skipped", "reason": "missing-key"},
                         "cleanup": "passed"}], "cleanup": "passed",
        }
        self.diagnostic = {"diagnosticVersion": 1, "kind": "chatgpt-startup-wrapper",
                           "wrapperSha256": digest(self.wrapper), "observation": report}
        self.diagnostic_path = self.root / "diagnostic.json"
        self.facts_path = self.root / "facts.json"
        self.destination = self.root / "staged.json"
        self.diagnostic_path.write_text(json.dumps(self.diagnostic, indent=2))
        self.facts_path.write_text(json.dumps(self.facts))

    def stage(self, replacements=None):
        args = [self.diagnostic_path, self.facts_path, self.wrapper, REDUCER,
                CHECKER, self.nanh, self.destination]
        for index, value in (replacements or {}).items():
            args[index] = value
        return subprocess.run(["bash", ROOT / "chatgpt-wave12-stage.sh", *args],
                              capture_output=True, timeout=10, check=False)

    def assert_refused(self, replacements=None):
        result = self.stage(replacements)
        self.assertEqual(result.returncode, 78, result.stderr.decode())
        self.assertFalse(self.destination.exists())

    def test_preserves_facts_and_rejects_public_submission(self):
        result = self.stage()
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        envelope = json.loads(self.destination.read_text())
        self.assertEqual(envelope, {**self.diagnostic, "startupFacts": self.facts})
        self.assertEqual(self.destination.stat().st_mode & 0o777, 0o600)
        result = subprocess.run([CHECKER, "validate-report", self.destination], capture_output=True)
        self.assertNotEqual(result.returncode, 0)

    def test_each_unknown_field_is_rejected(self):
        for location in ("diagnostic", "observation", "facts"):
            with self.subTest(location=location):
                diagnostic, facts = copy.deepcopy(self.diagnostic), copy.deepcopy(self.facts)
                target = {"diagnostic": diagnostic, "observation": diagnostic["observation"],
                          "facts": facts}[location]
                target["privateMarker"] = "must never leave the private input"
                self.diagnostic_path.write_text(json.dumps(diagnostic))
                self.facts_path.write_text(json.dumps(facts))
                self.assert_refused()

    def test_each_identity_is_bound_independently(self):
        for key in self.facts["identity"]:
            with self.subTest(identity=key):
                facts = copy.deepcopy(self.facts)
                facts["identity"][key] = "f" * 64
                self.facts_path.write_text(json.dumps(facts))
                self.assert_refused()

    def test_diagnostic_identity_and_lane(self):
        for change in (lambda d: d.update(wrapperSha256="f" * 64),
                       lambda d: d.update(diagnosticVersion=True),
                       lambda d: d["observation"]["nanHarness"].update(sha256="f" * 64),
                       lambda d: d["observation"].update(nanHarness=None),
                       lambda d: d["observation"].update(platform="macos")):
            diagnostic = copy.deepcopy(self.diagnostic)
            change(diagnostic)
            self.diagnostic_path.write_text(json.dumps(diagnostic))
            self.assert_refused()

    def test_duplicate_keys_are_rejected(self):
        self.diagnostic_path.write_text('{"diagnosticVersion":1,' + json.dumps(self.diagnostic)[1:])
        self.assert_refused()

    def test_symlink_inputs_are_rejected(self):
        for index, source in enumerate((self.diagnostic_path, self.facts_path, self.wrapper,
                                        REDUCER, CHECKER, self.nanh)):
            with self.subTest(input=index):
                link = self.root / f"link-{index}"
                link.symlink_to(source)
                self.assert_refused({index: link})

    def test_existing_destination_is_preserved(self):
        self.destination.write_text("previous evidence")
        self.assertEqual(self.stage().returncode, 78)
        self.assertEqual(self.destination.read_text(), "previous evidence")

    def test_dangling_destination_is_preserved(self):
        self.destination.symlink_to(self.root / "absent")
        self.assertEqual(self.stage().returncode, 78)
        self.assertTrue(self.destination.is_symlink())

    def test_nonregular_input_does_not_block(self):
        fifo = self.root / "fifo"
        os.mkfifo(fifo)
        self.assert_refused({0: fifo})
        self.assert_refused({1: self.root})

    def test_oversized_inputs_are_rejected(self):
        large = self.root / "large.json"
        large.write_bytes(b"x" * ((8 << 20) + 1))
        self.assert_refused({0: large})
        self.assert_refused({1: large})


if __name__ == "__main__":
    if not CHECKER.is_file() or not os.access(CHECKER, os.X_OK):
        raise SystemExit("Build the real desktop checker or set CHECKER before running this suite.")
    os.umask(0o077)
    unittest.main()
