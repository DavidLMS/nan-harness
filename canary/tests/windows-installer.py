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
                "https://code.kimi.com/kimi-code/install.ps1",
            "aaif-goose/goose",
            "goose-x86_64-pc-windows-msvc.zip",
        ):
            self.assertIn(marker, SCRIPT)
        self.assertIn("$bin = Join-Path $cell 'bin'", SCRIPT)
        self.assertIn("NPM_CONFIG_PREFIX", (Path(__file__).resolve().parents[1] / "actions" / "windows_diagnostic.py").read_text())

    def test_shell_seeking_harnesses_use_the_official_installers(self):
        # Kimi and OpenClaw install through the same official installers the product uses:
        # they pin the resolved version and bootstrap what the harness's shell tools need.
        self.assertIn("https://code.kimi.com/kimi-code/install.ps1", SCRIPT)
        self.assertIn("$env:KIMI_VERSION = $Version", SCRIPT)
        # Hermes stages its own launchers only when it manages the virtual environment, so
        # the cell must not skip it.
        self.assertNotIn("'-NoVenv'", SCRIPT)
        self.assertIn("@('-SkipSetup','-HermesHome',$hermesHome,'-InstallDir',$hermesInstall", SCRIPT)
        self.assertIn("'-ForceCommit','-NonInteractive','-Json'", SCRIPT)
        self.assertIn("'fetch','--depth','1','origin',$Ref", SCRIPT)
        self.assertIn("'checkout','--detach',$Ref", SCRIPT)
        self.assertIn("Hermes checkout is not a retryable Git repository", SCRIPT)
        self.assertIn('$privateProcessText = $errTask.Result + "`n" + $outTask.Result', SCRIPT)
        # A missing staged launcher is reported by the installer instead of surfacing later as
        # an uninstalled harness in the product's doctor.
        self.assertIn("'launcher-missing'", SCRIPT)
        self.assertIn("https://openclaw.ai/install.ps1", SCRIPT)
        self.assertNotIn("'openclaw' { Npm", SCRIPT)

    def test_safe_argument_boundary_does_not_use_start_process_argumentlist(self):
        self.assertIn("ProcessStartInfo", SCRIPT)
        self.assertIn("ArgumentList.Add", SCRIPT)
        self.assertNotIn("Start-Process", SCRIPT)
        collector = (Path(__file__).resolve().parents[1] / "actions" / "windows_diagnostic.py").read_text()
        self.assertIn('"pwsh"', collector)
        self.assertNotIn('"powershell"', collector)
        self.assertIn("WindowsJob", collector)
        self.assertNotIn('"taskkill"', collector)
        self.assertIn("'npm-node'", SCRIPT)
        self.assertIn("Get-Command node.exe", SCRIPT)
        self.assertIn("node_modules/npm/bin/npm-cli.js", SCRIPT)
        self.assertNotIn("$env:ComSpec", SCRIPT)
        self.assertIn("npm(?: ERR!| error) code", SCRIPT)
        self.assertIn("npm-command-missing", SCRIPT)
        self.assertNotIn("@('/d','/c',$command)", SCRIPT)
        # A nested installer's own failure class is recorded, so hermes does not report only
        # the generic installer-failed reason.
        self.assertIn("processCategory", SCRIPT)
        self.assertIn("installer-refused", SCRIPT)
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
