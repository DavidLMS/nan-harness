use super::arguments::{RunKind, headless_arguments};
use super::constants::{
    CONFORMANCE_SCHEMA_VERSION, HERMES_OPTIONAL_CREDENTIALS_CLEARED, KIMI_TIMEOUT,
    OPENCLAW_MEDIA_CREDENTIALS_CLEARED, PROVIDER_CLEANUP_MARGIN, PUBLISHED_SCENARIO_NAMES,
    TEST_CREDENTIAL, WRAPPER_TIMEOUT,
};
use super::helpers::duration_milliseconds;
use super::prime_cleanup::prime_status_path;
use super::registry::{
    HarnessRegistration, RegistryError, harness_registration, validate_harness_registry,
};
use super::report::{
    ConformanceOutcome, ConformanceReport, ConformanceStatus, ReportShapeError,
    validate_published_scenario_set,
};
use crate::scripted_provider::ScriptedProvider;
use crate::terminal::{TerminalCommand, TerminalOutput};
use crate::workspace::ConformanceWorkspace;
use nan_harness_core::HarnessKind;
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use thiserror::Error;

mod scenarios;

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
        let (inventory, observation) = scenarios::run_inventory(&self, registration).await;
        let scenarios = vec![
            inventory,
            scenarios::run_tool_round_trip(&self, registration).await,
            scenarios::run_sentinel(&self, registration).await,
            scenarios::run_external_prerequisite(&self, registration).await,
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
