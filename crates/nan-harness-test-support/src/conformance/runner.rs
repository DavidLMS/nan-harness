use super::arguments::{RunKind, headless_arguments};
use super::constants::{
    CONFORMANCE_SCHEMA_VERSION, HERMES_OPTIONAL_CREDENTIALS_CLEARED, INVENTORY_MARKER,
    KIMI_TIMEOUT, OPENCLAW_MEDIA_CREDENTIALS_CLEARED, PROVIDER_CLEANUP_MARGIN,
    PUBLISHED_SCENARIO_NAMES, ROUND_TRIP_MARKER, SENTINEL_MARKER, TEST_CREDENTIAL, WRAPPER_TIMEOUT,
};
use super::helpers::{
    duration_milliseconds, failed_scenario, scenario, tool_names, verify_expectation,
};
use super::inventory::{
    inventory_drift_fingerprint, inventory_matches, round_trip_probe, verify_probe_side_effect,
};
use super::prime_cleanup::{PrimeDaemonGuard, prime_status_path};
use super::registry::{
    HarnessRegistration, RegistryError, harness_registration, validate_harness_registry,
};
use super::report::{
    ConformanceObservation, ConformanceObservationKind, ConformanceOutcome, ConformanceReport,
    ConformanceScenario, ConformanceStatus, ReportShapeError, validate_published_scenario_set,
};
use crate::assertions::{
    ClaudeTranscript, assert_aider_edit_protocol, assert_provider_tool_round_trip, assert_sentinel,
    assert_tool_round_trip, assert_tool_round_trip_with_sanitized_ids,
};
use crate::manifest::{Coverage, embedded_tool_scenario};
use crate::scripted_provider::{ProviderScenario, ScriptedProvider, ScriptedToolCall};
use crate::terminal::{TerminalCommand, TerminalOutput};
use crate::workspace::ConformanceWorkspace;
use nan_harness_core::HarnessKind;
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConformanceError {
    #[error(transparent)]
    Registry(RegistryError),
    #[error(transparent)]
    Terminal(#[from] crate::terminal::TerminalError),
    #[error("could not prepare isolated conformance environment: {0}")]
    Environment(std::io::Error),
    #[error(transparent)]
    ReportShape(ReportShapeError),
}

#[derive(Debug)]
pub struct PublishedConformanceRunner {
    nan_harness: PathBuf,
    harness: HarnessKind,
}

impl PublishedConformanceRunner {
    #[must_use]
    pub fn new(nan_harness: impl Into<PathBuf>, harness: HarnessKind) -> Self {
        let nan_harness = nan_harness.into();
        let nan_harness = if nan_harness.is_absolute() {
            nan_harness
        } else {
            std::env::current_dir()
                .map_or(nan_harness.clone(), |directory| directory.join(nan_harness))
        };
        Self {
            nan_harness,
            harness,
        }
    }

    /// Runs the deterministic published-release contracts.
    ///
    /// # Errors
    ///
    /// Returns [`ConformanceError`] when the registry cannot be validated or a command cannot be
    /// started.
    pub async fn run(self) -> Result<ConformanceReport, ConformanceError> {
        validate_harness_registry().map_err(ConformanceError::Registry)?;
        let registration = *harness_registration(self.harness).ok_or(
            ConformanceError::Registry(RegistryError::Missing(self.harness)),
        )?;
        let started = Instant::now();
        let (inventory, observation) = self.run_inventory(registration).await;
        let scenarios = vec![
            inventory,
            self.run_tool_round_trip(registration).await,
            self.run_sentinel(registration).await,
            self.run_external_prerequisite(registration).await,
        ];
        validate_published_scenario_set(&scenarios).map_err(ConformanceError::ReportShape)?;
        let outcome = scenarios.iter().all(|scenario| {
            scenario.status == ConformanceStatus::Passed
                || (scenario.name == PUBLISHED_SCENARIO_NAMES[3]
                    && scenario.status == ConformanceStatus::Skipped)
        });
        let report = ConformanceReport {
            schema_version: CONFORMANCE_SCHEMA_VERSION,
            harness: self.harness,
            scenarios,
            observations: observation.into_iter().collect(),
            outcome: if outcome {
                ConformanceOutcome::Passed
            } else {
                ConformanceOutcome::Failed
            },
            duration_milliseconds: duration_milliseconds(started.elapsed()),
        };
        report
            .validate_shape()
            .map_err(ConformanceError::ReportShape)?;
        Ok(report)
    }

    async fn run_inventory(
        &self,
        registration: HarnessRegistration,
    ) -> (ConformanceScenario, Option<ConformanceObservation>) {
        let started = Instant::now();
        let Ok(manifest) = registration.manifest() else {
            return (failed_scenario("inventory", started), None);
        };
        let Ok(workspace) = ConformanceWorkspace::create() else {
            return (failed_scenario("inventory", started), None);
        };
        let Ok(mut daemon) = PrimeDaemonGuard::for_harness(registration.kind, workspace.path())
        else {
            return (failed_scenario("inventory", started), None);
        };
        let Ok(provider) =
            ScriptedProvider::start(ProviderScenario::inventory(INVENTORY_MARKER)).await
        else {
            let _ = daemon.cleanup().await;
            return (failed_scenario("inventory", started), None);
        };
        let output = self
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

    async fn run_tool_round_trip(&self, registration: HarnessRegistration) -> ConformanceScenario {
        let started = Instant::now();
        let Ok(manifest) = registration.manifest() else {
            return failed_scenario("tool-round-trip", started);
        };
        let Ok(workspace) = ConformanceWorkspace::create() else {
            return failed_scenario("tool-round-trip", started);
        };
        let Ok(mut daemon) = PrimeDaemonGuard::for_harness(registration.kind, workspace.path())
        else {
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
        let output = self
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

    async fn run_sentinel(&self, registration: HarnessRegistration) -> ConformanceScenario {
        let started = Instant::now();
        let Ok(workspace) = ConformanceWorkspace::create() else {
            return failed_scenario("sentinel", started);
        };
        let Ok(mut daemon) = PrimeDaemonGuard::for_harness(registration.kind, workspace.path())
        else {
            return failed_scenario("sentinel", started);
        };
        let Ok(provider) =
            ScriptedProvider::start(ProviderScenario::inventory(SENTINEL_MARKER)).await
        else {
            let _ = daemon.cleanup().await;
            return failed_scenario("sentinel", started);
        };
        let output = self
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
    async fn run_external_prerequisite(
        &self,
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
        let Ok(mut scenario_definition) =
            embedded_tool_scenario(registration.kind, &entry.scenario)
        else {
            return failed_scenario("external-prerequisite", started);
        };
        let Some(expected_error) = scenario_definition.expected_error.clone() else {
            return failed_scenario("external-prerequisite", started);
        };
        let Ok(workspace) = ConformanceWorkspace::create() else {
            return failed_scenario("external-prerequisite", started);
        };
        let Ok(mut daemon) = PrimeDaemonGuard::for_harness(registration.kind, workspace.path())
        else {
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
        let output = self
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

    async fn run_process(
        &self,
        registration: HarnessRegistration,
        workspace: &ConformanceWorkspace,
        provider: &ScriptedProvider,
        kind: RunKind,
        marker: &str,
    ) -> Result<TerminalOutput, ConformanceError> {
        let mut arguments = vec![
            OsString::from(registration.binary_name()),
            OsString::from("--provider-base-url"),
            OsString::from(provider.base_url()),
            OsString::from("--"),
        ];
        arguments.extend(headless_arguments(
            registration.kind,
            &kind,
            marker,
            workspace.path(),
        ));
        let home = workspace.path().join("home");
        fs::create_dir_all(&home).map_err(ConformanceError::Environment)?;
        let mut command = TerminalCommand::new(&self.nan_harness, workspace.path())
            .clear_environment()
            .args(arguments)
            .env("CI", "1")
            .env(
                "PATH",
                if registration.kind == HarnessKind::PrimeAgent {
                    prime_status_path()
                } else {
                    std::env::var_os("PATH").unwrap_or_default()
                },
            )
            .env("NAN_API_KEY", TEST_CREDENTIAL)
            .env("NAN_NO_COMPATIBILITY_CHECK", "1")
            .env("NAN_NO_UPDATE_CHECK", "1")
            .env(
                "NAN_HARNESS_CONFIG_DIR",
                workspace.path().join("nan-config"),
            )
            .env("HOME", &home)
            .timeout(timeout_for(registration.kind));
        if registration.kind == HarnessKind::ClaudeCode {
            command = command
                .env("CLAUDE_CONFIG_DIR", workspace.claude_config_path())
                .env("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1");
        }
        if registration.kind == HarnessKind::OpenCode {
            command = command
                .env("XDG_CONFIG_HOME", home.join("config"))
                .env("XDG_DATA_HOME", home.join("data"))
                .env("XDG_CACHE_HOME", home.join("cache"));
        }
        if matches!(
            registration.kind,
            HarnessKind::Pi | HarnessKind::Omp | HarnessKind::PrimeAgent
        ) {
            command = command
                .env("PI_CODING_AGENT_DIR", home.join("pi-agent"))
                .env("PI_OFFLINE", "1");
        }
        if registration.kind == HarnessKind::DeepSeekHarness {
            command = command
                .env("DSH_HOME", home.join("dsh"))
                .env("DSH_PERMISSION_MODE", "danger-full-access");
        }
        if registration.kind == HarnessKind::Hermes {
            for (name, value) in HERMES_OPTIONAL_CREDENTIALS_CLEARED {
                command = command.env(*name, *value);
            }
        }
        if registration.kind == HarnessKind::OpenClaw {
            for (name, value) in OPENCLAW_MEDIA_CREDENTIALS_CLEARED {
                command = command.env(*name, *value);
            }
        }
        command.run().await.map_err(ConformanceError::Terminal)
    }
}

fn timeout_for(kind: HarnessKind) -> Duration {
    if kind == HarnessKind::KimiCode {
        KIMI_TIMEOUT
    } else {
        WRAPPER_TIMEOUT.saturating_sub(PROVIDER_CLEANUP_MARGIN)
    }
}
