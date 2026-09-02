mod errors;
mod execution;
mod remote;
mod reporting;
mod spec;
mod vm;
mod workspace;

use crate::app::{CellArgs, ReproduceArgs};
use crate::credentials::{self, API_KEY_ACCOUNT};
use crate::report::{CanaryOutcome, CanaryReport, FailureClass};
use errors::CellError;
use execution::execute_in_vm;
use reporting::{
    ExecutionTiming, RuntimeFailure, build_report, failed_check, preserve_private_logs, timestamp,
};
use spec::LoadedSpec;
use std::path::Path;
use std::time::{Duration, Instant};
use workspace::CellWorkspace;

#[cfg(test)]
use execution::{shell_quote, step_script, valid_prepared_image_name};
#[cfg(test)]
use remote::ssh_command;
#[cfg(test)]
use spec::{CellSpec, GuestOperatingSystem};
#[cfg(test)]
use workspace::MAX_CONFORMANCE_REPORT_SIZE;

pub(crate) async fn run(arguments: &CellArgs) -> Result<(), CellError> {
    let spec = LoadedSpec::load(&arguments.spec)?;
    let report = execute(spec, None, arguments.private_log_dir.as_deref()).await?;
    let passed = report.outcome == CanaryOutcome::Passed;
    report.write(&arguments.output)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    if passed {
        Ok(())
    } else {
        Err(CellError::CanaryFailed(arguments.output.clone()))
    }
}

pub(crate) async fn reproduce(arguments: &ReproduceArgs) -> Result<(), CellError> {
    let previous = CanaryReport::read(&arguments.report)?;
    previous.validate()?;
    let spec = LoadedSpec::load(&arguments.spec)?;
    if previous.cell_id != spec.value.id || previous.harness.id != spec.value.harness {
        return Err(CellError::ReproductionMismatch);
    }
    let report = execute(spec, previous.model, arguments.private_log_dir.as_deref()).await?;
    let passed = report.outcome == CanaryOutcome::Passed;
    report.write(&arguments.output)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    if passed {
        Ok(())
    } else {
        Err(CellError::CanaryFailed(arguments.output.clone()))
    }
}

async fn execute(
    loaded: LoadedSpec,
    model_override: Option<String>,
    private_log_directory: Option<&Path>,
) -> Result<CanaryReport, CellError> {
    let started_at = timestamp()?;
    let started = Instant::now();
    let model = model_override.or_else(|| loaded.value.model.clone());
    let workspace = CellWorkspace::prepare(&loaded)?;
    let api_key = if loaded.value.steps.iter().any(|step| step.requires_api_key) {
        Some(
            credentials::read(API_KEY_ACCOUNT)
                .await
                .map_err(CellError::ReadCredential)?,
        )
    } else {
        None
    };
    let deadline = started + Duration::from_secs(loaded.value.overall_timeout_seconds);
    let execution = tokio::select! {
        result = execute_in_vm(
            &loaded.value,
            &workspace,
            api_key.as_ref().map(|key| key.as_str()),
            deadline,
        ) => result,
        () = shutdown_signal() => Err(RuntimeFailure::new(
            FailureClass::Infrastructure,
            "host-signal",
            "the host interrupted the canary cell",
            vec![failed_check(
                "host-signal",
                started.elapsed(),
                1,
                "the host requested shutdown",
            )],
        )),
    };
    let execution = preserve_private_logs(&workspace, private_log_directory, execution);
    let expects_conformance = loaded
        .value
        .steps
        .iter()
        .any(|step| step.name == "deterministic-conformance");
    let (execution, observations) = if expects_conformance {
        match (
            execution,
            workspace.conformance_observations(loaded.value.harness),
        ) {
            (Ok(checks), Ok(observations)) => (Ok(checks), observations),
            (Ok(mut checks), Err(detail)) => {
                checks.push(failed_check(
                    "conformance-report",
                    Duration::ZERO,
                    1,
                    detail,
                ));
                (
                    Err(RuntimeFailure::new(
                        FailureClass::TestContract,
                        "conformance-report",
                        "the conformance observation report was invalid",
                        checks,
                    )),
                    Vec::new(),
                )
            }
            (Err(failure), _) => (Err(failure), Vec::new()),
        }
    } else {
        (execution, Vec::new())
    };
    let completed_at = timestamp()?;
    let harness_version = workspace
        .harness_version(&loaded.value)
        .unwrap_or_else(|| "unknown".to_owned());
    Ok(build_report(
        loaded,
        workspace,
        model,
        harness_version,
        observations,
        ExecutionTiming {
            started_at,
            completed_at,
            duration: started.elapsed(),
        },
        execution,
    ))
}

fn remaining(deadline: Instant, requested: Duration) -> Duration {
    deadline
        .saturating_duration_since(Instant::now())
        .min(requested)
        .max(Duration::from_millis(1))
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("SIGTERM handler should install");
        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                let _ = result;
            }
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CellSpec, CellWorkspace, GuestOperatingSystem, MAX_CONFORMANCE_REPORT_SIZE, shell_quote,
        ssh_command, step_script, valid_prepared_image_name,
    };
    use crate::report::{CanaryObservationKind, sha256_hex};
    use nan_harness_core::HarnessKind;
    use std::fs;

    fn workspace() -> CellWorkspace {
        let root = tempfile::tempdir().expect("temporary directory should exist");
        let input = root.path().join("input");
        let output = root.path().join("output");
        let logs = root.path().join("logs");
        fs::create_dir_all(&input).expect("input directory should exist");
        fs::create_dir_all(&output).expect("output directory should exist");
        fs::create_dir_all(&logs).expect("log directory should exist");
        CellWorkspace {
            _root: root,
            input,
            output,
            logs,
            nan_harness_sha256: sha256_hex(b"nan-harness"),
        }
    }

    #[test]
    fn prepared_image_override_accepts_only_local_tart_names() {
        assert!(valid_prepared_image_name("nhc-suite-linux-123"));
        assert!(!valid_prepared_image_name("personal-vm"));
        assert!(!valid_prepared_image_name("ghcr.io/unsafe:latest"));
        assert!(!valid_prepared_image_name(""));
    }

    #[test]
    fn shell_quoting_does_not_allow_command_injection() {
        assert_eq!(shell_quote("a'b"), "'a'\"'\"'b'");
    }

    #[test]
    fn ssh_uses_only_the_canary_password_identity() {
        let command = ssh_command("192.0.2.1");
        let arguments = command
            .as_std()
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(
            arguments
                .iter()
                .any(|argument| argument == "IdentitiesOnly=yes")
        );
        assert!(
            arguments
                .iter()
                .any(|argument| argument == "PreferredAuthentications=password")
        );
    }

    #[test]
    fn live_secret_is_only_added_to_the_remote_script() {
        let spec: CellSpec = toml::from_str(
            r#"
schema_version = 1
id = "linux-claude-live"
harness = "claude-code"
trigger = "weekly"
tier = "live-core"
scenario = "text"
image = "ghcr.io/cirruslabs/ubuntu:latest"
guest = "linux"
profile = "node-24"
harness_version_file = "version.txt"
model = "qwen3.6"

[nan_harness]
version = "0.0.6"
source = "release"
artifact = "nan-harness"

[[steps]]
name = "prompt"
script = "test -n \"$NAN_API_KEY\""
failure_class = "provider"
requires_api_key = true
"#,
        )
        .expect("spec should parse");
        let script = step_script(&spec, &spec.steps[0], Some("secret'key"));

        assert!(script.contains("export NAN_API_KEY='secret'\"'\"'key'"));
        assert!(script.contains("export NAN_CANARY_MODEL='qwen3.6'"));
        assert_eq!(spec.guest.input_path(), "/mnt/shared/nan-input");
        assert_eq!(GuestOperatingSystem::Macos.as_str(), "macos");
    }

    #[test]
    fn conformance_observations_are_loaded_from_a_bounded_report() {
        let workspace = workspace();
        fs::write(
            workspace.output.join("conformance.json"),
            serde_json::to_vec(&serde_json::json!({
                "schemaVersion": 2,
                "harness": "hermes",
                "scenarios": [{
                    "name": "inventory",
                    "status": "passed",
                    "checks": [{"name": "contract", "status": "passed", "durationMilliseconds": 1}],
                    "durationMilliseconds": 1
                }],
                "observations": [{"kind": "inventory-drift", "fingerprint": "a".repeat(64)}],
                "outcome": "passed",
                "durationMilliseconds": 1
            }))
            .expect("report should serialize"),
        )
        .expect("report should be written");

        let observations = workspace
            .conformance_observations(HarnessKind::Hermes)
            .expect("observation should load");
        assert_eq!(observations.len(), 1);
        assert_eq!(observations[0].kind, CanaryObservationKind::InventoryDrift);
        assert_eq!(observations[0].fingerprint, "a".repeat(64));
    }

    #[test]
    fn oversized_conformance_report_is_rejected() {
        let workspace = workspace();
        fs::write(
            workspace.output.join("conformance.json"),
            vec![b' '; usize::try_from(MAX_CONFORMANCE_REPORT_SIZE + 1).unwrap()],
        )
        .expect("report should be written");

        assert!(
            workspace
                .conformance_observations(HarnessKind::Hermes)
                .is_err()
        );
    }
}
