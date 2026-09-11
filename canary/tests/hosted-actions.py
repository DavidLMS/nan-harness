#!/usr/bin/env python3
"""Deterministic contracts for hosted isolation, approval and durable publication."""

import argparse
import base64
import copy
import hashlib
import json
from pathlib import Path
import re
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "actions"))
import cell
import emergency
import publication
from state import Store, StateError, canonical, receipt_identity

WORKFLOWS = Path(__file__).resolve().parents[2] / ".github/workflows"
BINARY_BYTES = {"linux": b"linux-binary", "macos": b"macos-binary"}
BINARY_SHA = {system: hashlib.sha256(raw).hexdigest() for system, raw in BINARY_BYTES.items()}
MANIFEST = "".join(f"{BINARY_SHA[system]}  {publication.BINARIES[system]}\n"
                   for system in ("linux", "macos")).encode()
COMMIT = "a" * 40


def release_reports():
    return [{"environment": {"operatingSystem": system, "architecture": "aarch64"},
             "harness": {"id": harness, "version": "1.0.0"}, "outcome": "passed",
             "trigger": "release", "tier": "release-gate",
             "nanHarness": {"version": "0.9.0", "sha256": BINARY_SHA[system]},
             "checks": [{"name": name, "status": "passed"} for name in
                        ("install-and-diagnose", "deterministic-conformance", "live-tool")]}
            for system in ("linux", "macos") for harness in publication.HARNESSES]


def gate_request(source_run="1", reports=None):
    return {"schemaVersion": 1, "kind": "gate", "tag": "v0.9.0", "commit": COMMIT,
            "reports": release_reports() if reports is None else reports, "sourceRun": source_run}


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

    def durable_receipt(self):
        raw = self.get(f"receipts/{receipt_identity(self.repository, 'v0.9.0')}.json")
        return None if raw is None else json.loads(raw)


class UnavailableStore(MemoryStore):
    def call(self, endpoint, payload=None, method=None):
        raise StateError("state branch is missing or unreadable")


class ReleaseDouble:
    """Fake trusted-publication commands; records every command the writer runs."""

    def __init__(self, manifest=MANIFEST, fail_release_gate=False):
        self.manifest = manifest
        self.fail_release_gate = fail_release_gate
        self.calls = []

    def release_gate_runs(self):
        return [call for call in self.calls if call[0] == "bash" and call[1].endswith("run-release-gate.sh")]

    def __call__(self, arguments, env=None):
        arguments = [str(argument) for argument in arguments]
        self.calls.append(arguments)
        if arguments[:3] == ["gh", "release", "download"]:
            if "--dir" in arguments:
                assets = Path(arguments[arguments.index("--dir") + 1])
                for system, raw in BINARY_BYTES.items():
                    (assets / publication.BINARIES[system]).write_bytes(raw)
                (assets / "nan-harness-canary-aarch64-unknown-linux-musl").write_bytes(b"validator")
                (assets / "SHA256SUMS").write_bytes(self.manifest)
            else:
                Path(arguments[arguments.index("--output") + 1]).write_bytes(self.manifest)
        elif arguments[0] == "bash" and arguments[1].endswith("run-release-gate.sh"):
            assert env["NAN_CANARY_WRITER"] == "actions"
            if self.fail_release_gate:
                raise StateError("writer was interrupted")
        elif arguments[:3] == ["gh", "attestation", "verify"] or arguments[:2] == ["git", "fetch"]:
            pass
        elif arguments[:2] == ["git", "worktree"] or arguments[1:2] == ["validate-report"]:
            pass
        elif arguments[0] == "bash" and arguments[1].endswith("verify-release-assets.sh"):
            pass
        else:
            raise AssertionError(arguments)
        return b""


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

    def test_request_whose_bytes_no_longer_match_its_identity_is_not_drained(self):
        store = MemoryStore()
        store.enqueue(gate_request())
        blob = next(item for item in store.objects.values() if item.get("encoding") == "base64")
        blob["content"] = base64.b64encode(canonical(gate_request(source_run="forged"))).decode()
        with self.assertRaises(StateError):
            list(store.pending())

    def test_missing_history_fails_closed(self):
        with patch("state.api", side_effect=StateError("unavailable")):
            with self.assertRaises(StateError):
                Store("Acme/Fork").enqueue({"kind": "original"})

    def test_untrusted_state_paths_are_rejected(self):
        store = MemoryStore()
        for path in ("../main.yml", "requests/../file.json", "receipts/v1.2.3.json"):
            with self.assertRaises(StateError):
                store.put(path, b"{}")


class GateEvidenceTests(unittest.TestCase):
    """Invalid or incomplete suite evidence never becomes a publication request."""

    def enqueue(self, reports, commit=COMMIT, validator_fails=False):
        store = MemoryStore()
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            for index, report in enumerate(reports):
                (directory / f"{index}.json").write_bytes(canonical(report))
            args = argparse.Namespace(reports=directory, validator=Path("validator"), tag="v0.9.0", commit=commit)
            validator = patch("publication.command", side_effect=StateError("invalid") if validator_fails else None)
            with validator, patch("publication.remote_commit", return_value=COMMIT):
                try:
                    publication.enqueue_gate(args, store)
                finally:
                    self.pending = list(store.pending())

    def assert_refused(self, reports, **options):
        with self.assertRaises(StateError):
            self.enqueue(reports, **options)
        self.assertEqual(self.pending, [])

    def test_complete_matrix_is_persisted_once(self):
        self.enqueue(release_reports())
        self.assertEqual(len(self.pending), 1)
        self.assertEqual(len(self.pending[0][1]["reports"]), 30)

    def test_incomplete_matrix_cannot_be_enqueued(self):
        self.assert_refused(release_reports()[:-1])

    def test_duplicate_cell_cannot_fill_a_missing_cell(self):
        reports = release_reports()
        reports[-1] = copy.deepcopy(reports[0])
        self.assert_refused(reports)

    def test_failed_or_skipped_live_cell_cannot_be_enqueued(self):
        failed = release_reports()
        failed[3]["outcome"] = "failed"
        self.assert_refused(failed)
        skipped = release_reports()
        skipped[4]["checks"] = skipped[4]["checks"][:2]
        self.assert_refused(skipped)

    def test_evidence_from_another_coverage_cannot_be_enqueued(self):
        reports = release_reports()
        for report in reports:
            report["trigger"], report["tier"] = "weekly", "live-extended"
        self.assert_refused(reports)

    def test_report_rejected_by_trusted_validator_cannot_be_enqueued(self):
        self.assert_refused(release_reports(), validator_fails=True)

    def test_moved_release_tag_cannot_be_enqueued(self):
        self.assert_refused(release_reports(), commit="b" * 40)


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
                "tagCommit": COMMIT, "assetManifestSha256": hashlib.sha256(MANIFEST).hexdigest(),
                "phases": {phase: index < 2 for index, phase in enumerate(publication.PHASES)},
                "reports": release_reports()}

    def store_receipt(self, store, receipt):
        identity = receipt_identity("Acme/Fork", "v0.9.0")
        store.put(f"receipts/{identity}.json", canonical(receipt))

    def resume(self, store, manifest=MANIFEST, commit=COMMIT):
        with tempfile.TemporaryDirectory() as temporary:
            args = argparse.Namespace(tag="v0.9.0", reports=Path(temporary) / "reports")
            release = ReleaseDouble(manifest)
            with patch("publication.remote_commit", return_value=commit), patch("publication.command", side_effect=release):
                try:
                    return publication.resume(args, store)
                finally:
                    self.reused = len(list(args.reports.glob("*.json"))) if args.reports.exists() else 0

    def test_resume_reuses_durable_reports_without_running_model_calls(self):
        store = MemoryStore()
        self.store_receipt(store, self.receipt())
        self.assertTrue(self.resume(store))
        self.assertEqual(self.reused, 30)

    def test_resume_reuses_evidence_persisted_before_an_interrupted_writer_receipted_it(self):
        store = MemoryStore()
        store.enqueue(gate_request())
        self.assertTrue(self.resume(store))
        self.assertEqual(self.reused, 30)

    def test_completed_or_other_release_requests_are_not_reused(self):
        store = MemoryStore()
        identity = store.enqueue(gate_request())
        self.assertFalse(self.resume(store, commit="b" * 40))
        store.put(f"completed/{identity}.json", canonical({"request": identity}), immutable=True)
        self.assertFalse(self.resume(store))
        self.assertEqual(self.reused, 0)

    def test_reused_evidence_must_describe_the_attested_binaries(self):
        store = MemoryStore()
        store.enqueue(gate_request())
        forged = MANIFEST.replace(BINARY_SHA["linux"].encode(), b"0" * 64)
        with self.assertRaises(StateError):
            self.resume(store, manifest=forged)
        self.assertEqual(self.reused, 0)

    def test_changed_release_assets_cannot_reuse_the_receipted_suite(self):
        store = MemoryStore()
        self.store_receipt(store, self.receipt())
        with self.assertRaises(StateError):
            self.resume(store, manifest=MANIFEST + b"\n")
        self.assertEqual(self.reused, 0)

    def test_changed_release_commit_cannot_reuse_the_suite(self):
        store = MemoryStore()
        self.store_receipt(store, self.receipt())
        with patch("publication.remote_commit", return_value="b" * 40):
            with self.assertRaises(StateError):
                publication.resume(argparse.Namespace(tag="v0.9.0"), store)

    def test_absent_state_fails_closed_instead_of_rerunning_or_reusing(self):
        with patch("publication.remote_commit", return_value=COMMIT):
            with self.assertRaises(StateError):
                publication.resume(argparse.Namespace(tag="v0.9.0"), UnavailableStore())

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


class WriterTests(unittest.TestCase):
    """The sole Actions writer binds evidence to attested assets and resumes by receipt."""

    def drain(self, store, release):
        with patch("publication.remote_commit", return_value=COMMIT), patch("publication.command", side_effect=release):
            publication.drain(argparse.Namespace(checker=Path("checker")), store)

    def test_writer_receipts_verified_evidence_before_publishing(self):
        store, release = MemoryStore(), ReleaseDouble()
        store.enqueue(gate_request())
        self.drain(store, release)
        self.assertEqual(len(release.release_gate_runs()), 1)
        receipt = store.durable_receipt()
        self.assertEqual(receipt["assetManifestSha256"], hashlib.sha256(MANIFEST).hexdigest())
        self.assertTrue(receipt["phases"]["suitePassed"])
        self.assertNotIn("outputDirectory", receipt)
        self.assertEqual(list(store.pending()), [])

    def test_digest_mismatch_refuses_before_any_publication(self):
        reports = release_reports()
        reports[0]["nanHarness"]["sha256"] = "0" * 64
        store, release = MemoryStore(), ReleaseDouble()
        store.enqueue(gate_request(reports=reports))
        with self.assertRaises(StateError):
            self.drain(store, release)
        self.assertEqual(release.release_gate_runs(), [])
        self.assertIsNone(store.durable_receipt())
        self.assertEqual(len(list(store.pending())), 1)

    def test_receipt_for_different_assets_refuses_before_any_publication(self):
        store, release = MemoryStore(), ReleaseDouble()
        receipt = RecoveryTests.receipt(None)
        receipt["assetManifestSha256"] = "f" * 64
        RecoveryTests.store_receipt(None, store, receipt)
        store.enqueue(gate_request())
        with self.assertRaises(StateError):
            self.drain(store, release)
        self.assertEqual(release.release_gate_runs(), [])

    def test_duplicate_requests_share_one_receipt_and_both_complete(self):
        store, release = MemoryStore(), ReleaseDouble()
        store.enqueue(gate_request(source_run="1"))
        store.enqueue(gate_request(source_run="2"))
        self.drain(store, release)
        first = store.durable_receipt()
        self.assertEqual(len(release.release_gate_runs()), 2)
        self.assertEqual(list(store.pending()), [])
        self.drain(store, release)
        self.assertEqual(store.durable_receipt(), first)
        self.assertEqual(len(release.release_gate_runs()), 2)

    def test_interrupted_publication_stays_pending_and_retry_reuses_suite(self):
        store = MemoryStore()
        store.enqueue(gate_request())
        with self.assertRaises(StateError):
            self.drain(store, ReleaseDouble(fail_release_gate=True))
        self.assertEqual(len(list(store.pending())), 1)
        self.assertTrue(store.durable_receipt()["phases"]["suitePassed"])
        recovery = RecoveryTests()
        self.assertTrue(recovery.resume(store))
        retry = ReleaseDouble()
        self.drain(store, retry)
        self.assertEqual(len(retry.release_gate_runs()), 1)
        self.assertEqual(list(store.pending()), [])

    def test_absent_state_cannot_be_drained_as_empty_history(self):
        with self.assertRaises(StateError):
            self.drain(UnavailableStore(), ReleaseDouble())


class CoverageTests(unittest.TestCase):
    def select(self, coverage, harnesses=""):
        return cell.select_coverage(coverage, harnesses, 7, COMMIT, "c" * 40)

    def test_release_runs_every_live_cell_from_the_release_commit(self):
        selected = self.select("release")
        self.assertEqual(len(selected["cells"]), 30)
        self.assertTrue(all(item["live"] for item in selected["cells"]))
        self.assertEqual((selected["trigger"], selected["source"]), ("release", COMMIT))
        with self.assertRaises(ValueError):
            self.select("release", "codex")

    def test_evidence_only_coverage_runs_dispatched_code(self):
        daily = self.select("daily")
        self.assertEqual({item["system"] for item in daily["cells"]}, {"linux"})
        self.assertEqual(sum(item["live"] for item in daily["cells"]), 2)
        weekly = self.select("weekly")
        self.assertEqual(len(weekly["cells"]), 30)
        self.assertEqual({daily["source"], weekly["source"]}, {"c" * 40})
        self.assertEqual({item["architecture"] for item in weekly["cells"]}, {"aarch64"})

    def test_smoke_is_bounded_deterministic_and_never_release_coverage(self):
        selected = self.select("smoke", "codex,fx")
        self.assertEqual(len(selected["cells"]), 4)
        self.assertFalse(any(item["live"] for item in selected["cells"]))
        self.assertEqual({item["system"] for item in selected["cells"]}, {"linux", "macos"})
        self.assertEqual((selected["trigger"], selected["source"]), ("manual", "c" * 40))
        self.assertEqual({item["runner"] for item in selected["cells"]}, {"ubuntu-24.04", "macos-15"})
        self.assertEqual({item["architecture"] for item in selected["cells"]}, {"x86_64", "aarch64"})
        self.assertIn("nan-harness-x86_64-unknown-linux-musl",
                      {item["binary_asset"] for item in selected["cells"]})
        for invalid in ("", "codex,codex", "unknown", ",".join(cell.HARNESSES[:5])):
            with self.assertRaises(ValueError):
                self.select("smoke", invalid)
        with self.assertRaises(ValueError):
            self.select("nightly")


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

    def report(self, trigger):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            binary = directory / "nan-harness"
            binary.write_bytes(b"binary")
            state = {"harness": {"id": "codex", "version": "1.0.0"}, "tier": "deterministic",
                     "nanHarness": {"sha256": cell.digest(binary)}, "trigger": trigger,
                     "checks": [{"name": "install-and-diagnose"}, {"name": "deterministic-conformance"}]}
            (directory / "state.json").write_bytes(canonical(state))
            args = argparse.Namespace(stage="report", trigger=trigger, harness="codex", binary=binary,
                                      canary=binary, directory=directory, output=directory / "out.json")
            with patch("cell.private_command"):
                cell.run(args)
            return json.loads(args.output.read_bytes())

    def test_smoke_cell_reports_deterministic_evidence_without_a_model(self):
        report = self.report("manual")
        self.assertEqual(report["tier"], "deterministic")
        self.assertNotIn("model", report)

    def test_release_cell_without_live_stage_is_incomplete(self):
        with self.assertRaisesRegex(RuntimeError, "incomplete"):
            self.report("release")


class EmergencyTests(unittest.TestCase):
    def api(self, state="disabled_manually", active_status=None, failing=False):
        class Api:
            def call(self, endpoint):
                if failing:
                    raise StateError("uncertain")
                if "/runs?" in endpoint:
                    return {"total_count": int(f"status={active_status}&" in endpoint)}
                return {"state": state}
        return Api()

    def test_emergency_requires_disabled_and_idle_hosted_writers(self):
        emergency.assert_idle(self.api())

    def test_active_workflow_prevents_emergency_publication(self):
        with self.assertRaises(StateError):
            emergency.assert_idle(self.api(state="active"))

    def test_queued_or_waiting_hosted_writer_prevents_emergency_publication(self):
        for status in ("queued", "waiting", "pending", "in_progress", "requested"):
            with self.assertRaises(StateError):
                emergency.assert_idle(self.api(active_status=status))

    def test_uncertain_api_response_prevents_emergency_publication(self):
        with self.assertRaises(StateError):
            emergency.assert_idle(self.api(failing=True))

    def test_every_workflow_that_reaches_the_publisher_must_be_idle(self):
        sources = {path.name: path.read_text() for path in WORKFLOWS.glob("*.yml")}
        reaching = {name for name, text in sources.items() if "canary/actions/publication.py" in text}
        while True:
            callers = {name for name, text in sources.items()
                       if any(re.search(rf"workflows/{re.escape(target)}\b", text) for target in reaching)}
            if callers <= reaching:
                break
            reaching |= callers
        self.assertTrue(reaching)
        self.assertLessEqual(reaching, set(emergency.WRITERS))


if __name__ == "__main__":
    unittest.main()
