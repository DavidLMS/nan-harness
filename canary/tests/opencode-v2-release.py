#!/usr/bin/env python3
"""Contracts for independent, mandatory V2 release qualification."""
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "canary/actions"))
import opencode_v2_release as gate


class Qualification(unittest.TestCase):
    def test_requires_exact_installed_version_and_live_success(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            for prefix in ("nan-harness-", "nan-harness-canary-"):
                (directory / (prefix + "aarch64-apple-darwin")).write_bytes(b"fixture")
            args = SimpleNamespace(tag_commit="a" * 40, workflow_commit="b" * 40,
                                   system="macos", assets=directory, directory=directory,
                                   mode="live", model="qwen3.6")
            def install(_args, state):
                state["harness"]["version"] = gate.VERSION
            with patch.object(gate.cell, "detected_identity", return_value=("macos", "aarch64")), \
                 patch.object(gate.release_gate, "_asset_entries"), \
                 patch.object(gate.cell, "install", side_effect=install), \
                 patch.object(gate.cell, "live", return_value=1) as live:
                report = gate.qualify(args)
                self.assertEqual(report["outcome"], "passed")
                self.assertEqual(report["tagCommit"], args.tag_commit)
                self.assertEqual(report["generation"], 2)
                live.return_value = 0
                with self.assertRaises(ValueError):
                    gate.qualify(args)
                with patch.object(gate.cell, "install"):
                    with self.assertRaises(ValueError):
                        gate.qualify(args)
                with patch.object(gate.release_gate, "_asset_entries", side_effect=ValueError):
                    with self.assertRaises(ValueError):
                        gate.qualify(args)

    def test_native_and_live_jobs_block_release(self):
        workflows = ROOT / ".github/workflows"
        native = (workflows / "opencode-v2-conformance.yml").read_text()
        self.assertIn("workflow_call:", native)
        for platform in ("ubuntu-24.04", "macos-15", "windows-2025"):
            self.assertIn(platform, native)
        for name in ("ci.yml", "release.yml"):
            self.assertIn("uses: ./.github/workflows/opencode-v2-conformance.yml",
                          (workflows / name).read_text())
        release = (workflows / "release.yml").read_text().split("  publish:\n")[1]
        self.assertIn("      - opencode-v2\n", release)
        live = (workflows / "release-gate.yml").read_text()
        self.assertIn("needs: [prepare, cell, opencode-v2]", live)
        self.assertIn("needs: [prepare, aggregate, desktop, opencode-v2]", live)
        self.assertIn("needs.opencode-v2.result == 'success'", live)
        self.assertIn("--tag-commit \"$TAG_COMMIT\"", live)
        self.assertIn("name: opencode-v2-release-${{ matrix.system }}", live)


if __name__ == "__main__":
    unittest.main()
