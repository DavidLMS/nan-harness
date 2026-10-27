#!/usr/bin/env python3
"""Static contracts for the manual native Windows diagnostic workflow."""

from pathlib import Path
import os
import re
import subprocess
import sys
import textwrap
import unittest


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = (ROOT / ".github/workflows/windows-cli-diagnostic.yml").read_text()


class WindowsDiagnosticWorkflowTests(unittest.TestCase):
    def test_actual_selection_guard_rejects_release_omp_before_any_credential_or_review_job(self):
        guard = WORKFLOW.split("  validate-inputs:\n", 1)[1].split("\n  approve-live:", 1)[0]
        self.assertIn("permissions: {}", guard)
        self.assertNotIn("environment:", guard)
        program = guard.split("          python3 - <<'PY'\n", 1)[1].split("\n          PY", 1)[0]
        program = textwrap.dedent(program)
        release = dict(RELEASE_TAG="v0.1.26", RELEASE_COMMIT="0be5d88fdefe020e8c302f6538756869042c01fd",
                       ASSET_RUN_ID="40475697974", MANIFEST_SHA256="a" * 64, SELECTED_MODE="deterministic",
                       SELECTED_HARNESSES="mimo-code", OMP_COMPARISON="false", OMP_CANDIDATE="false")
        def status(env):
            keys = {*release, "SOURCE_OVERRIDE"}
            child_env = {key: value for key, value in os.environ.items() if key not in keys}
            result = subprocess.run([sys.executable, "-I", "-c", program], env={**child_env, **env},
                                    capture_output=True, text=True, timeout=10)
            self.assertNotIn("PRIVATE", result.stderr)
            return result.returncode
        self.assertEqual(status(release), 0)
        observation = {**release, "SELECTED_MODE": "mimo-inventory-observation"}
        self.assertEqual(status(observation), 0)
        for key in ("RELEASE_TAG", "RELEASE_COMMIT", "ASSET_RUN_ID", "MANIFEST_SHA256"):
            with self.subTest(observation_missing=key):
                self.assertNotEqual(status({**observation, key: ""}), 0)
        self.assertNotEqual(status({"SELECTED_MODE": "mimo-inventory-observation"}), 0)
        canonical = {**release, "SELECTED_MODE": "mimo-conformance-observation"}
        self.assertEqual(status(canonical), 0)
        for key in ("RELEASE_TAG", "RELEASE_COMMIT", "ASSET_RUN_ID", "MANIFEST_SHA256"):
            self.assertNotEqual(status({**canonical, key: ""}), 0)
        self.assertNotEqual(status({"SELECTED_MODE": "mimo-conformance-observation"}), 0)
        for key, value in [("OMP_COMPARISON", "true"), ("OMP_CANDIDATE", "true"),
                           ("SOURCE_OVERRIDE", "b" * 40), ("SELECTED_MODE", "live"),
                           ("SELECTED_HARNESSES", "omp"), ("RELEASE_COMMIT", ""),
                           ("RELEASE_TAG", "PRIVATE"), ("ASSET_RUN_ID", "99999999999999999999")]:
            with self.subTest(key=key):
                self.assertNotEqual(status({**release, key: value}), 0)
                if key != "SELECTED_MODE":
                    self.assertNotEqual(status({**observation, key: value}), 0)
                    self.assertNotEqual(status({**canonical, key: value}), 0)
        proof = dict(SELECTED_MODE="mimo-marker-proof", SELECTED_HARNESSES="mimo-code", OMP_COMPARISON="false", OMP_CANDIDATE="false")
        self.assertEqual(status(proof), 0)
        for key, value in [("RELEASE_TAG", "v0.1.26"), ("RELEASE_COMMIT", "a"*40),
                           ("ASSET_RUN_ID", "1"), ("MANIFEST_SHA256", "a"*64),
                           ("SOURCE_OVERRIDE", "a"*40), ("SELECTED_HARNESSES", "all"),
                           ("OMP_COMPARISON", "true"), ("OMP_CANDIDATE", "true")]:
            self.assertNotEqual(status({**proof, key: value}), 0)
        for mode, comparison in [("live", "false"), ("deterministic", "true"), ("native-diagnostic", "false")]:
            self.assertEqual(status(dict(SELECTED_MODE=mode, OMP_COMPARISON=comparison)), 0)
        for name in ("approve-live", "diagnostic", "omp-usage-comparison", "mimo-inventory-observation", "mimo-conformance-observation", "mimo-marker-proof"):
            job = re.split(r"\n  [a-z][a-z0-9-]*:\n", WORKFLOW.split(f"\n  {name}:\n", 1)[1], maxsplit=1)[0]
            # Only the job's first unindented sibling can end this block.
            self.assertIn("needs:", job)
            self.assertIn("needs.validate-inputs.result == 'success'", job)

    def test_attested_release_mode_keeps_controller_and_release_assets_separate(self):
        inputs = WORKFLOW.split("    inputs:\n", 1)[1].split("\npermissions:", 1)[0]
        self.assertEqual(len(re.findall(r"^      [a-z0-9_]+:$", inputs, re.M)), 10)
        for name in ("release_tag", "release_commit", "asset_run_id", "manifest_sha256"):
            self.assertIn(f"      {name}:", inputs)
        self.assertIn("actions: read", WORKFLOW)
        self.assertNotIn("contents: write", WORKFLOW)
        self.assertIn("$specified -ne 4 -or $env:SOURCE_SHA_INPUT", WORKFLOW)
        self.assertIn("$env:SELECTED_MODE -ne 'deterministic'", WORKFLOW)
        self.assertIn("$env:SELECTED_HARNESSES -ne 'mimo-code'", WORKFLOW)
        self.assertIn("(git rev-parse HEAD) -ne $env:GITHUB_SHA", WORKFLOW)
        self.assertIn("--controller $env:GITHUB_SHA", WORKFLOW)
        self.assertIn("windows-attested-release", WORKFLOW)
        self.assertIn("$env:NAN_DIAGNOSTIC_RELEASE_VERIFIED -ne '1'", WORKFLOW)
        self.assertIn("$env:NAN_DIAGNOSTIC_BINARY = $env:NAN_DIAGNOSTIC_RELEASE_BINARY", WORKFLOW)
        self.assertIn("$observed.nanHarness.sha256 -cne $receipt.nanHarnessSha256", WORKFLOW)
        self.assertIn("$observed.canary.sha256 -cne $receipt.canarySha256", WORKFLOW)
        self.assertIn("@('--expected-mimo-version', '0.1.16')", WORKFLOW)
        self.assertIn("windows-cli-diagnostic\\release-provenance.json", WORKFLOW)

    def test_windows_survivor_filters_use_retained_membership_not_unix_inventory(self):
        self.assertNotIn("survivors::tests::scan_reports_the_current_process_ancestry", WORKFLOW)
        for name in ("retained_job_counts_descendant_after_root_exit_then_proves_cleanup",
                     "retained_job_counts_a_live_root_without_descendants",
                     "unrelated_live_child_does_not_prevent_an_available_empty_job",
                     "diagnostic_detects_owned_descendant_after_root_exit_and_both_eofs",
                     "failed_job_query_cannot_report_an_available_empty_job"):
            self.assertIn(name, WORKFLOW)

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
                      "options: [native-diagnostic, deterministic, live, codex-diagnostic, mimo-inventory-observation, mimo-conformance-observation, mimo-marker-proof]", "--harnesses",
                      "--mode", "--model", "--source-sha", "--binary",
                      "--canary", "--output"):
            self.assertIn(token, WORKFLOW)
        self.assertEqual(WORKFLOW.count("canary\\windows-diagnostic.cmd"), 1)
        self.assertIn("if: always()", WORKFLOW)

    def test_observation_forwards_only_attested_selection_without_live_credentials(self):
        job = WORKFLOW.split("\n  mimo-inventory-observation:\n", 1)[1].split("\n  mimo-conformance-observation:\n", 1)[0]
        self.assertIn("needs: validate-inputs", job)
        self.assertIn("needs.validate-inputs.result == 'success'", job)
        self.assertIn("inputs.mode == 'mimo-inventory-observation'", job)
        self.assertIn("uses: ./.github/workflows/mimo-inventory-diagnostic.yml", job)
        for name in ("release_tag", "release_commit", "asset_run_id", "manifest_sha256"):
            self.assertIn(f"      {name}: ${{{{ inputs.{name} }}}}", job)
        for forbidden in ("environment:", "secrets:", "secrets.", "source_sha:", "model:", "harnesses:", "runs-on:"):
            self.assertNotIn(forbidden, job)
        self.assertEqual(len(re.findall(r"^      [a-z0-9_]+:", job, re.M)), 4)

    def test_marker_proof_never_runs_harnesses_or_receives_live_authority(self):
        job = WORKFLOW.split('\n  mimo-marker-proof:\n', 1)[1].split('\n  diagnostic:\n', 1)[0]
        self.assertIn('needs: validate-inputs', job)
        self.assertIn("inputs.mode == 'mimo-marker-proof'", job)
        self.assertIn('ref: ${{ github.sha }}', job)
        self.assertIn('native_windows_marker_command_preserves_contract_without_git -- --ignored --exact', job)
        for forbidden in ('environment:', 'secrets.', 'NAN_API_KEY', 'windows-diagnostic.cmd', 'cli-suite.py', '--live', 'continue-on-error:'):
            self.assertNotIn(forbidden, job)
        self.assertNotRegex(job, r'\b(?:contents|actions|id-token): write\b')
        self.assertEqual(job.count('cargo test --locked'), 1)

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

    def test_mimo_portability_regression_is_source_only_and_precedes_the_native_batch(self):
        step = WORKFLOW.split('      - name: Verify MiMo marker writes without optional Git utilities\n', 1)[1].split('\n      - name:', 1)[0]
        self.assertIn("inputs.mode == 'deterministic'", step)
        self.assertIn("inputs.harnesses == 'mimo-code'", step)
        self.assertIn("inputs.release_tag == ''", step)
        self.assertIn('NAN_MIMO_MARKER_POWERSHELL', step)
        self.assertIn('NAN_MIMO_MARKER_PYTHON', step)
        self.assertIn('NAN_MIMO_MARKER_GIT_USR_BIN', step)
        self.assertIn('native_windows_marker_command_preserves_contract_without_git -- --ignored --exact', step)
        self.assertIn('if ($LASTEXITCODE -ne 0)', step)
        self.assertNotIn('continue-on-error:', step)
        self.assertNotIn('NAN_API_KEY', step)
        self.assertNotIn('secrets.', step)
        self.assertLess(WORKFLOW.index('Verify MiMo marker writes without optional Git utilities'),
                        WORKFLOW.index('Run one isolated Windows diagnostic batch'))
        self.assertIn("Copy-Item -LiteralPath $artifactSummary", WORKFLOW)
        for setup in ("NAN_DIAGNOSTIC_SETUP_IDENTITY", "NAN_DIAGNOSTIC_SETUP_NODE", "NAN_DIAGNOSTIC_SETUP_FIXTURES"):
            self.assertIn(setup, WORKFLOW)

    def test_setup_is_advisory_and_versions_are_fixed(self):
        self.assertIn("continue-on-error: true", WORKFLOW)
        self.assertIn("node-version: 24.22.1", WORKFLOW)
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
        detached = WORKFLOW.index("- name: Run detached-helper inheritance regression")
        batch = WORKFLOW.index("- name: Run one isolated Windows diagnostic batch")
        self.assertLess(build, supervised)
        self.assertLess(supervised, detached)
        self.assertLess(detached, batch)
        step = WORKFLOW[supervised:detached]
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

    def test_product_regressions_gate_the_codex_verdict(self):
        supervised = WORKFLOW.index("- name: Run supervised standard-stream regression")
        detached = WORKFLOW.index("- name: Run detached-helper inheritance regression")
        batch = WORKFLOW.index("- name: Run one isolated Windows diagnostic batch")
        gate = WORKFLOW.index("- name: Fail for failed, blocked, or unsupported required checks")
        self.assertLess(supervised, detached)
        self.assertLess(detached, batch)
        detached_step = WORKFLOW[detached:batch]
        self.assertIn("--test inherited_handles", detached_step)
        self.assertIn("NAN_DIAGNOSTIC_SETUP_DETACHED_HELPER", detached_step)
        verdict = WORKFLOW[gate:]
        for marker in ("NAN_DIAGNOSTIC_SETUP_SUPERVISED", "NAN_DIAGNOSTIC_SETUP_DETACHED_HELPER"):
            self.assertIn(marker, verdict)

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
