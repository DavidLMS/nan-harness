use crate::assertions::{
    ClaudeTranscript, ProbeAssertionError, assert_aider_edit_protocol,
    assert_provider_tool_round_trip, assert_sentinel, assert_tool_round_trip,
    assert_tool_round_trip_with_sanitized_ids,
};
use crate::manifest::{
    ConformanceManifest, Coverage, Expectation, ToolManifestEntry, embedded_manifest,
    embedded_manifest_sources, embedded_tool_scenario,
};
use crate::scripted_provider::{ProviderScenario, ScriptedProvider, ScriptedToolCall};
use crate::terminal::{TerminalCommand, TerminalOutput};
use crate::workspace::ConformanceWorkspace;
use nan_harness_core::HarnessKind;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use thiserror::Error;

pub const CONFORMANCE_SCHEMA_VERSION: u8 = 1;
pub const TEST_CREDENTIAL: &str = "nan-harness-conformance-test-credential";
pub const INVENTORY_MARKER: &str = "NAN_HARNESS_CONFORMANCE_INVENTORY_OK";
pub const SENTINEL_MARKER: &str = "NAN_HARNESS_CONFORMANCE_SENTINEL_OK";
pub const ROUND_TRIP_MARKER: &str = "NAN_HARNESS_CONFORMANCE_ROUND_TRIP_OK";
pub const EXTERNAL_MARKER: &str = "NAN_HARNESS_CONFORMANCE_EXTERNAL_OK";

const MAX_DURATION_MILLISECONDS: u64 = 86_400_000;
const MAX_REPORT_SCENARIOS: usize = 4;
const MAX_REPORT_CHECKS: usize = 8;
const MAX_REPORT_NAME_BYTES: usize = 64;
const PUBLISHED_SCENARIO_NAMES: [&str; 4] = [
    "inventory",
    "tool-round-trip",
    "sentinel",
    "external-prerequisite",
];
const WRAPPER_TIMEOUT: Duration = Duration::from_secs(90);
const KIMI_TIMEOUT: Duration = Duration::from_secs(40);
const PROVIDER_CLEANUP_MARGIN: Duration = Duration::from_secs(2);
const PRIME_STATUS_TIMEOUT: Duration = Duration::from_secs(2);
const PRIME_TERM_TIMEOUT: Duration = Duration::from_secs(2);
const PRIME_KILL_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HarnessRegistration {
    pub kind: HarnessKind,
}

impl HarnessRegistration {
    #[must_use]
    pub const fn binary_name(self) -> &'static str {
        self.kind.binary_name()
    }

    /// Parses this registration's compile-time embedded manifest.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryError`] if the embedded source is malformed.
    pub fn manifest(self) -> Result<ConformanceManifest, RegistryError> {
        embedded_manifest(self.kind).map_err(|error| RegistryError::Manifest {
            kind: self.kind,
            message: error.to_string(),
        })
    }
}

const fn registration(kind: HarnessKind) -> HarnessRegistration {
    HarnessRegistration { kind }
}

const REGISTRY: [HarnessRegistration; HarnessKind::ALL.len()] = [
    registration(HarnessKind::ALL[0]),
    registration(HarnessKind::ALL[1]),
    registration(HarnessKind::ALL[2]),
    registration(HarnessKind::ALL[3]),
    registration(HarnessKind::ALL[4]),
    registration(HarnessKind::ALL[5]),
    registration(HarnessKind::ALL[6]),
    registration(HarnessKind::ALL[7]),
    registration(HarnessKind::ALL[8]),
    registration(HarnessKind::ALL[9]),
    registration(HarnessKind::ALL[10]),
    registration(HarnessKind::ALL[11]),
    registration(HarnessKind::ALL[12]),
    registration(HarnessKind::ALL[13]),
];

#[must_use]
pub fn harness_registry() -> &'static [HarnessRegistration] {
    &REGISTRY
}

#[must_use]
pub fn harness_registration(kind: HarnessKind) -> Option<&'static HarnessRegistration> {
    REGISTRY
        .iter()
        .find(|registration| registration.kind == kind)
}

/// Validates the exact one-to-one relationship between canonical harness identities, embedded
/// manifests, and canonical binary names.
///
/// # Errors
///
/// Returns [`RegistryError`] when a manifest is missing, duplicated, malformed, stale, or has no
/// tool/protocol contract.
pub fn validate_harness_registry() -> Result<(), RegistryError> {
    if REGISTRY.len() != HarnessKind::ALL.len() {
        return Err(RegistryError::Count {
            expected: HarnessKind::ALL.len(),
            actual: REGISTRY.len(),
        });
    }
    if embedded_manifest_sources().len() != HarnessKind::ALL.len() {
        return Err(RegistryError::ManifestCount {
            expected: HarnessKind::ALL.len(),
            actual: embedded_manifest_sources().len(),
        });
    }
    let mut kinds = BTreeSet::new();
    for registration in REGISTRY {
        if !kinds.insert(registration.kind) {
            return Err(RegistryError::Duplicate(registration.kind));
        }
        if registration.binary_name() != registration.kind.binary_name() {
            return Err(RegistryError::BinaryMapping {
                kind: registration.kind,
                actual: registration.binary_name().to_owned(),
                expected: registration.kind.binary_name().to_owned(),
            });
        }
        let manifest = registration.manifest()?;
        if manifest.harness != registration.kind {
            return Err(RegistryError::ManifestIdentity {
                kind: registration.kind,
                manifest: manifest.harness,
            });
        }
        if manifest.tool_names().is_empty() {
            return Err(RegistryError::EmptyInventory(registration.kind));
        }
        let external = manifest
            .tools
            .iter()
            .filter(|entry| entry.coverage == Coverage::ExternalAuthentication)
            .count();
        if registration.kind == HarnessKind::ClaudeCode && external != 1 {
            return Err(RegistryError::ScenarioContract {
                kind: registration.kind,
                message: "Claude must declare exactly one external-authentication scenario".into(),
            });
        }
        if registration.kind == HarnessKind::ClaudeCode {
            let Some(external_entry) = manifest
                .tools
                .iter()
                .find(|entry| entry.coverage == Coverage::ExternalAuthentication)
            else {
                return Err(RegistryError::ScenarioContract {
                    kind: registration.kind,
                    message: "Claude DesignSync scenario must be embedded".into(),
                });
            };
            if external_entry.name != "DesignSync"
                || embedded_tool_scenario(registration.kind, &external_entry.scenario).is_err()
            {
                return Err(RegistryError::ScenarioContract {
                    kind: registration.kind,
                    message: "Claude DesignSync scenario must be embedded".into(),
                });
            }
        }
        if registration.kind != HarnessKind::ClaudeCode && external != 0 {
            return Err(RegistryError::ScenarioContract {
                kind: registration.kind,
                message: "only Claude may declare an external-authentication scenario".into(),
            });
        }
    }
    for kind in HarnessKind::ALL {
        if !kinds.contains(&kind) {
            return Err(RegistryError::Missing(kind));
        }
        let source_count = embedded_manifest_sources()
            .iter()
            .filter(|(source_kind, _)| *source_kind == kind)
            .count();
        if source_count != 1 {
            return Err(RegistryError::ManifestIdentityCount {
                kind,
                actual: source_count,
            });
        }
    }
    Ok(())
}

/// Builds a clean command prefix for a conformance test process.
#[must_use]
pub fn conformance_command(
    nan_harness: impl Into<PathBuf>,
    harness: HarnessKind,
    workspace: impl AsRef<Path>,
    provider_base_url: &str,
) -> TerminalCommand {
    TerminalCommand::new(nan_harness, workspace.as_ref())
        .clear_environment()
        .args([
            OsString::from(harness.binary_name()),
            OsString::from("--provider-base-url"),
            OsString::from(provider_base_url),
            OsString::from("--"),
        ])
        .env("CI", "1")
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("NAN_API_KEY", TEST_CREDENTIAL)
        .env("NAN_NO_COMPATIBILITY_CHECK", "1")
        .env("NAN_NO_UPDATE_CHECK", "1")
        .env(
            "NAN_HARNESS_CONFIG_DIR",
            workspace.as_ref().join("nan-config"),
        )
        .env("HOME", workspace.as_ref().join("home"))
        .timeout(WRAPPER_TIMEOUT)
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RegistryError {
    #[error("harness registry contains {actual} entries; expected {expected}")]
    Count { expected: usize, actual: usize },
    #[error("embedded conformance source contains {actual} manifests; expected {expected}")]
    ManifestCount { expected: usize, actual: usize },
    #[error("harness registry contains duplicate {0}")]
    Duplicate(HarnessKind),
    #[error("harness registry is missing {0}")]
    Missing(HarnessKind),
    #[error("harness registry has no inventory for {0}")]
    EmptyInventory(HarnessKind),
    #[error("harness registry maps {kind} to binary '{actual}', expected '{expected}'")]
    BinaryMapping {
        kind: HarnessKind,
        actual: String,
        expected: String,
    },
    #[error("manifest for {kind} identifies itself as {manifest}")]
    ManifestIdentity {
        kind: HarnessKind,
        manifest: HarnessKind,
    },
    #[error("manifest for {kind} appears {actual} times in embedded sources")]
    ManifestIdentityCount { kind: HarnessKind, actual: usize },
    #[error("could not load manifest for {kind}: {message}")]
    Manifest { kind: HarnessKind, message: String },
    #[error("invalid scenario contract for {kind}: {message}")]
    ScenarioContract { kind: HarnessKind, message: String },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ConformanceStatus {
    Passed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ConformanceOutcome {
    Passed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConformanceCheck {
    pub name: String,
    pub status: ConformanceStatus,
    pub duration_milliseconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConformanceScenario {
    pub name: String,
    pub status: ConformanceStatus,
    pub checks: Vec<ConformanceCheck>,
    pub duration_milliseconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConformanceReport {
    pub schema_version: u8,
    pub harness: HarnessKind,
    pub scenarios: Vec<ConformanceScenario>,
    pub outcome: ConformanceOutcome,
    pub duration_milliseconds: u64,
}

impl ConformanceReport {
    #[must_use]
    pub fn is_success(&self) -> bool {
        self.outcome == ConformanceOutcome::Passed
    }

    /// Validates the bounded public report shape before serialization.
    ///
    /// # Errors
    ///
    /// Returns [`ReportShapeError`] when a report contains an unbounded or unknown scenario.
    pub fn validate_shape(&self) -> Result<(), ReportShapeError> {
        if self.schema_version != CONFORMANCE_SCHEMA_VERSION {
            return Err(ReportShapeError::Schema(self.schema_version));
        }
        if self.scenarios.len() > MAX_REPORT_SCENARIOS {
            return Err(ReportShapeError::TooManyScenarios(self.scenarios.len()));
        }
        if self.duration_milliseconds > MAX_DURATION_MILLISECONDS {
            return Err(ReportShapeError::Duration(self.duration_milliseconds));
        }
        for scenario in &self.scenarios {
            validate_report_name(&scenario.name)?;
            if scenario.checks.is_empty() || scenario.checks.len() > MAX_REPORT_CHECKS {
                return Err(ReportShapeError::Checks(scenario.name.clone()));
            }
            if scenario.duration_milliseconds > MAX_DURATION_MILLISECONDS {
                return Err(ReportShapeError::Duration(scenario.duration_milliseconds));
            }
            for check in &scenario.checks {
                validate_report_name(&check.name)?;
                if check.duration_milliseconds > MAX_DURATION_MILLISECONDS {
                    return Err(ReportShapeError::Duration(check.duration_milliseconds));
                }
            }
        }
        Ok(())
    }
}

fn validate_report_name(name: &str) -> Result<(), ReportShapeError> {
    if name.is_empty() || name.len() > MAX_REPORT_NAME_BYTES {
        Err(ReportShapeError::Name)
    } else {
        Ok(())
    }
}

fn validate_published_scenario_set(
    scenarios: &[ConformanceScenario],
) -> Result<(), ReportShapeError> {
    let names = scenarios
        .iter()
        .map(|scenario| scenario.name.as_str())
        .collect::<BTreeSet<_>>();
    if names.len() != PUBLISHED_SCENARIO_NAMES.len()
        || PUBLISHED_SCENARIO_NAMES
            .iter()
            .any(|name| !names.contains(name))
    {
        return Err(ReportShapeError::ScenarioSet);
    }
    Ok(())
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ReportShapeError {
    #[error("unsupported conformance report schema version {0}")]
    Schema(u8),
    #[error("conformance report contains too many scenarios: {0}")]
    TooManyScenarios(usize),
    #[error("conformance report contains an invalid duration: {0}")]
    Duration(u64),
    #[error("conformance report contains an invalid name")]
    Name,
    #[error("conformance scenario '{0}' has an invalid check list")]
    Checks(String),
    #[error("published conformance report is missing a required scenario")]
    ScenarioSet,
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
        let scenarios = vec![
            self.run_inventory(registration).await,
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

    async fn run_inventory(&self, registration: HarnessRegistration) -> ConformanceScenario {
        let started = Instant::now();
        let Ok(manifest) = registration.manifest() else {
            return failed_scenario("inventory", started);
        };
        let Ok(workspace) = ConformanceWorkspace::create() else {
            return failed_scenario("inventory", started);
        };
        let Ok(mut daemon) = PrimeDaemonGuard::for_harness(registration.kind, workspace.path())
        else {
            return failed_scenario("inventory", started);
        };
        let Ok(provider) =
            ScriptedProvider::start(ProviderScenario::inventory(INVENTORY_MARKER)).await
        else {
            let _ = daemon.cleanup().await;
            return failed_scenario("inventory", started);
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
        let passed = output.as_ref().is_ok_and(|output| {
            output.status.success()
                && output.stdout.contains(INVENTORY_MARKER)
                && !requests.is_empty()
                && provider_complete
                && provider_bounded
                && provider_shutdown
                && daemon_clean
                && inventory_matches(
                    registration.kind,
                    &manifest,
                    &requests
                        .iter()
                        .filter_map(tool_names)
                        .flatten()
                        .collect::<BTreeSet<_>>(),
                )
        });
        let status = if passed {
            ConformanceStatus::Passed
        } else {
            ConformanceStatus::Failed
        };
        scenario("inventory", status, started)
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
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
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
        if matches!(registration.kind, HarnessKind::Pi | HarnessKind::PrimeAgent) {
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

const HERMES_OPTIONAL_CREDENTIALS_CLEARED: &[(&str, &str)] = &[
    ("BFL_API_KEY", ""),
    ("ELEVENLABS_API_KEY", ""),
    ("FAL_KEY", ""),
    ("OPENAI_API_KEY", ""),
    ("XAI_API_KEY", ""),
];

const OPENCLAW_MEDIA_CREDENTIALS_CLEARED: &[(&str, &str)] = &[
    ("AZURE_OPENAI_API_KEY", ""),
    ("BFL_API_KEY", ""),
    ("DEEPINFRA_API_KEY", ""),
    ("FAL_KEY", ""),
    ("GEMINI_API_KEY", ""),
    ("GOOGLE_API_KEY", ""),
    ("MINIMAX_API_KEY", ""),
    ("OPENAI_API_KEY", ""),
    ("OPENROUTER_API_KEY", ""),
    ("VYDRA_API_KEY", ""),
    ("XAI_API_KEY", ""),
];

#[derive(Debug, Clone)]
enum RunKind {
    Inventory,
    Tool(ScriptedToolCall),
    Sentinel,
    External {
        tool: String,
        arguments: Vec<String>,
        enabled_tools: Vec<String>,
    },
}

#[allow(clippy::too_many_lines)]
fn headless_arguments(
    kind: HarnessKind,
    run_kind: &RunKind,
    marker: &str,
    workspace: &Path,
) -> Vec<OsString> {
    let is_inventory = matches!(run_kind, RunKind::Inventory | RunKind::Sentinel);
    let prompt = match run_kind {
        RunKind::Inventory | RunKind::Sentinel => {
            format!("Reply exactly {marker} without using tools.")
        }
        RunKind::Tool(tool) => format!(
            "Use the {} tool exactly once, wait for its result, then reply exactly {marker}.",
            tool.name
        ),
        RunKind::External { tool, .. } => format!(
            "Run the deterministic {tool} authorization scenario, report its controlled prerequisite, then reply exactly {marker}."
        ),
    };
    let mut arguments = match kind {
        HarnessKind::ClaudeCode => vec![
            "-p".into(),
            prompt.into(),
            "--permission-mode".into(),
            "bypassPermissions".into(),
            "--output-format".into(),
            "stream-json".into(),
            "--verbose".into(),
            "--no-session-persistence".into(),
            "--max-turns".into(),
            "12".into(),
        ],
        HarnessKind::Codex => vec![
            "exec".into(),
            "--skip-git-repo-check".into(),
            "--ephemeral".into(),
            "--dangerously-bypass-approvals-and-sandbox".into(),
            "--json".into(),
            prompt.into(),
        ],
        HarnessKind::OpenCode => vec![
            "run".into(),
            "--pure".into(),
            "--format".into(),
            "json".into(),
            "--auto".into(),
            prompt.into(),
        ],
        HarnessKind::Hermes => vec![
            "chat".into(),
            "--query".into(),
            prompt.into(),
            "--quiet".into(),
            "--yolo".into(),
            "--safe-mode".into(),
            "--source".into(),
            "tool".into(),
            "--max-turns".into(),
            "12".into(),
        ],
        HarnessKind::Pi | HarnessKind::PrimeAgent => vec![
            "--mode".into(),
            "json".into(),
            "--print".into(),
            "--no-session".into(),
            "--no-extensions".into(),
            "--no-skills".into(),
            "--no-prompt-templates".into(),
            "--no-themes".into(),
            "--no-context-files".into(),
            "--tools".into(),
            if kind == HarnessKind::PrimeAgent {
                "ipython".into()
            } else {
                "read,bash,edit,write,grep,find,ls".into()
            },
            prompt.into(),
        ],
        HarnessKind::DeepSeekHarness => vec!["--profile".into(), "headless".into(), prompt.into()],
        HarnessKind::OpenClaw => vec![
            "agent".into(),
            "--local".into(),
            "--session-id".into(),
            "nan-harness-conformance".into(),
            "--message".into(),
            prompt.into(),
            "--json".into(),
        ],
        HarnessKind::Cline => vec![
            "--json".into(),
            "--timeout".into(),
            "60".into(),
            prompt.into(),
        ],
        HarnessKind::QwenCode => vec![
            "--safe-mode".into(),
            "--prompt".into(),
            prompt.into(),
            "--output-format".into(),
            "json".into(),
        ],
        HarnessKind::KimiCode => vec![
            "--prompt".into(),
            prompt.into(),
            "--output-format".into(),
            "stream-json".into(),
        ],
        HarnessKind::Aider => {
            if matches!(run_kind, RunKind::Tool(_)) {
                vec![
                    "--message".into(),
                    format!("Replace the entire file with {ROUND_TRIP_MARKER}.").into(),
                    "--yes-always".into(),
                    "--no-auto-commits".into(),
                    "--no-git".into(),
                    "--edit-format".into(),
                    "whole".into(),
                    "--no-show-model-warnings".into(),
                    "--no-check-update".into(),
                    "--map-tokens".into(),
                    "0".into(),
                    "edit-target.txt".into(),
                ]
            } else {
                vec![
                    "--message".into(),
                    prompt.clone().into(),
                    "--yes-always".into(),
                    "--no-auto-commits".into(),
                    "--no-git".into(),
                    "--no-show-model-warnings".into(),
                    "--no-check-update".into(),
                    "--map-tokens".into(),
                    "0".into(),
                ]
            }
        }
        HarnessKind::Goose => vec![
            "run".into(),
            "--no-profile".into(),
            "--no-session".into(),
            "--with-builtin".into(),
            "developer".into(),
            "--output-format".into(),
            "json".into(),
            "--text".into(),
            prompt.into(),
        ],
        HarnessKind::Fx => vec![
            "ask".into(),
            "--yolo".into(),
            "--no-save".into(),
            "--no-color".into(),
            prompt.into(),
        ],
    };
    if kind == HarnessKind::ClaudeCode && !is_inventory {
        let (enabled_tools, scenario_arguments) = match run_kind {
            RunKind::External {
                enabled_tools,
                arguments,
                ..
            } => (enabled_tools.clone(), arguments.clone()),
            RunKind::Tool(tool) => (vec![tool.name.clone()], Vec::new()),
            RunKind::Inventory | RunKind::Sentinel => (Vec::new(), Vec::new()),
        };
        let enabled_tools = enabled_tools.join(",");
        arguments.extend([
            OsString::from("--tools"),
            OsString::from(enabled_tools.clone()),
            OsString::from("--allowedTools"),
            OsString::from(enabled_tools),
        ]);
        arguments.extend(scenario_arguments.into_iter().map(OsString::from));
    }
    if kind == HarnessKind::QwenCode && matches!(run_kind, RunKind::Tool(_)) {
        arguments.extend([
            OsString::from("--allowed-tools"),
            OsString::from("read_file"),
        ]);
    }
    if kind == HarnessKind::PrimeAgent {
        let socket = workspace.join("home/prime-agent.sock");
        arguments.extend([OsString::from("--daemon-socket"), socket.into_os_string()]);
    }
    arguments
}

#[derive(Debug, Clone)]
struct FilesystemContract {
    path: PathBuf,
    text: String,
    must_change: bool,
    before: Option<String>,
}

#[derive(Debug, Clone)]
struct RoundTripProbe {
    call: ScriptedToolCall,
    filesystem: FilesystemContract,
}

#[derive(Debug, Error)]
#[error("round-trip probe tool '{tool}' is not declared by the {kind} manifest")]
struct ProbeSelectionError {
    kind: HarnessKind,
    tool: String,
}

#[allow(clippy::too_many_lines)]
fn round_trip_probe(
    kind: HarnessKind,
    workspace: &Path,
    manifest: &ConformanceManifest,
) -> Result<RoundTripProbe, ProbeSelectionError> {
    let read_path = workspace.join("read-target.txt");
    let read_path_string = read_path.to_string_lossy().into_owned();
    let (name, input, filesystem) = match kind {
        HarnessKind::ClaudeCode => (
            "Write",
            json!({
                "file_path": workspace.join("tool-output.txt"),
                "content": ROUND_TRIP_MARKER
            }),
            filesystem_contract(workspace.join("tool-output.txt"), ROUND_TRIP_MARKER, true),
        ),
        HarnessKind::Codex => (
            "exec_command",
            json!({"cmd": "printf NAN_HARNESS_TOOL_OK > tool-output.txt"}),
            filesystem_contract(
                workspace.join("tool-output.txt"),
                "NAN_HARNESS_TOOL_OK",
                true,
            ),
        ),
        HarnessKind::OpenCode => (
            "bash",
            json!({"command": "printf NAN_HARNESS_TOOL_OK > tool-output.txt"}),
            filesystem_contract(
                workspace.join("tool-output.txt"),
                "NAN_HARNESS_TOOL_OK",
                true,
            ),
        ),
        HarnessKind::Hermes | HarnessKind::Fx => (
            "write_file",
            json!({
                "path": workspace.join("tool-output.txt"),
                "content": ROUND_TRIP_MARKER
            }),
            filesystem_contract(workspace.join("tool-output.txt"), ROUND_TRIP_MARKER, true),
        ),
        HarnessKind::Pi | HarnessKind::OpenClaw => (
            "write",
            json!({
                "path": workspace.join("tool-output.txt"),
                "content": ROUND_TRIP_MARKER
            }),
            filesystem_contract(workspace.join("tool-output.txt"), ROUND_TRIP_MARKER, true),
        ),
        HarnessKind::PrimeAgent => (
            "ipython",
            json!({"code": format!("open('tool-output.txt','w').write('{ROUND_TRIP_MARKER}')")}),
            filesystem_contract(workspace.join("tool-output.txt"), ROUND_TRIP_MARKER, true),
        ),
        HarnessKind::DeepSeekHarness => (
            "write",
            json!({
                "file_path": workspace.join("tool-output.txt"),
                "content": ROUND_TRIP_MARKER
            }),
            filesystem_contract(workspace.join("tool-output.txt"), ROUND_TRIP_MARKER, true),
        ),
        HarnessKind::Cline => (
            "run_commands",
            json!({
                "commands": [format!(
                    "printf NAN_HARNESS_TOOL_OK > '{}'",
                    workspace.join("tool-output.txt").display()
                )]
            }),
            filesystem_contract(
                workspace.join("tool-output.txt"),
                "NAN_HARNESS_TOOL_OK",
                true,
            ),
        ),
        HarnessKind::QwenCode => (
            "read_file",
            json!({"file_path": read_path_string}),
            filesystem_contract(read_path, "READ_TARGET_CONTENT\n", false),
        ),
        HarnessKind::KimiCode => (
            "Write",
            json!({"path": "tool-output.txt", "content": ROUND_TRIP_MARKER}),
            filesystem_contract(workspace.join("tool-output.txt"), ROUND_TRIP_MARKER, true),
        ),
        HarnessKind::Aider => (
            "edit-protocol",
            json!({}),
            filesystem_contract(workspace.join("edit-target.txt"), ROUND_TRIP_MARKER, true),
        ),
        HarnessKind::Goose => (
            "write",
            json!({"path": "round-trip.txt", "content": "NAN_HARNESS_TOOL_OK\n"}),
            filesystem_contract(
                workspace.join("round-trip.txt"),
                "NAN_HARNESS_TOOL_OK\n",
                true,
            ),
        ),
    };
    if !manifest.tool_names().contains(name) {
        return Err(ProbeSelectionError {
            kind,
            tool: name.to_owned(),
        });
    }
    Ok(RoundTripProbe {
        call: ScriptedToolCall {
            name: name.to_owned(),
            input,
            result_expected: true,
        },
        filesystem,
    })
}

fn filesystem_contract(path: PathBuf, text: &str, must_change: bool) -> FilesystemContract {
    FilesystemContract {
        path,
        text: text.to_owned(),
        must_change,
        before: must_change.then(|| "EDIT_TARGET_BEFORE\n".to_owned()),
    }
}

fn verify_probe_side_effect(probe: &RoundTripProbe) -> Result<(), ProbeAssertionError> {
    let contents = fs::read_to_string(&probe.filesystem.path)
        .map_err(|error| ProbeAssertionError::Filesystem(error.to_string()))?;
    if !contents.contains(&probe.filesystem.text) {
        return Err(ProbeAssertionError::MissingFilesystemSideEffect(
            probe.filesystem.path.clone(),
        ));
    }
    if probe.filesystem.must_change
        && probe
            .filesystem
            .before
            .as_ref()
            .is_some_and(|before| contents == *before)
    {
        return Err(ProbeAssertionError::MissingFilesystemSideEffect(
            probe.filesystem.path.clone(),
        ));
    }
    Ok(())
}

fn timeout_for(kind: HarnessKind) -> Duration {
    if kind == HarnessKind::KimiCode {
        KIMI_TIMEOUT
    } else {
        WRAPPER_TIMEOUT.saturating_sub(PROVIDER_CLEANUP_MARGIN)
    }
}

fn inventory_matches(
    kind: HarnessKind,
    manifest: &ConformanceManifest,
    actual: &BTreeSet<String>,
) -> bool {
    if kind == HarnessKind::Aider {
        return actual.is_empty();
    }
    let expected = manifest.tool_names();
    if kind == HarnessKind::Hermes {
        let required = manifest
            .inventory
            .iter()
            .map(String::as_str)
            .chain(manifest.tools.iter().flat_map(ToolManifestEntry::names))
            .map(ToOwned::to_owned)
            .collect::<BTreeSet<_>>();
        let optional = manifest
            .optional_inventory
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let dynamic = actual
            .difference(&required)
            .filter(|name| !optional.contains(*name))
            .cloned()
            .collect::<BTreeSet<_>>();
        let configured_variant = manifest
            .dynamic_inventory
            .iter()
            .any(|variant| variant.iter().cloned().collect::<BTreeSet<_>>() == dynamic);
        return required.is_subset(actual)
            && actual.iter().all(|name| {
                required.contains(name) || optional.contains(name) || dynamic.contains(name)
            })
            && configured_variant;
    }
    let required = manifest.inventory.iter().all(|name| actual.contains(name))
        && manifest
            .tools
            .iter()
            .all(|entry| entry.names().any(|name| actual.contains(name)));
    required && actual.is_subset(&expected)
}

/// Builds a scripted tool call for a deterministic conformance scenario.
#[must_use]
pub fn call(name: &str, input: Value) -> ScriptedToolCall {
    ScriptedToolCall {
        name: name.to_owned(),
        input,
        result_expected: true,
    }
}

/// Finds a tool result in provider requests, accepting punctuation differences in its ID.
///
/// Text blocks in provider-native content arrays are joined with newlines so callers can apply
/// the same assertions to string and structured content responses.
#[must_use]
pub fn tool_result(requests: &[Value], tool_call_id: &str) -> Option<String> {
    requests.iter().find_map(|request| {
        request
            .get("messages")
            .and_then(Value::as_array)
            .and_then(|messages| {
                messages.iter().find_map(|message| {
                    let matches = message.get("role").and_then(Value::as_str) == Some("tool")
                        && message
                            .get("tool_call_id")
                            .and_then(Value::as_str)
                            .is_some_and(|actual| tool_call_ids_match(actual, tool_call_id));
                    matches.then(|| {
                        message
                            .get("content")
                            .map_or_else(|| message.to_string(), message_content)
                    })
                })
            })
    })
}

/// Reports whether a serialized tool result represents an error.
///
/// Both quoted and unquoted textual error results are accepted, alongside the structured error
/// shapes emitted by the supported provider protocols.
#[must_use]
pub fn tool_result_failed(result: &str) -> bool {
    let normalized = result.trim_matches('"').trim_start().to_ascii_lowercase();
    if normalized.starts_with("error") || normalized.starts_with("<system>error:") {
        return true;
    }
    let Ok(value) = serde_json::from_str::<Value>(result) else {
        return false;
    };
    value.get("isError").and_then(Value::as_bool) == Some(true)
        || value
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(|status| matches!(status, "error" | "failed"))
        || value.get("error").is_some_and(|error| !error.is_null())
}

/// Writes a text fixture beneath a conformance workspace.
///
/// # Panics
///
/// Panics if the fixture path has no parent or its directory or contents cannot be written.
pub fn write_fixture(workspace: &Path, relative_path: &str, content: &str) {
    let path = workspace.join(relative_path);
    fs::create_dir_all(path.parent().expect("fixture should have a parent"))
        .expect("fixture directory should exist");
    fs::write(path, content).expect("fixture should be written");
}

/// Asserts that a conformance fixture exists and contains the expected text.
///
/// # Panics
///
/// Panics if the fixture cannot be read or does not contain `expected`.
pub fn assert_file(workspace: &Path, relative_path: &str, expected: &str) {
    let path = workspace.join(relative_path);
    let content = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "expected conformance file '{}' should exist: {error}",
            path.display()
        )
    });
    assert!(content.contains(expected), "file content was {content:?}");
}

/// Extracts tool names from one provider request, accepting `OpenAI` and native tool shapes.
#[must_use]
pub fn tool_names(request: &Value) -> Option<BTreeSet<String>> {
    request
        .get("tools")?
        .as_array()
        .map(|tools| {
            tools
                .iter()
                .filter_map(|tool| {
                    tool.pointer("/function/name")
                        .or_else(|| tool.get("name"))
                        .and_then(Value::as_str)
                })
                .map(ToOwned::to_owned)
                .collect()
        })
        .filter(|tools: &BTreeSet<String>| !tools.is_empty())
}

/// Asserts an exact native tool inventory.
///
/// # Panics
///
/// Panics if `actual` does not exactly match `expected`.
pub fn assert_inventory(actual: &BTreeSet<String>, expected: &[&str]) {
    let expected = expected
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, &expected);
}

/// Asserts that a harness completed successfully without emitting a diagnostic.
///
/// # Panics
///
/// Panics if the harness failed or its standard output contains an `NH-` diagnostic.
pub fn assert_success(output: &TerminalOutput) {
    assert!(output.status.success(), "{}", output.diagnostic());
    assert!(!output.stdout.contains("NH-"), "{}", output.diagnostic());
}

fn tool_call_ids_match(left: &str, right: &str) -> bool {
    left.chars()
        .filter(char::is_ascii_alphanumeric)
        .eq(right.chars().filter(char::is_ascii_alphanumeric))
}

fn message_content(content: &Value) -> String {
    content.as_str().map_or_else(
        || {
            content.as_array().map_or_else(
                || content.to_string(),
                |blocks| {
                    blocks
                        .iter()
                        .map(|block| {
                            block
                                .get("text")
                                .and_then(Value::as_str)
                                .map_or_else(|| block.to_string(), ToOwned::to_owned)
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                },
            )
        },
        ToOwned::to_owned,
    )
}

fn verify_expectation(expectation: &Expectation) -> Result<(), String> {
    match expectation {
        Expectation::None => Ok(()),
        Expectation::FileContains { path, text } => {
            let contents = fs::read_to_string(path).map_err(|error| error.to_string())?;
            contents
                .contains(text)
                .then_some(())
                .ok_or_else(|| "file expectation was not met".to_owned())
        }
        Expectation::FileMissing { path } if !Path::new(path).exists() => Ok(()),
        Expectation::FileMissing { .. } => Err("file expected to be absent exists".to_owned()),
    }
}

fn scenario(name: &str, status: ConformanceStatus, started: Instant) -> ConformanceScenario {
    let duration = duration_milliseconds(started.elapsed());
    ConformanceScenario {
        name: name.to_owned(),
        status,
        checks: vec![ConformanceCheck {
            name: "contract".to_owned(),
            status,
            duration_milliseconds: duration,
        }],
        duration_milliseconds: duration,
    }
}

fn failed_scenario(name: &str, started: Instant) -> ConformanceScenario {
    scenario(name, ConformanceStatus::Failed, started)
}

fn duration_milliseconds(duration: Duration) -> u64 {
    duration
        .as_millis()
        .try_into()
        .unwrap_or(MAX_DURATION_MILLISECONDS)
        .min(MAX_DURATION_MILLISECONDS)
}

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
struct PrimeDaemonGuard {
    socket: Option<PathBuf>,
}

impl PrimeDaemonGuard {
    fn for_harness(kind: HarnessKind, workspace: &Path) -> Result<Self, std::io::Error> {
        if kind != HarnessKind::PrimeAgent {
            return Ok(Self { socket: None });
        }
        let socket = workspace.join("home/prime-agent.sock");
        if let Some(parent) = socket.parent() {
            fs::create_dir_all(parent)?;
        }
        Ok(Self {
            socket: Some(socket),
        })
    }

    async fn cleanup(&mut self) -> Result<(), String> {
        let Some(socket) = self.socket.clone() else {
            return Ok(());
        };
        let pids = owned_prime_pids(&socket).await?;
        if pids.is_empty() {
            self.socket = None;
            return Ok(());
        }
        let mut targets = PrimeCleanupTargets::from_pids(&pids);
        signal_prime_targets(&targets, false).await?;
        match wait_for_prime_cleanup(&socket, false, PRIME_TERM_TIMEOUT).await {
            Ok(true) => {
                self.socket = None;
                return Ok(());
            }
            Ok(false) => {}
            Err(error) => {
                return Err(format!(
                    "Prime status inspection failed during cleanup: {error}"
                ));
            }
        }

        let remaining = match owned_prime_pids(&socket).await {
            Ok(pids) => pids,
            Err(error) => {
                return Err(format!(
                    "Prime status inspection failed during cleanup: {error}"
                ));
            }
        };
        if remaining.is_empty() {
            self.socket = None;
            return Ok(());
        }
        targets = PrimeCleanupTargets::from_pids(&remaining);
        signal_prime_targets(&targets, true).await?;
        match wait_for_prime_cleanup(&socket, true, PRIME_KILL_TIMEOUT).await {
            Ok(true) => {
                self.socket = None;
                Ok(())
            }
            Ok(false) => Err("owned Prime daemon remained after cleanup".to_owned()),
            Err(error) => Err(format!(
                "Prime status inspection failed during cleanup: {error}"
            )),
        }
    }
}

#[derive(Debug, Default)]
struct PrimeCleanupTargets {
    pids: BTreeSet<u32>,
}

impl PrimeCleanupTargets {
    fn from_pids(pids: &[u32]) -> Self {
        Self {
            pids: pids.iter().copied().collect(),
        }
    }
}

async fn wait_for_prime_cleanup(
    socket: &Path,
    force: bool,
    timeout: Duration,
) -> Result<bool, String> {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining == Duration::ZERO {
            return Ok(false);
        }
        let pids = tokio::time::timeout(remaining, owned_prime_pids(socket))
            .await
            .map_err(|_| {
                "Prime daemon status inspection exceeded its cleanup deadline".to_owned()
            })??;
        if pids.is_empty() {
            return Ok(true);
        }
        signal_prime_targets(&PrimeCleanupTargets::from_pids(&pids), force).await?;
        let sleep_for = remaining.min(Duration::from_millis(50));
        tokio::time::sleep(sleep_for).await;
    }
}

async fn owned_prime_pids(socket: &Path) -> Result<Vec<u32>, String> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let current_directory = socket.parent().unwrap_or_else(|| Path::new("."));
    let output = TerminalCommand::new("prime-agent", current_directory)
        .clear_environment()
        .env("PATH", path)
        .args(["status", "--json"])
        .timeout(PRIME_STATUS_TIMEOUT)
        .run()
        .await
        .map_err(|error| format!("could not inspect Prime daemons: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "Prime daemon status command failed: {}",
            output.diagnostic()
        ));
    }
    let value: Value = serde_json::from_str(&output.stdout)
        .map_err(|error| format!("could not parse Prime daemon status: {error}"))?;
    owned_prime_pids_from_status(&value, socket)
}

fn owned_prime_pids_from_status(value: &Value, socket: &Path) -> Result<Vec<u32>, String> {
    let entries = value
        .as_array()
        .or_else(|| value.get("daemons").and_then(Value::as_array))
        .ok_or_else(|| "Prime daemon status did not contain a daemon list".to_owned())?;
    Ok(entries
        .iter()
        .filter(|entry| {
            entry.get("socketPath").and_then(Value::as_str)
                == Some(socket.to_string_lossy().as_ref())
        })
        .filter_map(|entry| entry.get("pid").and_then(Value::as_u64))
        .filter(|pid| *pid > 1)
        .filter_map(|pid| u32::try_from(pid).ok())
        .collect())
}

#[cfg(unix)]
fn signal_prime_targets_now(targets: &PrimeCleanupTargets, force: bool) -> Result<(), String> {
    use nix::errno::Errno;
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    let signal = if force {
        Signal::SIGKILL
    } else {
        Signal::SIGTERM
    };
    for pid in &targets.pids {
        let pid = i32::try_from(*pid).map_err(|_| "Prime pid was out of range".to_owned())?;
        match kill(Pid::from_raw(pid), signal) {
            Ok(()) | Err(Errno::ESRCH) => {}
            Err(error) => {
                return Err(format!("could not signal owned Prime daemon: {error}"));
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
async fn signal_prime_targets(targets: &PrimeCleanupTargets, force: bool) -> Result<(), String> {
    tokio::task::yield_now().await;
    signal_prime_targets_now(targets, force)
}

#[cfg(not(unix))]
async fn signal_prime_targets(targets: &PrimeCleanupTargets, _force: bool) -> Result<(), String> {
    for pid in &targets.pids {
        let output = TerminalCommand::new("taskkill", ".")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .timeout(PRIME_STATUS_TIMEOUT)
            .run()
            .await
            .map_err(|error| format!("could not terminate owned Prime daemon: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "taskkill could not terminate owned Prime daemon: {}",
                output.diagnostic()
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        CONFORMANCE_SCHEMA_VERSION, ConformanceOutcome, ConformanceReport, ConformanceStatus,
        HarnessRegistration, harness_registry, inventory_matches, owned_prime_pids_from_status,
        round_trip_probe, tool_result, tool_result_failed, validate_harness_registry,
    };
    #[cfg(unix)]
    use super::{PrimeCleanupTargets, signal_prime_targets_now};
    use nan_harness_core::HarnessKind;
    use serde_json::json;
    use std::path::Path;

    #[cfg(unix)]
    #[tokio::test]
    async fn conformance_command_replaces_a_parent_api_key() {
        use std::os::unix::fs::PermissionsExt;

        let workspace = tempfile::tempdir().expect("workspace should exist");
        let script = workspace.path().join("assert-environment.sh");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\n[ \"$NAN_API_KEY\" = \"{}\" ]\n[ \"$NAN_NO_UPDATE_CHECK\" = 1 ]\n",
                super::TEST_CREDENTIAL
            ),
        )
        .expect("environment assertion script should be written");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700))
            .expect("environment assertion script should be executable");
        let output = super::conformance_command(
            script,
            HarnessKind::Fx,
            workspace.path(),
            "http://127.0.0.1:1/v1",
        )
        .run()
        .await
        .expect("environment assertion command should run");
        assert!(output.status.success(), "{}", output.diagnostic());
    }

    #[test]
    fn registry_covers_every_harness_kind_and_manifest() {
        validate_harness_registry().expect("the conformance registry should be complete");
        let kinds = harness_registry()
            .iter()
            .map(|registration| registration.kind)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(kinds.len(), HarnessKind::ALL.len());
        assert!(HarnessKind::ALL.iter().all(|kind| kinds.contains(kind)));
        for registration in harness_registry() {
            assert_eq!(registration.binary_name(), registration.kind.binary_name());
            assert_eq!(
                registration.manifest().expect("embedded manifest").harness,
                registration.kind
            );
        }
    }

    #[test]
    fn codex_inventory_accepts_version_dependent_native_tools() {
        let manifest = harness_registry()
            .iter()
            .find(|registration| registration.kind == HarnessKind::Codex)
            .expect("Codex registration should exist")
            .manifest()
            .expect("Codex manifest should parse");
        let baseline = ["apply_patch", "exec_command", "update_plan", "write_stdin"]
            .into_iter()
            .map(ToOwned::to_owned)
            .collect();
        assert!(inventory_matches(HarnessKind::Codex, &manifest, &baseline));

        let current = manifest.tool_names();
        assert!(inventory_matches(HarnessKind::Codex, &manifest, &current));
    }

    #[test]
    fn hermes_inventory_accepts_only_declared_dynamic_variants() {
        let manifest = harness_registry()
            .iter()
            .find(|registration| registration.kind == HarnessKind::Hermes)
            .expect("Hermes registration should exist")
            .manifest()
            .expect("Hermes manifest should parse");
        let mut browser_inventory = manifest
            .inventory
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        browser_inventory.insert("browser_exec".to_owned());
        assert!(inventory_matches(
            HarnessKind::Hermes,
            &manifest,
            &browser_inventory
        ));

        browser_inventory.insert("undeclared_tool".to_owned());
        assert!(!inventory_matches(
            HarnessKind::Hermes,
            &manifest,
            &browser_inventory
        ));
    }

    #[test]
    fn report_serialization_is_bounded_and_safe() {
        let report = ConformanceReport {
            schema_version: CONFORMANCE_SCHEMA_VERSION,
            harness: HarnessKind::ClaudeCode,
            scenarios: vec![super::scenario(
                "sentinel",
                ConformanceStatus::Passed,
                std::time::Instant::now(),
            )],
            outcome: ConformanceOutcome::Passed,
            duration_milliseconds: 3,
        };
        report.validate_shape().expect("report should be bounded");
        let encoded = serde_json::to_string(&report).expect("report should serialize");
        assert!(encoded.contains("schemaVersion"));
        assert!(encoded.contains("durationMilliseconds"));
        assert!(!encoded.contains("prompt"));
        assert!(!encoded.contains("credential"));
        assert!(!encoded.contains("tool_calls"));
        assert!(matches!(
            report.outcome,
            ConformanceOutcome::Passed | ConformanceOutcome::Failed
        ));
    }

    #[test]
    fn registry_registration_is_derived_from_canonical_identity() {
        let registration = HarnessRegistration {
            kind: HarnessKind::KimiCode,
        };
        assert_eq!(registration.binary_name(), "kimi");
    }

    #[test]
    fn published_round_trip_probes_are_declared_by_embedded_manifests() {
        let workspace = tempfile::tempdir().expect("workspace should exist");
        for registration in harness_registry() {
            let manifest = registration.manifest().expect("embedded manifest");
            let probe = round_trip_probe(registration.kind, workspace.path(), &manifest)
                .expect("published probe should satisfy the manifest contract");
            assert!(manifest.tool_names().contains(&probe.call.name));
        }
    }

    #[test]
    fn tool_result_supports_plain_and_content_block_array_results() {
        let requests = vec![
            json!({
                "messages": [{
                    "role": "tool",
                    "tool_call_id": "call_nan_harness_conformance_0",
                    "content": "plain result"
                }]
            }),
            json!({
                "messages": [{
                    "role": "tool",
                    "tool_call_id": "call-nan-harness-conformance-1",
                    "content": [
                        {"type": "text", "text": "first block"},
                        {"type": "text", "text": "second block"}
                    ]
                }]
            }),
        ];

        assert_eq!(
            tool_result(&requests, "callnan_harness_conformance0").as_deref(),
            Some("plain result")
        );
        assert_eq!(
            tool_result(&requests, "call_nan_harness_conformance_1").as_deref(),
            Some("first block\nsecond block")
        );
    }

    #[test]
    fn tool_result_returns_none_for_a_missing_identifier() {
        let requests = vec![json!({
            "messages": [{
                "role": "tool",
                "tool_call_id": "call_nan_harness_conformance_0",
                "content": "result"
            }]
        })];

        assert_eq!(tool_result(&requests, "missing"), None);
    }

    #[test]
    fn tool_result_failed_accepts_quoted_and_unquoted_error_text() {
        assert!(tool_result_failed("ERROR: tool failed"));
        assert!(tool_result_failed(r#""error: tool failed""#));
        assert!(tool_result_failed(
            "<system>ERROR: tool failed</system>\nThe file must be read first."
        ));
        assert!(tool_result_failed(r#"{"isError":true}"#));
        assert!(!tool_result_failed("tool completed successfully"));
    }

    #[test]
    fn prime_status_ownership_uses_the_exact_workspace_socket() {
        let status = json!([
            {"socketPath": "/workspace/prime-agent.sock", "pid": 42},
            {"socketPath": "/workspace/prime-agent.sock", "pid": 0},
            {"socketPath": "/other/prime-agent.sock", "pid": 43}
        ]);
        assert_eq!(
            owned_prime_pids_from_status(&status, Path::new("/workspace/prime-agent.sock"))
                .expect("status should parse"),
            vec![42]
        );
    }

    #[cfg(unix)]
    #[test]
    fn prime_cleanup_terminates_the_owned_process() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("owned test process should start");
        let targets = PrimeCleanupTargets::from_pids(&[child.id()]);
        signal_prime_targets_now(&targets, false).expect("owned process should terminate");
        let status = child.wait().expect("owned process should be reaped");
        assert!(!status.success());
    }

    #[cfg(unix)]
    #[test]
    fn prime_cleanup_does_not_signal_an_unrelated_shared_process_group_member() {
        use nix::unistd::{Pid, getpgid, getpgrp};
        use std::os::unix::process::CommandExt;
        use std::process::Stdio;

        let mut owned = std::process::Command::new("/bin/sh")
            .args(["-c", "trap '' TERM; while :; do :; done"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .expect("owned Prime-like process should start");
        let owned_pid = i32::try_from(owned.id()).expect("owned pid should fit");
        let mut unrelated = std::process::Command::new("sleep")
            .arg("30")
            .process_group(owned_pid)
            .spawn()
            .expect("unrelated process should start");

        let process_group = getpgid(Some(Pid::from_raw(owned_pid)))
            .expect("owned process group should be readable");
        assert_eq!(process_group.as_raw(), owned_pid);
        assert_ne!(process_group, getpgrp());
        assert_eq!(
            getpgid(Some(Pid::from_raw(
                i32::try_from(unrelated.id()).expect("unrelated pid should fit",)
            )))
            .expect("unrelated process group should be readable"),
            process_group
        );

        let targets = PrimeCleanupTargets::from_pids(&[owned.id()]);
        signal_prime_targets_now(&targets, false).expect("TERM should be delivered");
        signal_prime_targets_now(&targets, true).expect("KILL should be delivered");
        let status = owned.wait().expect("owned process should be reaped");
        assert!(!status.success());
        assert!(
            unrelated
                .try_wait()
                .expect("unrelated status should work")
                .is_none()
        );
        let _ = unrelated.kill();
        let _ = unrelated.wait();
    }
}
