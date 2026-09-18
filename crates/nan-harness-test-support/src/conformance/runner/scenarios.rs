use super::super::arguments::RunKind;
use super::super::constants::{INVENTORY_MARKER, ROUND_TRIP_MARKER, SENTINEL_MARKER};
use super::super::helpers::{
    assertion_passed, failed_scenario, progress_event, record_assertion_code, scenario, tool_names,
    verify_expectation,
};
use super::super::inventory::{
    inventory_drift_fingerprint, inventory_matches, round_trip_probe, verify_probe_side_effect,
};
use super::super::prime_cleanup::PrimeDaemonGuard;
use super::super::registry::HarnessRegistration;
use super::super::report::{
    ConformanceObservation, ConformanceObservationKind, ConformanceScenario, ConformanceStatus,
    InventoryFailureReason,
};
use super::{PublishedConformanceRunner, inventory_process_evidence};
use crate::assertions::{
    ClaudeTranscript, assert_aider_edit_protocol, assert_provider_tool_round_trip, assert_sentinel,
    assert_tool_round_trip, assert_tool_round_trip_with_sanitized_ids,
};
use crate::manifest::{Coverage, embedded_tool_scenario};
use crate::scripted_provider::{ProviderScenario, ScriptedProvider, ScriptedToolCall};
use crate::terminal::TerminalOutput;
use crate::workspace::ConformanceWorkspace;
use nan_harness_core::HarnessKind;
use std::collections::BTreeSet;
use std::fs;
use std::time::Instant;

fn progress_result(scenario: &str, stage: &str, passed: bool, started: Instant) {
    progress_event(
        scenario,
        stage,
        if passed { "passed" } else { "failed" },
        started,
    );
}

#[allow(
    clippy::too_many_lines,
    reason = "keep the measured inventory checks together"
)]
pub(super) async fn run_inventory(
    runner: &PublishedConformanceRunner,
    registration: HarnessRegistration,
) -> (
    ConformanceScenario,
    Option<ConformanceObservation>,
    Vec<InventoryFailureReason>,
    Option<super::super::report::InventoryProcessEvidence>,
) {
    let started = Instant::now();
    progress_event("inventory", "scenario", "started", started);
    let Ok(manifest) = registration.manifest() else {
        return (
            failed_scenario("inventory", started),
            None,
            Vec::new(),
            None,
        );
    };
    let Ok(workspace) = ConformanceWorkspace::create() else {
        return (
            failed_scenario("inventory", started),
            None,
            Vec::new(),
            None,
        );
    };
    let Ok(mut daemon) = PrimeDaemonGuard::for_harness(registration.kind, workspace.path()) else {
        return (
            failed_scenario("inventory", started),
            None,
            Vec::new(),
            None,
        );
    };
    let Ok(provider) = ScriptedProvider::start(ProviderScenario::inventory(INVENTORY_MARKER)).await
    else {
        let _ = daemon.cleanup().await;
        return (
            failed_scenario("inventory", started),
            None,
            vec![InventoryFailureReason::ProviderFailed],
            None,
        );
    };
    progress_event("inventory", "process", "started", started);
    let output = runner
        .run_process(
            registration,
            &workspace,
            &provider,
            RunKind::Inventory,
            INVENTORY_MARKER,
        )
        .await;
    let process_evidence = inventory_process_evidence(&output);
    progress_result("inventory", "process", output.is_ok(), started);
    let requests = provider.chat_requests();
    let provider_complete = provider.completed();
    let provider_bounded = provider.recording_bounded();
    progress_event("inventory", "provider-shutdown", "started", started);
    let provider_shutdown = provider.shutdown().await.is_ok();
    progress_result("inventory", "provider-shutdown", provider_shutdown, started);
    progress_event("inventory", "cleanup", "started", started);
    let daemon_clean = daemon.cleanup().await.is_ok();
    progress_result("inventory", "cleanup", daemon_clean, started);
    let actual_inventory = requests
        .iter()
        .filter_map(tool_names)
        .flatten()
        .collect::<BTreeSet<_>>();
    let inventory_matches = inventory_matches(registration.kind, &manifest, &actual_inventory);
    let operationally_compatible = output.as_ref().is_ok_and(|output| {
        output.status.success()
            && output.stdout.contains(INVENTORY_MARKER)
            && !requests.is_empty()
            && provider_complete
            && provider_bounded
            && provider_shutdown
            && daemon_clean
    });
    if !operationally_compatible || !inventory_matches {
        record_assertion_code(if !inventory_matches {
            "inventory-mismatch"
        } else if !output.as_ref().is_ok_and(|output| output.status.success()) {
            "process-failed"
        } else if !output
            .as_ref()
            .is_ok_and(|output| output.stdout.contains(INVENTORY_MARKER))
        {
            "marker-missing"
        } else {
            "provider-incomplete"
        });
    }
    if !operationally_compatible
        || !inventory_matches
        || std::env::var_os("NAN_HARNESS_CONFORMANCE_DIAGNOSTICS").is_some()
    {
        let diagnostics = InventoryDiagnostics {
            kind: registration.kind,
            expected: manifest.tool_names(),
            actual: &actual_inventory,
            matched: status_of(inventory_matches),
            output: output.as_ref().ok(),
            requests: requests.len(),
            provider_complete: status_of(provider_complete),
            provider_bounded: status_of(provider_bounded),
            provider_shutdown: status_of(provider_shutdown),
            daemon_clean: status_of(daemon_clean),
        };
        log_inventory_diagnostics(&diagnostics);
    }
    let failure_reasons = inventory_failure_reasons(InventoryHealth {
        output: output.as_ref().ok(),
        provider: ProviderHealth {
            requests: if requests.is_empty() {
                CheckStatus::Failed
            } else {
                CheckStatus::Passed
            },
            complete: if provider_complete {
                CheckStatus::Passed
            } else {
                CheckStatus::Failed
            },
            bounded: if provider_bounded {
                CheckStatus::Passed
            } else {
                CheckStatus::Failed
            },
        },
        provider_shutdown: if provider_shutdown {
            CheckStatus::Passed
        } else {
            CheckStatus::Failed
        },
        daemon_cleanup: if daemon_clean {
            CheckStatus::Passed
        } else {
            CheckStatus::Failed
        },
    });
    let status = if operationally_compatible {
        ConformanceStatus::Passed
    } else {
        ConformanceStatus::Failed
    };
    let observation =
        (operationally_compatible && !inventory_matches).then(|| ConformanceObservation {
            kind: ConformanceObservationKind::InventoryDrift,
            fingerprint: inventory_drift_fingerprint(
                registration.kind,
                &manifest.tool_names(),
                &actual_inventory,
            ),
        });
    (
        scenario("inventory", status, started),
        observation,
        failure_reasons,
        process_evidence,
    )
}

#[derive(Clone, Copy)]
struct InventoryHealth<'a> {
    output: Option<&'a TerminalOutput>,
    provider: ProviderHealth,
    provider_shutdown: CheckStatus,
    daemon_cleanup: CheckStatus,
}

#[derive(Clone, Copy)]
struct ProviderHealth {
    requests: CheckStatus,
    complete: CheckStatus,
    bounded: CheckStatus,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CheckStatus {
    Passed,
    Failed,
}

fn inventory_failure_reasons(health: InventoryHealth<'_>) -> Vec<InventoryFailureReason> {
    let mut reasons = Vec::new();
    if !health.output.is_some_and(|output| output.status.success()) {
        reasons.push(InventoryFailureReason::ProcessFailed);
    } else if !health
        .output
        .is_some_and(|output| output.stdout.contains(INVENTORY_MARKER))
    {
        reasons.push(InventoryFailureReason::MarkerMissing);
    }
    if health.provider.requests == CheckStatus::Failed
        || health.provider.complete == CheckStatus::Failed
        || health.provider.bounded == CheckStatus::Failed
    {
        reasons.push(InventoryFailureReason::ProviderFailed);
    }
    if health.provider_shutdown == CheckStatus::Failed {
        reasons.push(InventoryFailureReason::ProviderShutdownFailed);
    }
    if health.daemon_cleanup == CheckStatus::Failed {
        reasons.push(InventoryFailureReason::DaemonCleanupFailed);
    }
    reasons
}

struct InventoryDiagnostics<'a> {
    kind: HarnessKind,
    expected: BTreeSet<String>,
    actual: &'a BTreeSet<String>,
    matched: CheckStatus,
    output: Option<&'a TerminalOutput>,
    requests: usize,
    provider_complete: CheckStatus,
    provider_bounded: CheckStatus,
    provider_shutdown: CheckStatus,
    daemon_clean: CheckStatus,
}

fn status_of(value: bool) -> CheckStatus {
    if value {
        CheckStatus::Passed
    } else {
        CheckStatus::Failed
    }
}

fn log_inventory_diagnostics(diagnostics: &InventoryDiagnostics<'_>) {
    eprintln!(
        "conformance inventory diagnostics for {}: expected={:?}, actual={:?}, matched={}, process_succeeded={}, marker_observed={}, requests={}, provider_complete={}, provider_bounded={}, provider_shutdown={}, daemon_clean={}",
        diagnostics.kind,
        diagnostics.expected,
        diagnostics.actual,
        diagnostics.matched == CheckStatus::Passed,
        diagnostics
            .output
            .is_some_and(|output| output.status.success()),
        diagnostics
            .output
            .is_some_and(|output| output.stdout.contains(INVENTORY_MARKER)),
        diagnostics.requests,
        diagnostics.provider_complete == CheckStatus::Passed,
        diagnostics.provider_bounded == CheckStatus::Passed,
        diagnostics.provider_shutdown == CheckStatus::Passed,
        diagnostics.daemon_clean == CheckStatus::Passed,
    );
}

pub(super) async fn run_tool_round_trip(
    runner: &PublishedConformanceRunner,
    registration: HarnessRegistration,
) -> ConformanceScenario {
    let started = Instant::now();
    progress_event("tool-round-trip", "scenario", "started", started);
    let Ok(manifest) = registration.manifest() else {
        return failed_scenario("tool-round-trip", started);
    };
    let Ok(workspace) = ConformanceWorkspace::create() else {
        return failed_scenario("tool-round-trip", started);
    };
    let Ok(mut daemon) = PrimeDaemonGuard::for_harness(registration.kind, workspace.path()) else {
        return failed_scenario("tool-round-trip", started);
    };
    let Ok(probe) = round_trip_probe(registration.kind, workspace.path(), &manifest) else {
        let _ = daemon.cleanup().await;
        return failed_scenario("tool-round-trip", started);
    };
    if registration.kind == HarnessKind::Aider
        && fs::write(workspace.resolve("edit-target.txt"), "EDIT_TARGET_BEFORE\n").is_err()
    {
        let _ = daemon.cleanup().await;
        return failed_scenario("tool-round-trip", started);
    }
    let provider_scenario = if registration.kind == HarnessKind::Aider {
        ProviderScenario::inventory(format!(
            "edit-target.txt\n```text\n{ROUND_TRIP_MARKER}\n```\n"
        ))
    } else {
        ProviderScenario::tool(
            probe.call.name.clone(),
            probe.call.input.clone(),
            ROUND_TRIP_MARKER,
        )
    };
    let Ok(provider) = ScriptedProvider::start(provider_scenario).await else {
        let _ = daemon.cleanup().await;
        return failed_scenario("tool-round-trip", started);
    };
    progress_event("tool-round-trip", "process", "started", started);
    let output = runner
        .run_process(
            registration,
            &workspace,
            &provider,
            RunKind::Tool(probe.call.clone()),
            ROUND_TRIP_MARKER,
        )
        .await;
    progress_result("tool-round-trip", "process", output.is_ok(), started);
    let requests = provider.chat_requests();
    let provider_complete = provider.completed();
    let provider_bounded = provider.recording_bounded();
    progress_event("tool-round-trip", "provider-shutdown", "started", started);
    let provider_shutdown = provider.shutdown().await.is_ok();
    progress_result(
        "tool-round-trip",
        "provider-shutdown",
        provider_shutdown,
        started,
    );
    progress_event("tool-round-trip", "cleanup", "started", started);
    let daemon_clean = daemon.cleanup().await.is_ok();
    progress_result("tool-round-trip", "cleanup", daemon_clean, started);
    let passed = output.as_ref().is_ok_and(|output| {
        if !(provider_complete && provider_bounded && provider_shutdown && daemon_clean) {
            record_assertion_code("provider-incomplete");
            return false;
        }
        let assertion = match registration.kind {
            HarnessKind::Aider => assert_aider_edit_protocol(
                output,
                &requests,
                &workspace.resolve("edit-target.txt"),
                "EDIT_TARGET_BEFORE\n",
                ROUND_TRIP_MARKER,
            ),
            HarnessKind::OpenClaw => assert_tool_round_trip_with_sanitized_ids(
                output,
                &requests,
                std::slice::from_ref(&probe.call),
                ROUND_TRIP_MARKER,
            ),
            _ => assert_tool_round_trip(
                output,
                &requests,
                std::slice::from_ref(&probe.call),
                ROUND_TRIP_MARKER,
            ),
        };
        assertion_passed(assertion.and_then(|()| verify_probe_side_effect(&probe)))
    });
    let status = if passed {
        ConformanceStatus::Passed
    } else {
        ConformanceStatus::Failed
    };
    scenario("tool-round-trip", status, started)
}

pub(super) async fn run_sentinel(
    runner: &PublishedConformanceRunner,
    registration: HarnessRegistration,
) -> ConformanceScenario {
    let started = Instant::now();
    progress_event("sentinel", "scenario", "started", started);
    let Ok(workspace) = ConformanceWorkspace::create() else {
        return failed_scenario("sentinel", started);
    };
    let Ok(mut daemon) = PrimeDaemonGuard::for_harness(registration.kind, workspace.path()) else {
        return failed_scenario("sentinel", started);
    };
    let Ok(provider) = ScriptedProvider::start(ProviderScenario::inventory(SENTINEL_MARKER)).await
    else {
        let _ = daemon.cleanup().await;
        return failed_scenario("sentinel", started);
    };
    progress_event("sentinel", "process", "started", started);
    let output = runner
        .run_process(
            registration,
            &workspace,
            &provider,
            RunKind::Sentinel,
            SENTINEL_MARKER,
        )
        .await;
    progress_result("sentinel", "process", output.is_ok(), started);
    let requests = provider.chat_requests();
    let provider_complete = provider.completed();
    let provider_bounded = provider.recording_bounded();
    progress_event("sentinel", "provider-shutdown", "started", started);
    let provider_shutdown = provider.shutdown().await.is_ok();
    progress_result("sentinel", "provider-shutdown", provider_shutdown, started);
    progress_event("sentinel", "cleanup", "started", started);
    let daemon_clean = daemon.cleanup().await.is_ok();
    progress_result("sentinel", "cleanup", daemon_clean, started);
    let passed = output.as_ref().is_ok_and(|output| {
        if !(provider_complete && provider_bounded && provider_shutdown && daemon_clean) {
            record_assertion_code("provider-incomplete");
            return false;
        }
        assertion_passed(assert_sentinel(output, &requests, SENTINEL_MARKER))
    });
    let status = if passed {
        ConformanceStatus::Passed
    } else {
        ConformanceStatus::Failed
    };
    scenario("sentinel", status, started)
}

#[allow(clippy::too_many_lines)]
pub(super) async fn run_external_prerequisite(
    runner: &PublishedConformanceRunner,
    registration: HarnessRegistration,
) -> ConformanceScenario {
    let started = Instant::now();
    progress_event("external-prerequisite", "scenario", "started", started);
    let Ok(manifest) = registration.manifest() else {
        return failed_scenario("external-prerequisite", started);
    };
    let Some(entry) = manifest
        .tools
        .iter()
        .find(|entry| entry.coverage == Coverage::ExternalAuthentication)
    else {
        return scenario("external-prerequisite", ConformanceStatus::Skipped, started);
    };
    if registration.kind != HarnessKind::ClaudeCode {
        return failed_scenario("external-prerequisite", started);
    }
    let Ok(mut scenario_definition) = embedded_tool_scenario(registration.kind, &entry.scenario)
    else {
        return failed_scenario("external-prerequisite", started);
    };
    let Some(expected_error) = scenario_definition.expected_error.clone() else {
        return failed_scenario("external-prerequisite", started);
    };
    let Ok(workspace) = ConformanceWorkspace::create() else {
        return failed_scenario("external-prerequisite", started);
    };
    let Ok(mut daemon) = PrimeDaemonGuard::for_harness(registration.kind, workspace.path()) else {
        return failed_scenario("external-prerequisite", started);
    };
    scenario_definition.expand_workspace(workspace.path(), "{{fixture_url}}");
    let calls = scenario_definition
        .steps
        .iter()
        .map(|step| ScriptedToolCall {
            name: step.tool.clone(),
            input: step.input.clone(),
            result_expected: true,
        })
        .collect::<Vec<_>>();
    let Ok(provider) = ScriptedProvider::start(ProviderScenario::sequence(
        calls.iter().cloned(),
        &scenario_definition.final_marker,
    ))
    .await
    else {
        let _ = daemon.cleanup().await;
        return failed_scenario("external-prerequisite", started);
    };
    scenario_definition.expand_workspace(workspace.path(), &provider.fixture_url());
    let calls = scenario_definition
        .steps
        .iter()
        .map(|step| ScriptedToolCall {
            name: step.tool.clone(),
            input: step.input.clone(),
            result_expected: true,
        })
        .collect::<Vec<_>>();
    let enabled_tools = calls
        .iter()
        .map(|call| call.name.clone())
        .collect::<Vec<_>>();
    progress_event("external-prerequisite", "process", "started", started);
    let output = runner
        .run_process(
            registration,
            &workspace,
            &provider,
            RunKind::External {
                tool: entry.name.clone(),
                arguments: scenario_definition.arguments.clone(),
                enabled_tools,
            },
            &scenario_definition.final_marker,
        )
        .await;
    progress_result("external-prerequisite", "process", output.is_ok(), started);
    let requests = provider.chat_requests();
    let provider_complete = provider.completed();
    let provider_bounded = provider.recording_bounded();
    progress_event(
        "external-prerequisite",
        "provider-shutdown",
        "started",
        started,
    );
    let provider_shutdown = provider.shutdown().await.is_ok();
    progress_result(
        "external-prerequisite",
        "provider-shutdown",
        provider_shutdown,
        started,
    );
    progress_event("external-prerequisite", "cleanup", "started", started);
    let daemon_clean = daemon.cleanup().await.is_ok();
    progress_result("external-prerequisite", "cleanup", daemon_clean, started);
    let passed = output.as_ref().is_ok_and(|output| {
        if !(provider_complete
            && provider_bounded
            && provider_shutdown
            && daemon_clean
            && assert_provider_tool_round_trip(&requests, &calls).is_ok())
        {
            return false;
        }
        let transcript = ClaudeTranscript::parse(output.stdout.clone());
        transcript.is_ok_and(|transcript| {
            transcript
                .require_expected_tool_error(
                    &entry.name,
                    &expected_error,
                    &scenario_definition.final_marker,
                )
                .is_ok()
                && verify_expectation(&scenario_definition.expectation).is_ok()
        })
    });
    let status = if passed {
        ConformanceStatus::Passed
    } else {
        ConformanceStatus::Failed
    };
    scenario("external-prerequisite", status, started)
}
