#!/usr/bin/env python3
"""Deterministic contracts for hosted isolation, approval and durable publication."""

import argparse
import copy
import hashlib
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "actions"))
import cell
import emergency
import publication
from state import Store, StateError, canonical, receipt_identity


def release_reports():
    return [{"environment": {"operatingSystem": system, "architecture": "aarch64"},
             "harness": {"id": harness, "version": "1.0.0"}, "outcome": "passed",
             "trigger": "release", "tier": "release-gate", "nanHarness": {"version": "0.9.0"},
             "checks": [{"name": name, "status": "passed"} for name in
                        ("install-and-diagnose", "deterministic-conformance", "live-tool")]}
            for system in ("linux", "macos") for harness in publication.HARNESSES]


class MemoryStore(Store):
    """Small Git API double, including a competing fast-forward reference update."""

    def __init__(self):
        super().__init__("Acme/Fork")
        self.objects = {}
        tree = self.save({"kind": "tree", "tree": {}})
        self.head_sha = self.save({"kind": "commit", "tree": tree, "parents": []})
        self.conflict_once = False

    def save(self, item):
        sha = hashlib.sha256(canonical(item)).hexdigest()[:40]
        self.objects[sha] = item
        return sha

    def call(self, endpoint, payload=None, method=None):
        if endpoint == "git/ref/heads/compatibility-state":
            return {"object": {"sha": self.head_sha}}
        if endpoint.startswith("git/commits/"):
            node = self.objects[endpoint.split("/")[2]]
            assert node["kind"] == "commit"
            return {"tree": {"sha": node["tree"]}}
        if endpoint.startswith("git/trees/"):
            node = self.objects[endpoint.split("/")[2].split("?")[0]]
            assert node["kind"] == "tree", "Git tree API requires a tree SHA, not a commit SHA"
            tree = node["tree"]
            return {"tree": [{"type": "blob", "path": path, "sha": sha} for path, sha in tree.items()]}
        if endpoint == "git/blobs":
            import base64
            value = {"encoding": "base64", "content": payload["content"],
                     "size": len(base64.b64decode(payload["content"]))}
            return {"sha": self.save(value)}
        if endpoint.startswith("git/blobs/"):
            return self.objects[endpoint.split("/")[2]]
        if endpoint == "git/trees":
            base = self.objects[payload["base_tree"]]
            assert base["kind"] == "tree", "base_tree must identify a tree object"
            tree = copy.deepcopy(base["tree"])
            for item in payload["tree"]:
                tree[item["path"]] = item["sha"]
            return {"sha": self.save({"kind": "tree", "tree": tree})}
        if endpoint == "git/commits":
            assert self.objects[payload["tree"]]["kind"] == "tree"
            return {"sha": self.save({"kind": "commit", "tree": payload["tree"], "parents": payload["parents"]})}
        if endpoint == "git/refs/heads/compatibility-state":
            self.assert_patch(method)
            if self.conflict_once:
                self.conflict_once = False
                self.enqueue({"kind": "other-approved-request"})
            if self.objects[payload["sha"]]["parents"] != [self.head_sha]:
                raise StateError("not a fast forward")
            self.head_sha = payload["sha"]
            return {}
        raise AssertionError(endpoint)

    @staticmethod
    def assert_patch(method):
        if method != "PATCH":
            raise AssertionError("reference updates must use PATCH")


class QueueTests(unittest.TestCase):
    def test_enqueue_is_idempotent_and_completed_requests_do_not_repeat(self):
        store = MemoryStore()
        request = {"kind": "recommend", "tag": "v1.2.3"}
        identity = store.enqueue(request)
        self.assertEqual(identity, store.enqueue(request))
        self.assertEqual(list(store.pending()), [(identity, request)])
        store.put(f"completed/{identity}.json", canonical({"request": identity}), immutable=True)
        self.assertEqual(list(store.pending()), [])

    def test_concurrent_enqueue_rebases_without_losing_either_request(self):
        store = MemoryStore()
        store.conflict_once = True
        with patch("state.time.sleep"):
            store.enqueue({"kind": "first-approved-request"})
        self.assertEqual({request["kind"] for _, request in store.pending()},
                         {"first-approved-request", "other-approved-request"})

    def test_request_cannot_be_edited_in_place(self):
        store = MemoryStore()
        identity = store.enqueue({"kind": "original"})
        with self.assertRaises(StateError):
            store.put(f"requests/{identity}.json", canonical({"kind": "edited"}), immutable=True)

    def test_missing_history_fails_closed(self):
        with patch("state.api", side_effect=StateError("unavailable")):
            with self.assertRaises(StateError):
                Store("Acme/Fork").enqueue({"kind": "original"})

    def test_untrusted_state_paths_are_rejected(self):
        store = MemoryStore()
        for path in ("../main.yml", "requests/../file.json", "receipts/v1.2.3.json"):
            with self.assertRaises(StateError):
                store.put(path, b"{}")


class ApprovalTests(unittest.TestCase):
    def test_split_and_legacy_reports_are_selected_only_by_exact_digest(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            deterministic = b'{"track":"deterministic"}'
            live = b'{"track":"live"}'
            (root / "deterministic.json").write_bytes(deterministic)
            (root / "live.json").write_bytes(live)
            digest = hashlib.sha256(live).hexdigest()
            self.assertEqual(publication.reviewed_desktop_bytes(root, digest), live)
            self.assertEqual(publication.reviewed_desktop_bytes(root / "live.json", digest), live)
            with self.assertRaises(StateError):
                publication.reviewed_desktop_bytes(root, "0" * 64)
            (root / "report.json").write_bytes(live)
            with self.assertRaises(StateError):
                publication.reviewed_desktop_bytes(root, digest)

    def test_artifact_selection_rejects_oversized_reports_before_parsing(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "live.json"
            path.write_bytes(b"x" * 49153)
            with self.assertRaises(StateError):
                publication.reviewed_desktop_bytes(path, "0" * 64)

    def args(self, raw):
        return argparse.Namespace(issue="42", digest=hashlib.sha256(raw).hexdigest(),
                                  checker=Path("checker"), run=None, report=None)

    def issue_store(self, raw):
        store = MemoryStore()
        issue = {"body": "```nanh-desktop-report\n" + raw.decode() + "\n```"}
        original = store.call
        store.call = lambda endpoint, payload=None, method=None: issue if endpoint == "issues/42" else original(endpoint, payload, method)
        return store

    def test_manual_approval_freezes_exact_digest_without_author_allowlist(self):
        raw = b'{"schemaVersion":1}'
        store = self.issue_store(raw)
        with patch("publication.command", return_value=b""):
            publication.enqueue_desktop(self.args(raw), store)
        request = list(store.pending())[0][1]
        self.assertEqual(request["report"], {"schemaVersion": 1})
        self.assertEqual(request["digest"], hashlib.sha256(raw).hexdigest())

    def test_edited_report_is_not_the_approved_report(self):
        original = b'{"schemaVersion":1}'
        changed = b'{ "schemaVersion":1}'
        with self.assertRaises(StateError):
            publication.enqueue_desktop(self.args(original), self.issue_store(changed))

    def test_oversized_report_is_rejected_before_running_validator(self):
        raw = b"x" * 49153
        with patch("publication.command") as command:
            with self.assertRaises(StateError):
                publication.enqueue_desktop(self.args(raw), self.issue_store(raw))
            command.assert_not_called()


class RecoveryTests(unittest.TestCase):
    def receipt(self):
        return {"schemaVersion": 2, "repository": "Acme/Fork", "tag": "v0.9.0",
                "tagCommit": "a" * 40, "assetManifestSha256": hashlib.sha256(b"manifest").hexdigest(),
                "phases": {phase: index < 2 for index, phase in enumerate(publication.PHASES)},
                "reports": release_reports()}

    def store_receipt(self, store, receipt):
        identity = receipt_identity("Acme/Fork", "v0.9.0")
        store.put(f"receipts/{identity}.json", canonical(receipt))

    def test_resume_reuses_durable_reports_without_running_model_calls(self):
        store = MemoryStore()
        self.store_receipt(store, self.receipt())
        with tempfile.TemporaryDirectory() as temporary:
            args = argparse.Namespace(tag="v0.9.0", reports=Path(temporary) / "reports")
            def command(arguments):
                if arguments[1:3] == ["release", "download"]:
                    Path(arguments[-1]).write_bytes(b"manifest")
                else:
                    self.assertEqual(arguments[1:3], ["attestation", "verify"])
                return b""
            with patch("publication.remote_commit", return_value="a" * 40), patch("publication.command", side_effect=command):
                self.assertTrue(publication.resume(args, store))
            self.assertEqual(len(list(args.reports.glob("*.json"))), 30)

    def test_changed_release_commit_cannot_reuse_the_suite(self):
        store = MemoryStore()
        self.store_receipt(store, self.receipt())
        with patch("publication.remote_commit", return_value="b" * 40):
            with self.assertRaises(StateError):
                publication.resume(argparse.Namespace(tag="v0.9.0"), store)

    def test_phase_receipt_cannot_regress(self):
        store = MemoryStore()
        previous = self.receipt()
        self.store_receipt(store, previous)
        previous["phases"]["suitePassed"] = False
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "receipt.json"
            path.write_bytes(canonical(previous))
            with self.assertRaises(StateError):
                publication.checkpoint(argparse.Namespace(receipt=path), store)

    def test_checkpoint_discards_machine_specific_output_path(self):
        store = MemoryStore()
        receipt = self.receipt()
        receipt["outputDirectory"] = "/private/local/path"
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "receipt.json"
            path.write_bytes(canonical(receipt))
            publication.checkpoint(argparse.Namespace(receipt=path), store)
        raw = store.get(f"receipts/{receipt_identity('Acme/Fork', 'v0.9.0')}.json")
        self.assertNotIn(b"/private/local/path", raw)
        self.assertEqual(len(json.loads(raw)["reports"]), 30)


class CellTests(unittest.TestCase):
    def test_failure_never_exposes_child_output(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(RuntimeError, "stage did not pass") as failure:
                cell.private_command([sys.executable, "-c", "print('PRIVATE_PAYLOAD'); raise SystemExit(1)"], Path(directory))
            self.assertNotIn("PRIVATE_PAYLOAD", str(failure.exception))

    def test_non_live_process_has_no_provider_key(self):
        with tempfile.TemporaryDirectory() as directory:
            with patch.dict("os.environ", {"NAN_API_KEY": "PRIVATE_KEY"}):
                cell.private_command([sys.executable, "-c", "import os; assert 'NAN_API_KEY' not in os.environ"], Path(directory))

    def test_timeout_terminates_the_process_group(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(RuntimeError, "execution limit"):
                cell.private_command([sys.executable, "-c", "import time; time.sleep(60)"], Path(directory), timeout=0.02)

    def test_release_requires_thirty_matching_cells(self):
        reports = release_reports()
        publication.validate_reports(reports, "0.9.0")
        with self.assertRaises(StateError):
            publication.validate_reports(reports[:-1], "0.9.0")
        reports[-1]["harness"]["version"] = "1.0.1"
        with self.assertRaises(StateError):
            publication.validate_reports(reports, "0.9.0")


class EmergencyTests(unittest.TestCase):
    def test_emergency_requires_disabled_and_idle_hosted_writers(self):
        class Api:
            def call(self, endpoint):
                return {"total_count": 0} if "/runs?" in endpoint else {"state": "disabled_manually"}
        emergency.assert_idle(Api())

    def test_active_workflow_prevents_emergency_publication(self):
        class Api:
            def call(self, _endpoint):
                return {"state": "active"}
        with self.assertRaises(StateError):
            emergency.assert_idle(Api())


if __name__ == "__main__":
    unittest.main()
