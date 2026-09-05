use super::super::arguments::RunKind;
use super::super::constants::{INVENTORY_MARKER, ROUND_TRIP_MARKER, SENTINEL_MARKER};
use super::super::helpers::{failed_scenario, scenario, tool_names, verify_expectation};
use super::super::inventory::{
    inventory_drift_fingerprint, inventory_matches, round_trip_probe, verify_probe_side_effect,
};
use super::super::prime_cleanup::PrimeDaemonGuard;
use super::super::registry::HarnessRegistration;
use super::super::report::{
    ConformanceObservation, ConformanceObservationKind, ConformanceScenario, ConformanceStatus,
};
use super::PublishedConformanceRunner;
use crate::assertions::{
    ClaudeTranscript, assert_aider_edit_protocol, assert_provider_tool_round_trip, assert_sentinel,
    assert_tool_round_trip, assert_tool_round_trip_with_sanitized_ids,
};
use crate::manifest::{Coverage, embedded_tool_scenario};
use crate::scripted_provider::{ProviderScenario, ScriptedProvider, ScriptedToolCall};
use crate::workspace::ConformanceWorkspace;
use nan_harness_core::HarnessKind;
use std::collections::BTreeSet;
use std::fs;
use std::time::Instant;

pub(super) async fn run_inventory(
    runner: &PublishedConformanceRunner,
    registration: HarnessRegistration,
) -> (ConformanceScenario, Option<ConformanceObservation>) {
    let started = Instant::now();
    let Ok(manifest) = registration.manifest() else {
        return (failed_scenario("inventory", started), None);
    };
    let Ok(workspace) = ConformanceWorkspace::create() else {
        return (failed_scenario("inventory", started), None);
    };
    let Ok(mut daemon) = PrimeDaemonGuard::for_harness(registration.kind, workspace.path()) else {
        return (failed_scenario("inventory", started), None);
    };
    let Ok(provider) = ScriptedProvider::start(ProviderScenario::inventory(INVENTORY_MARKER)).await
    else {
        let _ = daemon.cleanup().await;
        return (failed_scenario("inventory", started), None);
    };
    let output = runner
        .run_process(
            registration,
            &workspace,
            &provider,
            RunKind::Inventory,
            INVENTORY_MARKER,
        )
        .await;
    let requests = provider.chat_requests();
    let provider_complete = provider.completed();
    let provider_bounded = provider.recording_bounded();
    let provider_shutdown = provider.shutdown().await.is_ok();
    let daemon_clean = daemon.cleanup().await.is_ok();
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
    if !operationally_compatible
        || !inventory_matches
        || std::env::var_os("NAN_HARNESS_CONFORMANCE_DIAGNOSTICS").is_some()
    {
        eprintln!(
            "conformance inventory diagnostics for {}: expected={:?}, actual={actual_inventory:?}, matched={inventory_matches}, process_succeeded={}, marker_observed={}, requests={}, provider_complete={provider_complete}, provider_bounded={provider_bounded}, provider_shutdown={provider_shutdown}, daemon_clean={daemon_clean}",
            registration.kind,
            manifest.tool_names(),
            output.as_ref().is_ok_and(|output| output.status.success()),
            output
                .as_ref()
                .is_ok_and(|output| output.stdout.contains(INVENTORY_MARKER)),
            requests.len(),
        );
    }
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
    (scenario("inventory", status, started), observation)
}

pub(super) async fn run_tool_round_trip(
    runner: &PublishedConformanceRunner,
    registration: HarnessRegistration,
) -> ConformanceScenario {
    let started = Instant::now();
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
    let output = runner
        .run_process(
            registration,
            &workspace,
            &provider,
            RunKind::Tool(probe.call.clone()),
            ROUND_TRIP_MARKER,
        )
        .await;
    let requests = provider.chat_requests();
    let provider_complete = provider.completed();
    let provider_bounded = provider.recording_bounded();
    let provider_shutdown = provider.shutdown().await.is_ok();
    let daemon_clean = daemon.cleanup().await.is_ok();
    let passed = output.as_ref().is_ok_and(|output| {
        if !(provider_complete && provider_bounded && provider_shutdown && daemon_clean) {
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
        assertion
            .and_then(|()| verify_probe_side_effect(&probe))
            .is_ok()
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
    let output = runner
        .run_process(
            registration,
            &workspace,
            &provider,
            RunKind::Sentinel,
            SENTINEL_MARKER,
        )
        .await;
    let requests = provider.chat_requests();
    let provider_complete = provider.completed();
    let provider_bounded = provider.recording_bounded();
    let provider_shutdown = provider.shutdown().await.is_ok();
    let daemon_clean = daemon.cleanup().await.is_ok();
    let passed = output.as_ref().is_ok_and(|output| {
        provider_complete
            && provider_bounded
            && provider_shutdown
            && daemon_clean
            && assert_sentinel(output, &requests, SENTINEL_MARKER).is_ok()
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
    let requests = provider.chat_requests();
    let provider_complete = provider.completed();
    let provider_bounded = provider.recording_bounded();
    let provider_shutdown = provider.shutdown().await.is_ok();
    let daemon_clean = daemon.cleanup().await.is_ok();
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
