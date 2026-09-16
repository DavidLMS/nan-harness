#!/usr/bin/env python3
"""Static installer contracts that run without Windows or network access."""
from pathlib import Path
import unittest

SCRIPT = (Path(__file__).resolve().parents[1] / "guest" / "install-harness.ps1").read_text(encoding="utf-8")


class WindowsInstallerTests(unittest.TestCase):
    def test_native_paths_use_exact_sources_and_private_cell_roots(self):
        for marker in (
            "raw.githubusercontent.com/NousResearch/hermes-agent/$Ref/scripts/install.ps1",
            "can1357/oh-my-pi",
            "omp-windows-x64.exe",
            "kimi-cli==$Version",
            "aaif-goose/goose",
            "goose-x86_64-pc-windows-msvc.zip",
        ):
            self.assertIn(marker, SCRIPT)
        self.assertIn("$bin = Join-Path $cell 'bin'", SCRIPT)
        self.assertIn("NPM_CONFIG_PREFIX", (Path(__file__).resolve().parents[1] / "actions" / "windows_diagnostic.py").read_text())

    def test_safe_argument_boundary_does_not_use_start_process_argumentlist(self):
        self.assertIn("ProcessStartInfo", SCRIPT)
        self.assertIn("ArgumentList.Add", SCRIPT)
        self.assertNotIn("Start-Process", SCRIPT)
        collector = (Path(__file__).resolve().parents[1] / "actions" / "windows_diagnostic.py").read_text()
        self.assertIn('"pwsh"', collector)
        self.assertNotIn('"powershell"', collector)
        self.assertIn("WindowsJob", collector)
        self.assertNotIn('"taskkill"', collector)
        # npm runs lifecycle scripts only for the packages an installer names, so a
        # harness with native dependencies passes the same allowlist the Unix channel uses.
        self.assertIn("--allow-scripts=", SCRIPT)
        self.assertIn("@('openclaw','@google/genai','protobufjs','tree-sitter-bash')", SCRIPT)
        self.assertIn("'npm-node'", SCRIPT)
        self.assertIn("Get-Command node.exe", SCRIPT)
        self.assertIn("node_modules/npm/bin/npm-cli.js", SCRIPT)
        self.assertNotIn("$env:ComSpec", SCRIPT)
        self.assertIn("npm(?: ERR!| error) code", SCRIPT)
        self.assertIn("npm-command-missing", SCRIPT)
        self.assertNotIn("@('/d','/c',$command)", SCRIPT)
        self.assertIn("pipCategory", SCRIPT)
        self.assertIn("PythonVersion", SCRIPT)
        self.assertIn('"-$PythonVersion"', SCRIPT)

    def test_exact_version_and_structured_prime_fx_probe_reasons(self):
        self.assertIn("$Version", SCRIPT)
        self.assertIn("Probe-OfficialWindowsMetadata", SCRIPT)
        self.assertIn("official platform metadata is inconclusive", SCRIPT)
        self.assertIn("official platform metadata probe failed", SCRIPT)
        self.assertIn("$Version -notmatch", SCRIPT)
        self.assertIn("official-metadata-no-windows-asset", SCRIPT)

    def test_closed_diagnostic_marker_has_bounded_fields(self):
        self.assertIn("schemaVersion = 2", SCRIPT)
        for marker in ("subphase", "executable", "exitCode", "win32Error", "httpStatus", "assetReason", "npmCode", "pipCategory"):
            self.assertIn(marker, SCRIPT)

    def test_private_logs_are_removed_on_success_and_failure(self):
        self.assertIn("finally", SCRIPT)
        self.assertIn("Remove-Item -LiteralPath $stdoutLog,$stderrLog", SCRIPT)
        self.assertNotIn("NAN_API_KEY", SCRIPT)
        self.assertNotIn("GITHUB_TOKEN", SCRIPT)


if __name__ == "__main__":
    unittest.main()
