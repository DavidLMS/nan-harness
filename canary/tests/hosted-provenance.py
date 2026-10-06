#!/usr/bin/env python3
"""Automatic publication rejects fork, branch, cancelled and identity-shifted evidence."""

import copy
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "actions"))
import provenance
from state import StateError

SHA = "a" * 40


class Store:
    repository = "owner/repository"

    def __init__(self):
        self.run = {"id": 12, "head_sha": SHA, "path": provenance.AUTOMATED_WORKFLOW,
                    "head_branch": "main", "head_repository": {"full_name": self.repository},
                    "event": "schedule", "status": "completed", "conclusion": "success"}
        self.comparison = {"status": "ahead", "merge_base_commit": {"sha": SHA}}

    def call(self, endpoint):
        if endpoint == "actions/runs/12":
            return self.run
        if endpoint == "git/ref/heads/main":
            return {"object": {"sha": "b" * 40}}
        if endpoint == f"compare/{SHA}...{'b' * 40}":
            return self.comparison
        raise AssertionError(endpoint)


class ProvenanceTests(unittest.TestCase):
    def test_only_completed_trusted_main_runs_can_authorize_automatic_updates(self):
        self.assertEqual(provenance.trusted_run(Store(), 12), SHA)
        for field, value in (("event", "pull_request"), ("head_branch", "feature"),
                             ("head_repository", {"full_name": "fork/repository"}),
                             ("path", ".github/workflows/desktop-check.yml"),
                             ("conclusion", "cancelled"), ("status", "in_progress")):
            store = Store()
            store.run[field] = value
            with self.assertRaises(StateError, msg=field):
                provenance.trusted_run(store, 12)
        store = Store()
        store.comparison["merge_base_commit"]["sha"] = "c" * 40
        with self.assertRaises(StateError):
            provenance.trusted_run(store, 12)

    def test_negative_completed_suite_is_not_silently_discarded(self):
        store = Store()
        store.run["conclusion"] = "failure"
        self.assertEqual(provenance.trusted_run(store, 12), SHA)

    def test_digest_is_stable_for_docs_but_changes_for_executable_inputs(self):
        def tree(code="b", docs="c"):
            return (f"100644 blob {code * 40}\tcanary/actions/cell.py\0"
                    f"100644 blob {docs * 40}\tcanary/README.md\0").encode()
        with patch("provenance.subprocess.run") as execute:
            execute.return_value.returncode = 0
            execute.return_value.stdout = tree()
            digest = provenance.specification_digest("/unused", SHA, "cli")
            execute.return_value.stdout = tree(docs="d")
            self.assertEqual(digest, provenance.specification_digest("/unused", SHA, "cli"))
            execute.return_value.stdout = tree(code="e")
            self.assertNotEqual(digest, provenance.specification_digest("/unused", SHA, "cli"))

    def test_native_release_and_model_must_match_independently_verified_inputs(self):
        expected = {"version": "0.1.6", "binarySha256": "a" * 64, "platform": "linux",
                    "architecture": "aarch64", "model": "qwen3.6"}
        report = {"nanHarness": {"version": "0.1.6", "sha256": "a" * 64},
                  "environment": {"operatingSystem": "linux", "architecture": "aarch64"},
                  "model": "qwen3.6", "checks": [{"name": "live-tool"}]}
        provenance.bind_report(report, "cli", expected)
        for field in expected:
            changed = copy.deepcopy(expected)
            changed[field] = "different"
            with self.assertRaises(StateError, msg=field):
                provenance.bind_report(report, "cli", changed)


if __name__ == "__main__":
    unittest.main()
