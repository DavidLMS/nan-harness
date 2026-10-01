#!/usr/bin/env python3
"""Offline contracts for the bounded OMP incident comparison."""
import json
import os
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "canary/actions"))
import omp_usage_comparison as comparison


class ComparisonTests(unittest.TestCase):
    def test_release_identity_must_match_the_incident(self):
        name = comparison.PLATFORM_ASSETS["windows"]["harness"]
        release = {"commit": comparison.RELEASE_COMMIT, "digests": {name: comparison.RELEASE_DIGEST}}
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            with patch.object(comparison.daily, "release_assets", return_value=release):
                binary, _, commit, version = comparison.binaries("release", root, "a" * 40)
                self.assertEqual(binary.name, name)
                self.assertEqual((commit, version), (comparison.RELEASE_COMMIT, "0.1.11"))
            for changed in ({**release, "commit": "b" * 40},
                            {**release, "digests": {name: "c" * 64}}):
                with patch.object(comparison.daily, "release_assets", return_value=changed):
                    with self.assertRaises(ValueError):
                        comparison.binaries("release", root, "a" * 40)

    def test_frozen_versions_isolate_credentials_and_keep_failure_status(self):
        for kind in ("release", "candidate"):
            for version in comparison.VERSIONS:
                for status in (0, 1, 3):
                    with self.subTest(kind=kind, version=version, status=status), tempfile.TemporaryDirectory() as tmp:
                        root = Path(tmp)
                        args = SimpleNamespace(kind=kind, omp_version=version,
                                               directory=root / "private", reports=root / "reports")
                        def binaries(*_):
                            self.assertNotIn("NAN_API_KEY", os.environ)
                            return root / "nan.exe", root / "canary.exe", "a" * 40, "0.1.11"
                        env = {"NAN_API_KEY": "synthetic-private-key", "GH_TOKEN": "synthetic-github-key",
                               "GITHUB_TOKEN": "synthetic-github-key", "GITHUB_SHA": "a" * 40,
                               "GITHUB_RUN_ID": "123", "GITHUB_RUN_ATTEMPT": "1"}
                        with patch.dict(os.environ, env, clear=True), \
                             patch.object(comparison.daily, "command", return_value=b"a" * 40), \
                             patch.object(comparison, "binaries", side_effect=binaries), \
                             patch.object(comparison, "digest", return_value="b" * 64), \
                             patch.object(comparison.subprocess, "run", return_value=SimpleNamespace(returncode=status)) as run:
                            self.assertEqual(comparison.run(args), status)
                        command = run.call_args.args[0]
                        self.assertEqual(command[command.index("--source-kind") + 1],
                                         "release" if kind == "release" else "branch")
                        self.assertNotIn("synthetic-private-key", str(command))
                        self.assertNotIn("GH_TOKEN", run.call_args.kwargs["env"])
                        self.assertNotIn("GITHUB_TOKEN", run.call_args.kwargs["env"])
                        self.assertEqual(run.call_args.kwargs["env"]["NAN_API_KEY"], "synthetic-private-key")
                        frozen = comparison.daily.suite.read_frozen_manifest(
                            args.directory / "versions.json", ["omp"], "windows", "x86_64", "qwen3.6")
                        self.assertEqual(frozen[0].version, version)
                        provenance = json.loads((args.reports / "provenance.json").read_text())
                        self.assertEqual(provenance["attempts"], 1)
                        self.assertEqual(provenance["binaryKind"], kind)
                        self.assertNotIn("synthetic-private-key", json.dumps(provenance))
                        self.assertEqual(run.call_count, 1)

    def test_wrong_checkout_never_starts_a_probe(self):
        with patch.dict(os.environ, {"NAN_API_KEY": "synthetic", "GITHUB_SHA": "b" * 40}), \
             patch.object(comparison.daily, "command", return_value=b"a" * 40), \
             patch.object(comparison, "binaries") as binaries:
            with self.assertRaises(ValueError):
                comparison.run(SimpleNamespace())
            binaries.assert_not_called()


if __name__ == "__main__":
    unittest.main()
