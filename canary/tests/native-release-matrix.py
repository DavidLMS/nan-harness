#!/usr/bin/env python3
"""New release receipts require native coverage and the exact selected model."""

import copy
import importlib.util
from pathlib import Path
import sys
from types import SimpleNamespace
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "actions"))
import publication
from selection import select_suite
from state import StateError

spec = importlib.util.spec_from_file_location("legacy_tests", ROOT / "tests/hosted-actions.py")
legacy = importlib.util.module_from_spec(spec)
spec.loader.exec_module(legacy)


def native_reports():
    base = legacy.release_reports()[0]
    reports = []
    for job in select_suite("cli")["platforms"]:
        for harness in job["harnesses"]:
            report = copy.deepcopy(base)
            report["environment"] = {"operatingSystem": job["system"], "architecture": job["architecture"]}
            report["harness"]["id"] = harness
            report["model"] = "selected-model"
            reports.append(report)
    return reports


class NativeReleaseMatrixTests(unittest.TestCase):
    def test_complete_native_matrix_requires_44_supported_cells(self):
        reports = native_reports()
        self.assertEqual(len(reports), 44)
        publication.validate_reports(reports, "0.9.0", 2, "selected-model")
        for changed in (reports[:-1], reports + [reports[0]], legacy.release_reports()):
            with self.assertRaises(StateError):
                publication.validate_reports(changed, "0.9.0", 2, "selected-model")

    def test_legacy_receipt_contract_remains_readable_but_cannot_approve_new_matrix(self):
        publication.validate_reports(legacy.release_reports(), "0.9.0")
        store = legacy.MemoryStore()
        store.enqueue(legacy.gate_request())
        args = SimpleNamespace(tag="v0.9.0", native_matrix=True, model="selected-model")
        with patch.object(publication, "remote_commit", return_value=legacy.COMMIT):
            self.assertFalse(publication.resume(args, store))

    def test_model_and_native_architecture_cannot_be_reused_across_cells(self):
        for field, value in (("model", "different-model"), ("model", None)):
            reports = native_reports()
            reports[-1][field] = value
            with self.assertRaises(StateError):
                publication.validate_reports(reports, "0.9.0", 2, "selected-model")
        reports = native_reports()
        reports[-1]["environment"]["architecture"] = "aarch64"
        with self.assertRaises(StateError):
            publication.validate_reports(reports, "0.9.0", 2, "selected-model")
        with self.assertRaises(StateError):
            publication.validate_reports(native_reports(), "0.9.0", 2)


if __name__ == "__main__":
    unittest.main()
