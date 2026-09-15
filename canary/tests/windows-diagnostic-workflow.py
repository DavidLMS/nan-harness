#!/usr/bin/env python3
"""Static contracts for the manual native Windows diagnostic workflow."""

from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = (ROOT / ".github/workflows/windows-cli-diagnostic.yml").read_text()


class WindowsDiagnosticWorkflowTests(unittest.TestCase):
    def test_manual_windows_only_and_least_privilege(self):
        self.assertIn("workflow_dispatch:", WORKFLOW)
        self.assertNotIn("schedule:", WORKFLOW)
        self.assertNotIn("workflow_call:", WORKFLOW)
        self.assertIn("runs-on: windows-2025", WORKFLOW)
        self.assertNotIn("wsl", WORKFLOW.lower())
        self.assertIn("permissions:\n  contents: read", WORKFLOW)
        self.assertIn("persist-credentials: false", WORKFLOW)
        self.assertIn("fetch --no-tags --depth=1 origin $source", WORKFLOW)
        self.assertIn("rev-parse HEAD) -ne $source", WORKFLOW)

    def test_bounded_inputs_and_single_batch_entrypoint(self):
        for token in ("harnesses:", "mode:", "model:", "source_sha:",
                      "options: [native-diagnostic, deterministic, live, codex-diagnostic]", "--harnesses",
                      "--mode", "--model", "--source-sha", "--binary",
                      "--canary", "--output"):
            self.assertIn(token, WORKFLOW)
        self.assertEqual(WORKFLOW.count("canary\\windows-diagnostic.cmd"), 1)
        self.assertIn("if: always()", WORKFLOW)

    def test_codex_mode_uses_one_canary_invocation_and_strict_reader(self):
        self.assertIn("    env:\n      DIAGNOSTIC_MODE: ${{ inputs.mode }}", WORKFLOW)
        self.assertIn("--codex $installed.executable --expected-version $installed.version", WORKFLOW)
        self.assertIn("- $prepareStarted).TotalMilliseconds", WORKFLOW)
        self.assertIn("$env:DIAGNOSTIC_MODE -eq 'codex-diagnostic'", WORKFLOW)
        self.assertIn("codex-diagnostic --nan-harness $env:NAN_DIAGNOSTIC_BINARY", WORKFLOW)
        self.assertIn("canary\\actions\\codex_diagnostic.py", WORKFLOW)
        self.assertIn("$report.overall.failed", WORKFLOW)
        self.assertIn("$report.overall.blocked", WORKFLOW)
        self.assertIn("--prepare --cell $codexCell", WORKFLOW)
        self.assertIn("install-harness.ps1", WORKFLOW)
        self.assertIn("$totalMilliseconds", WORKFLOW)
        self.assertIn("home\\.npm-global", WORKFLOW)
        self.assertIn("--blocked-reason unsafe_prerequisite", WORKFLOW)
        self.assertIn("Copy-Item -LiteralPath $artifactSummary", WORKFLOW)
        for setup in ("NAN_DIAGNOSTIC_SETUP_IDENTITY", "NAN_DIAGNOSTIC_SETUP_NODE", "NAN_DIAGNOSTIC_SETUP_FIXTURES"):
            self.assertIn(setup, WORKFLOW)

    def test_setup_is_advisory_and_versions_are_fixed(self):
        self.assertIn("continue-on-error: true", WORKFLOW)
        self.assertIn("node-version: 24.20.0", WORKFLOW)
        self.assertIn("python-version: '3.12'", WORKFLOW)
        self.assertIn("rustup show", WORKFLOW)
        self.assertIn("cargo build --locked --release --package nan-harness-cli", WORKFLOW)
        self.assertIn("cargo build --locked --release --package nan-harness-canary", WORKFLOW)
        self.assertIn("--target x86_64-pc-windows-msvc", WORKFLOW)
        self.assertIn("NAN_DIAGNOSTIC_BINARY", WORKFLOW)
        self.assertIn("NAN_DIAGNOSTIC_CANARY", WORKFLOW)
        self.assertIn("timeout-minutes: 120", WORKFLOW)
        self.assertIn("NAN_DIAGNOSTIC_WORKFLOW_START_UTC", WORKFLOW)
        self.assertIn("120 * 60 - $elapsed - 600", WORKFLOW)
        self.assertIn("[math]::Min(6000, $remaining)", WORKFLOW)
        self.assertIn("--budget-seconds $env:NAN_DIAGNOSTIC_BATCH_BUDGET", WORKFLOW)

    def test_supervised_regression_is_built_and_reported_without_blocking_the_batch(self):
        build = WORKFLOW.index("- name: Build native diagnostic binaries")
        supervised = WORKFLOW.index("- name: Run supervised standard-stream regression")
        batch = WORKFLOW.index("- name: Run one isolated Windows diagnostic batch")
        self.assertLess(build, supervised)
        self.assertLess(supervised, batch)
        step = WORKFLOW[supervised:batch]
        # The regression owns the real supervisor and a harness double, not Codex itself.
        self.assertIn("--bin codex-harness-fixture", WORKFLOW[build:supervised])
        self.assertIn("NAN_CODEX_DIAGNOSTIC_TEST_NAN_HARNESS", step)
        self.assertIn("NAN_CODEX_DIAGNOSTIC_TEST_HARNESS_FIXTURE", step)
        self.assertIn("codex_diagnostic::tests::native_supervised_launch_attributes_a_leaked_descendant", step)
        self.assertIn("-- --ignored --exact --nocapture", step)
        self.assertIn("if: always()", step)
        self.assertIn("NAN_DIAGNOSTIC_SETUP_SUPERVISED", step)
        # Its own failure must be visible in the run without suppressing the independent batch.
        self.assertNotIn("continue-on-error: true", step)

    def test_focused_rust_fixture_runs_after_exact_source_checkout(self):
        source = WORKFLOW.index("- name: Select exact tested source SHA")
        fixtures = WORKFLOW.index("- name: Run native Windows fixture regressions")
        fixture = WORKFLOW.index("- name: Run focused Rust Windows environment fixture")
        build = WORKFLOW.index("- name: Build native diagnostic binaries")
        self.assertLess(source, fixtures)
        self.assertLess(fixtures, fixture)
        self.assertLess(fixture, build)
        self.assertIn("cargo test --locked -p nan-harness-test-support --all-features $filter -- --exact --list", WORKFLOW)
        self.assertIn("matches.Count -ne 1", WORKFLOW)
        self.assertIn("focused Rust $($entry.Name) fixture discovery failed", WORKFLOW)
        self.assertIn("NAN_DIAGNOSTIC_SETUP_RUST_FIXTURE=success", WORKFLOW)
        self.assertIn("terminal::tests::inherited_pipe_descendants_are_killed_with_the_owned_shell", WORKFLOW)
        self.assertIn("terminal::tests::parent_exits_before_inherited_pipe_cleanup", WORKFLOW)
        self.assertIn("$rustFailures.Count -eq 0", WORKFLOW)
        self.assertIn("env.NAN_DIAGNOSTIC_SETUP_RUST_FIXTURE || steps.rust_fixture.outcome", WORKFLOW)

    def test_all_python_and_powershell_fixtures_run_after_exact_source(self):
        source = WORKFLOW.index("- name: Select exact tested source SHA")
        fixtures = WORKFLOW.index("- name: Run native Windows fixture regressions")
        self.assertLess(source, fixtures)
        self.assertIn("Get-Command pwsh", WORKFLOW)
        self.assertIn("python3 canary/tests/probe-harness-windows.py -v", WORKFLOW)
        self.assertIn("canary/tests/windows-summary.py", WORKFLOW)
        self.assertIn("-match 'skipped=[1-9][0-9]*'", WORKFLOW)
        self.assertIn("NAN_DIAGNOSTIC_SETUP_FIXTURES=success", WORKFLOW)
        self.assertIn("NAN_DIAGNOSTIC_SETUP_FIXTURES=failed-probe-regressions", WORKFLOW)

    def test_focused_fixture_failure_remains_a_reported_setup_failure(self):
        self.assertIn("NAN_DIAGNOSTIC_SETUP_RUST_FIXTURE", WORKFLOW)
        self.assertIn("NAN_DIAGNOSTIC_SETUP_FIXTURES", WORKFLOW)
        self.assertIn("$_.Value -notin @('success', '')", WORKFLOW)
        self.assertIn("throw \"Windows diagnostic setup failed", WORKFLOW)
        self.assertNotIn("continue-on-error: false", WORKFLOW)

    def test_live_secret_is_scoped_and_report_is_always_uploaded(self):
        self.assertIn("inputs.mode == 'live' && secrets.NAN_API_KEY || ''", WORKFLOW)
        self.assertIn("name: Upload diagnostic report", WORKFLOW)
        self.assertIn("if-no-files-found: warn", WORKFLOW)
        self.assertNotIn("echo $env:NAN_API_KEY", WORKFLOW)
        self.assertNotIn("Write-Host $env:NAN_API_KEY", WORKFLOW)
        self.assertIn("windows-cli-diagnostic\\report.json", WORKFLOW)
        self.assertIn("windows-cli-diagnostic\\summary.md", WORKFLOW)
        self.assertNotIn("windows-cli-diagnostic\\*", WORKFLOW)
        self.assertIn("Fail for failed, blocked, or unsupported", WORKFLOW)
        for stage in ("CHECKOUT", "NODE", "PYTHON", "RUST", "SOURCE", "FIXTURES", "BUILD"):
            self.assertIn(f"NAN_DIAGNOSTIC_SETUP_{stage}", WORKFLOW)


if __name__ == "__main__":
    unittest.main()
