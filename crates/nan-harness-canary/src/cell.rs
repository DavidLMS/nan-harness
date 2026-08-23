use crate::app::{CellArgs, ReproduceArgs};
use crate::credentials::{self, API_KEY_ACCOUNT};
use crate::report::{
    CanaryOutcome, CanaryReport, CanaryTier, CanaryTrigger, CheckReport, CheckStatus,
    EnvironmentEvidence, FailureClass, FailureIdentity, FailureReport, HarnessEvidence,
    NanHarnessEvidence, REPORT_SCHEMA_VERSION, RuntimeEvidence, sha256_hex,
};
use nan_harness_core::HarnessKind;
use serde::Deserialize;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tempfile::TempDir;
use thiserror::Error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tokio::io::AsyncWriteExt as _;
use tokio::process::{Child, Command};
use zeroize::Zeroizing;

const CELL_SCHEMA_VERSION: u8 = 1;
const DEFAULT_CLONE_TIMEOUT_SECONDS: u64 = 1_800;
const DEFAULT_BOOT_TIMEOUT_SECONDS: u64 = 180;
const DEFAULT_STEP_TIMEOUT_SECONDS: u64 = 300;
const DEFAULT_OVERALL_TIMEOUT_SECONDS: u64 = 1_800;
const SSH_RETRY_DELAY: Duration = Duration::from_secs(2);

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
    let completed_at = timestamp()?;
    let harness_version = workspace
        .harness_version(&loaded.value)
        .unwrap_or_else(|| "unknown".to_owned());
    Ok(build_report(
        loaded,
        workspace,
        model,
        harness_version,
        ExecutionTiming {
            started_at,
            completed_at,
            duration: started.elapsed(),
        },
        execution,
    ))
}

fn preserve_private_logs(
    workspace: &CellWorkspace,
    private_log_directory: Option<&Path>,
    execution: Result<Vec<CheckReport>, RuntimeFailure>,
) -> Result<Vec<CheckReport>, RuntimeFailure> {
    let Some(directory) = private_log_directory else {
        return execution;
    };
    if workspace.preserve_logs(directory).is_ok() {
        return execution;
    }
    let check = failed_check(
        "preserve-private-logs",
        Duration::ZERO,
        1,
        "private diagnostic logs could not be preserved",
    );
    match execution {
        Ok(mut checks) => {
            checks.push(check);
            Err(RuntimeFailure::new(
                FailureClass::Infrastructure,
                "preserve-private-logs",
                "private diagnostic logs could not be preserved",
                checks,
            ))
        }
        Err(mut failure) => {
            failure.checks.push(check);
            Err(failure)
        }
    }
}

struct ExecutionTiming {
    started_at: String,
    completed_at: String,
    duration: Duration,
}

fn build_report(
    loaded: LoadedSpec,
    workspace: CellWorkspace,
    model: Option<String>,
    harness_version: String,
    timing: ExecutionTiming,
    execution: Result<Vec<CheckReport>, RuntimeFailure>,
) -> CanaryReport {
    let (checks, failure) = match execution {
        Ok(checks) => (checks, None),
        Err(runtime_failure) => {
            let identity = FailureIdentity {
                harness: loaded.value.harness,
                harness_version: &harness_version,
                operating_system: loaded.value.guest.as_str(),
                architecture: "aarch64",
                tier: loaded.value.tier,
                scenario: &loaded.value.scenario,
            };
            let report = FailureReport::new(
                runtime_failure.class,
                runtime_failure.phase,
                None,
                runtime_failure.summary,
                &identity,
            );
            (runtime_failure.checks, Some(report))
        }
    };
    let outcome = failure.as_ref().map_or(CanaryOutcome::Passed, |failure| {
        if failure.class == FailureClass::Infrastructure {
            CanaryOutcome::InfrastructureFailure
        } else {
            CanaryOutcome::Failed
        }
    });

    CanaryReport {
        schema_version: REPORT_SCHEMA_VERSION,
        run_id: run_id(&loaded.value.id),
        cell_id: loaded.value.id,
        spec_sha256: loaded.sha256,
        trigger: loaded.value.trigger,
        tier: loaded.value.tier,
        scenario: loaded.value.scenario,
        started_at: timing.started_at,
        completed_at: timing.completed_at,
        duration_milliseconds: milliseconds(timing.duration),
        nan_harness: NanHarnessEvidence {
            version: loaded.value.nan_harness.version,
            source: loaded.value.nan_harness.source,
            sha256: workspace.nan_harness_sha256,
        },
        environment: EnvironmentEvidence {
            operating_system: loaded.value.guest.as_str().to_owned(),
            architecture: "aarch64".to_owned(),
            image: loaded.value.image,
            profile: loaded.value.profile,
            runtimes: loaded.value.runtimes,
        },
        harness: HarnessEvidence {
            id: loaded.value.harness,
            version: harness_version,
        },
        model,
        checks,
        outcome,
        failure,
    }
}

async fn execute_in_vm(
    spec: &CellSpec,
    workspace: &CellWorkspace,
    api_key: Option<&str>,
    deadline: Instant,
) -> Result<Vec<CheckReport>, RuntimeFailure> {
    let mut checks = Vec::new();
    let vm_name = vm_name(&spec.id);
    let mut lease = VmLease::new(vm_name);

    clone_vm(spec, &mut lease, &mut checks, deadline).await?;
    let execution = async {
        let ip = start_vm(spec, workspace, &mut lease, &mut checks, deadline).await?;
        mount_workspace(spec, workspace, &ip, &mut checks, deadline).await?;
        run_steps(spec, workspace, api_key, &ip, &mut checks, deadline).await
    }
    .await;
    match execution {
        Ok(()) => {
            cleanup_vm(&mut lease, &mut checks).await?;
            Ok(checks)
        }
        Err(mut failure) => {
            let cleanup_started = Instant::now();
            match lease.cleanup().await {
                Ok(()) => {
                    failure
                        .checks
                        .push(passed_check("cleanup-vm", cleanup_started.elapsed(), 1));
                }
                Err(detail) => {
                    failure.checks.push(failed_check(
                        "cleanup-vm",
                        cleanup_started.elapsed(),
                        1,
                        &detail,
                    ));
                    failure.class = FailureClass::Infrastructure;
                    "vm-cleanup".clone_into(&mut failure.phase);
                    "the Tart VM could not be destroyed after a failed check"
                        .clone_into(&mut failure.summary);
                }
            }
            Err(failure)
        }
    }
}

async fn clone_vm(
    spec: &CellSpec,
    lease: &mut VmLease,
    checks: &mut Vec<CheckReport>,
    deadline: Instant,
) -> Result<(), RuntimeFailure> {
    match run_host_command(
        "tart",
        &["clone", spec.image.as_str(), lease.name.as_str()],
        remaining(deadline, Duration::from_secs(spec.clone_timeout_seconds)),
    )
    .await
    {
        Ok(duration) => {
            lease.created = true;
            checks.push(passed_check("clone-vm", duration, 1));
        }
        Err(error) => {
            checks.push(failed_check("clone-vm", error.duration, 1, &error.detail));
            return Err(RuntimeFailure::new(
                FailureClass::Infrastructure,
                "vm-clone",
                "the Tart VM could not be cloned",
                checks.clone(),
            ));
        }
    }
    Ok(())
}

async fn start_vm(
    spec: &CellSpec,
    workspace: &CellWorkspace,
    lease: &mut VmLease,
    checks: &mut Vec<CheckReport>,
    deadline: Instant,
) -> Result<String, RuntimeFailure> {
    let run_started = Instant::now();
    lease
        .start(
            workspace,
            spec.network,
            remaining(deadline, Duration::from_secs(spec.boot_timeout_seconds)),
        )
        .await
        .map_err(|detail| {
            checks.push(failed_check("start-vm", run_started.elapsed(), 1, &detail));
            RuntimeFailure::new(
                FailureClass::Infrastructure,
                "vm-start",
                "the Tart VM could not be started",
                checks.clone(),
            )
        })?;
    checks.push(passed_check("start-vm", run_started.elapsed(), 1));

    let boot_started = Instant::now();
    let ip = wait_for_ssh(
        lease.name.as_str(),
        remaining(deadline, Duration::from_secs(spec.boot_timeout_seconds)),
    )
    .await
    .map_err(|detail| {
        checks.push(failed_check(
            "wait-for-ssh",
            boot_started.elapsed(),
            1,
            &detail,
        ));
        RuntimeFailure::new(
            FailureClass::Infrastructure,
            "vm-boot",
            "the Tart VM did not become reachable over SSH",
            checks.clone(),
        )
    })?;
    checks.push(passed_check("wait-for-ssh", boot_started.elapsed(), 1));
    Ok(ip)
}

async fn mount_workspace(
    spec: &CellSpec,
    workspace: &CellWorkspace,
    ip: &str,
    checks: &mut Vec<CheckReport>,
    deadline: Instant,
) -> Result<(), RuntimeFailure> {
    let mount_started = Instant::now();
    run_remote_script(
        ip,
        spec.guest.mount_script(),
        &workspace.log_path("mount-workspace", 1),
        remaining(deadline, Duration::from_secs(DEFAULT_STEP_TIMEOUT_SECONDS)),
    )
    .await
    .map_err(|error| {
        checks.push(failed_check(
            "mount-workspace",
            mount_started.elapsed(),
            1,
            &error,
        ));
        RuntimeFailure::new(
            FailureClass::Infrastructure,
            "vm-mount",
            "the host workspace could not be mounted inside the VM",
            checks.clone(),
        )
    })?;
    checks.push(passed_check("mount-workspace", mount_started.elapsed(), 1));
    Ok(())
}

async fn run_steps(
    spec: &CellSpec,
    workspace: &CellWorkspace,
    api_key: Option<&str>,
    ip: &str,
    checks: &mut Vec<CheckReport>,
    deadline: Instant,
) -> Result<(), RuntimeFailure> {
    for step in &spec.steps {
        let mut attempts = 0_u8;
        let mut last_failure = None;
        let step_started = Instant::now();
        while attempts < step.attempts {
            attempts += 1;
            let script = step_script(spec, step, api_key);
            let timeout = remaining(deadline, Duration::from_secs(step.timeout_seconds));
            match run_remote_script(
                ip,
                &script,
                &workspace.log_path(&step.name, attempts),
                timeout,
            )
            .await
            {
                Ok(()) => {
                    checks.push(passed_check(&step.name, step_started.elapsed(), attempts));
                    last_failure = None;
                    break;
                }
                Err(error) => {
                    last_failure = Some(error);
                    if attempts < step.attempts {
                        tokio::time::sleep(SSH_RETRY_DELAY).await;
                    }
                }
            }
        }
        if let Some(detail) = last_failure {
            checks.push(failed_check(
                &step.name,
                step_started.elapsed(),
                attempts,
                &detail,
            ));
            return Err(RuntimeFailure::new(
                step.failure_class,
                step.name.clone(),
                format!("canary step '{}' failed", step.name),
                checks.clone(),
            ));
        }
    }
    Ok(())
}

async fn cleanup_vm(
    lease: &mut VmLease,
    checks: &mut Vec<CheckReport>,
) -> Result<(), RuntimeFailure> {
    let cleanup_started = Instant::now();
    lease.cleanup().await.map_err(|detail| {
        checks.push(failed_check(
            "cleanup-vm",
            cleanup_started.elapsed(),
            1,
            &detail,
        ));
        RuntimeFailure::new(
            FailureClass::Infrastructure,
            "vm-cleanup",
            "the Tart VM could not be destroyed cleanly",
            checks.clone(),
        )
    })?;
    checks.push(passed_check("cleanup-vm", cleanup_started.elapsed(), 1));
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CellSpec {
    schema_version: u8,
    id: String,
    harness: HarnessKind,
    trigger: CanaryTrigger,
    tier: CanaryTier,
    scenario: String,
    image: String,
    guest: GuestOperatingSystem,
    #[serde(default)]
    network: GuestNetwork,
    profile: String,
    nan_harness: NanHarnessArtifact,
    #[serde(default)]
    model: Option<String>,
    #[serde(default = "default_boot_timeout")]
    boot_timeout_seconds: u64,
    #[serde(default = "default_clone_timeout")]
    clone_timeout_seconds: u64,
    #[serde(default = "default_overall_timeout")]
    overall_timeout_seconds: u64,
    harness_version_file: PathBuf,
    #[serde(default)]
    runtimes: Vec<RuntimeEvidence>,
    #[serde(default)]
    artifacts: Vec<Artifact>,
    steps: Vec<Step>,
}

impl CellSpec {
    fn validate(&self) -> Result<(), CellError> {
        if self.schema_version != CELL_SCHEMA_VERSION {
            return Err(CellError::UnsupportedSpecSchema(self.schema_version));
        }
        for (field, value) in [
            ("id", self.id.as_str()),
            ("scenario", self.scenario.as_str()),
            ("image", self.image.as_str()),
            ("profile", self.profile.as_str()),
            ("nanHarness.version", self.nan_harness.version.as_str()),
            ("nanHarness.source", self.nan_harness.source.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(CellError::EmptySpecField(field));
            }
        }
        semver::Version::parse(&self.nan_harness.version)
            .map_err(|source| CellError::InvalidNanHarnessVersion(source.to_string()))?;
        if self.steps.is_empty() {
            return Err(CellError::MissingSteps);
        }
        if self.overall_timeout_seconds == 0
            || self.clone_timeout_seconds == 0
            || self.boot_timeout_seconds == 0
            || self
                .steps
                .iter()
                .any(|step| step.timeout_seconds == 0 || step.attempts == 0)
        {
            return Err(CellError::InvalidTimeout);
        }
        validate_relative_path(&self.nan_harness.artifact, "nanHarness.artifact")?;
        validate_relative_path(&self.harness_version_file, "harnessVersionFile")?;
        for artifact in &self.artifacts {
            validate_relative_path(&artifact.source, "artifacts.source")?;
            validate_file_name(&artifact.name)?;
        }
        for step in &self.steps {
            if step.name.trim().is_empty() || step.script.trim().is_empty() {
                return Err(CellError::InvalidStep);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum GuestOperatingSystem {
    Linux,
    Macos,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum GuestNetwork {
    #[default]
    Shared,
    Softnet,
}

impl GuestOperatingSystem {
    const fn as_str(&self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Macos => "macos",
        }
    }

    const fn input_path(&self) -> &'static str {
        match self {
            Self::Linux => "/mnt/shared/nan-input",
            Self::Macos => "/Volumes/My Shared Files/nan-input",
        }
    }

    const fn output_path(&self) -> &'static str {
        match self {
            Self::Linux => "/mnt/shared/nan-output",
            Self::Macos => "/Volumes/My Shared Files/nan-output",
        }
    }

    const fn mount_script(&self) -> &'static str {
        match self {
            Self::Linux => {
                "set -euo pipefail\nsudo mkdir -p /mnt/shared\nmountpoint -q /mnt/shared || sudo mount -t virtiofs com.apple.virtio-fs.automount /mnt/shared\ntest -d /mnt/shared/nan-input\ntest -d /mnt/shared/nan-output\n"
            }
            Self::Macos => {
                "set -euo pipefail\ntest -d '/Volumes/My Shared Files/nan-input'\ntest -d '/Volumes/My Shared Files/nan-output'\n"
            }
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NanHarnessArtifact {
    version: String,
    source: String,
    artifact: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Artifact {
    source: PathBuf,
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Step {
    name: String,
    script: String,
    failure_class: FailureClass,
    #[serde(default)]
    requires_api_key: bool,
    #[serde(default = "default_step_timeout")]
    timeout_seconds: u64,
    #[serde(default = "default_attempts")]
    attempts: u8,
}

struct LoadedSpec {
    value: CellSpec,
    path: PathBuf,
    sha256: String,
}

impl LoadedSpec {
    fn load(path: &Path) -> Result<Self, CellError> {
        let contents = fs::read(path).map_err(|source| CellError::ReadSpec {
            path: path.to_owned(),
            source,
        })?;
        let value: CellSpec =
            toml::from_slice(&contents).map_err(|source| CellError::ParseSpec {
                path: path.to_owned(),
                source,
            })?;
        value.validate()?;
        Ok(Self {
            value,
            path: path.to_owned(),
            sha256: sha256_hex(&contents),
        })
    }

    fn resolve(&self, path: &Path) -> Result<PathBuf, CellError> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| CellError::InvalidSpecPath(self.path.clone()))?;
        Ok(parent.join(path))
    }
}

struct CellWorkspace {
    _root: TempDir,
    input: PathBuf,
    output: PathBuf,
    logs: PathBuf,
    nan_harness_sha256: String,
}

impl CellWorkspace {
    fn prepare(spec: &LoadedSpec) -> Result<Self, CellError> {
        let root = tempfile::Builder::new()
            .prefix("nan-harness-canary-cell-")
            .tempdir()
            .map_err(CellError::CreateWorkspace)?;
        let input = root.path().join("input");
        let output = root.path().join("output");
        let logs = root.path().join("private-logs");
        fs::create_dir_all(&input).map_err(CellError::CreateWorkspace)?;
        fs::create_dir_all(&output).map_err(CellError::CreateWorkspace)?;
        fs::create_dir_all(&logs).map_err(CellError::CreateWorkspace)?;

        let nan_harness_source = spec.resolve(&spec.value.nan_harness.artifact)?;
        let nan_harness_contents =
            fs::read(&nan_harness_source).map_err(|source| CellError::ReadArtifact {
                path: nan_harness_source.clone(),
                source,
            })?;
        let nan_harness_name = spec
            .value
            .nan_harness
            .artifact
            .file_name()
            .ok_or_else(|| CellError::InvalidArtifactName("nanHarness.artifact".to_owned()))?;
        fs::write(input.join(nan_harness_name), &nan_harness_contents).map_err(|source| {
            CellError::CopyArtifact {
                path: nan_harness_source,
                source,
            }
        })?;

        for artifact in &spec.value.artifacts {
            let source_path = spec.resolve(&artifact.source)?;
            fs::copy(&source_path, input.join(&artifact.name)).map_err(|source| {
                CellError::CopyArtifact {
                    path: source_path,
                    source,
                }
            })?;
        }

        Ok(Self {
            _root: root,
            input,
            output,
            logs,
            nan_harness_sha256: sha256_hex(&nan_harness_contents),
        })
    }

    fn log_path(&self, step: &str, attempt: u8) -> PathBuf {
        self.logs
            .join(format!("{}-{attempt}.log", safe_log_name(step)))
    }

    fn preserve_logs(&self, destination: &Path) -> Result<(), CellError> {
        fs::create_dir_all(destination).map_err(CellError::CreatePrivateLogDirectory)?;
        for entry in fs::read_dir(&self.logs).map_err(CellError::ReadPrivateLogs)? {
            let entry = entry.map_err(CellError::ReadPrivateLogs)?;
            if entry
                .file_type()
                .map_err(CellError::ReadPrivateLogs)?
                .is_file()
            {
                fs::copy(entry.path(), destination.join(entry.file_name()))
                    .map_err(CellError::PreservePrivateLog)?;
            }
        }
        Ok(())
    }

    fn harness_version(&self, spec: &CellSpec) -> Option<String> {
        let contents = fs::read_to_string(self.output.join(&spec.harness_version_file)).ok()?;
        contents.split_whitespace().find_map(|token| {
            let candidate = token.trim().trim_start_matches('v');
            semver::Version::parse(candidate)
                .ok()
                .map(|version| version.to_string())
        })
    }
}

struct VmLease {
    name: String,
    created: bool,
    run_process: Option<Child>,
    cleaned: bool,
}

impl VmLease {
    fn new(name: String) -> Self {
        Self {
            name,
            created: false,
            run_process: None,
            cleaned: false,
        }
    }

    async fn start(
        &mut self,
        workspace: &CellWorkspace,
        network: GuestNetwork,
        timeout: Duration,
    ) -> Result<(), String> {
        let input = format!("nan-input:{}:ro", workspace.input.display());
        let output = format!("nan-output:{}", workspace.output.display());
        let mut command = Command::new("tart");
        command.args(["run", "--no-graphics"]);
        if matches!(network, GuestNetwork::Softnet) {
            command.arg("--net-softnet");
        }
        let stdout = fs::File::create(workspace.log_path("start-vm", 1))
            .map_err(|error| format!("could not create the Tart private log: {error}"))?;
        let stderr = stdout
            .try_clone()
            .map_err(|error| format!("could not clone the Tart private log: {error}"))?;
        command
            .args([
                &format!("--dir={input}"),
                &format!("--dir={output}"),
                self.name.as_str(),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .kill_on_drop(true);
        let child = command
            .spawn()
            .map_err(|error| format!("could not start tart: {error}"))?;
        self.run_process = Some(child);
        tokio::time::sleep(timeout.min(Duration::from_millis(500))).await;
        if self
            .run_process
            .as_mut()
            .expect("run process should exist")
            .try_wait()
            .map_err(|error| format!("could not inspect tart: {error}"))?
            .is_some()
        {
            return Err("tart exited before the VM became reachable".to_owned());
        }
        Ok(())
    }

    async fn cleanup(&mut self) -> Result<(), String> {
        if self.cleaned {
            return Ok(());
        }
        let mut failures = Vec::new();
        if self.created
            && let Err(error) = run_host_command(
                "tart",
                &["stop", self.name.as_str()],
                Duration::from_secs(30),
            )
            .await
        {
            failures.push(error.detail);
        }
        if let Some(mut child) = self.run_process.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        if self.created
            && let Err(error) = run_host_command(
                "tart",
                &["delete", self.name.as_str()],
                Duration::from_secs(30),
            )
            .await
        {
            failures.push(error.detail);
        }
        if failures.is_empty() {
            self.cleaned = true;
            Ok(())
        } else {
            Err(failures.join("; "))
        }
    }
}

impl Drop for VmLease {
    fn drop(&mut self) {
        if self.cleaned || !self.created {
            return;
        }
        let _ = std::process::Command::new("tart")
            .args(["stop", self.name.as_str()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = std::process::Command::new("tart")
            .args(["delete", self.name.as_str()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

async fn wait_for_ssh(vm_name: &str, timeout: Duration) -> Result<String, String> {
    let deadline = Instant::now() + timeout;
    loop {
        let ip = command_text("tart", &["ip", vm_name], Duration::from_secs(10)).await;
        if let Ok(ip) = ip {
            let ip = ip.trim();
            if !ip.is_empty()
                && run_ssh_command(ip, "true", Duration::from_secs(10))
                    .await
                    .is_ok()
            {
                return Ok(ip.to_owned());
            }
        }
        if Instant::now() >= deadline {
            return Err("SSH readiness timed out".to_owned());
        }
        tokio::time::sleep(SSH_RETRY_DELAY).await;
    }
}

async fn run_remote_script(
    ip: &str,
    script: &str,
    log_path: &Path,
    timeout: Duration,
) -> Result<(), String> {
    let stdout = fs::File::create(log_path)
        .map_err(|error| format!("could not create the private step log: {error}"))?;
    let stderr = stdout
        .try_clone()
        .map_err(|error| format!("could not clone the private step log: {error}"))?;
    let mut command = ssh_command(ip);
    command
        .args(["bash", "-s"])
        .stdin(Stdio::piped())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|error| format!("could not start SSH: {error}"))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "SSH stdin is unavailable".to_owned())?;
    stdin
        .write_all(script.as_bytes())
        .await
        .map_err(|error| format!("could not send the remote script: {error}"))?;
    drop(stdin);
    let status = tokio::time::timeout(timeout, child.wait())
        .await
        .map_err(|_| "remote step timed out".to_owned())?
        .map_err(|error| format!("could not wait for SSH: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "remote step exited with {}",
            status
                .code()
                .map_or_else(|| "a signal".to_owned(), |code| format!("status {code}"))
        ))
    }
}

async fn run_ssh_command(ip: &str, remote: &str, timeout: Duration) -> Result<(), String> {
    let mut command = ssh_command(ip);
    command
        .arg(remote)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let status = tokio::time::timeout(timeout, command.status())
        .await
        .map_err(|_| "SSH command timed out".to_owned())?
        .map_err(|error| format!("could not start SSH: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("SSH command failed".to_owned())
    }
}

fn ssh_command(ip: &str) -> Command {
    let mut command = Command::new("sshpass");
    command.args([
        "-p",
        "admin",
        "ssh",
        "-o",
        "StrictHostKeyChecking=no",
        "-o",
        "UserKnownHostsFile=/dev/null",
        "-o",
        "LogLevel=ERROR",
        "-o",
        "IdentitiesOnly=yes",
        "-o",
        "PreferredAuthentications=password",
        "-o",
        "ConnectTimeout=10",
        &format!("admin@{ip}"),
    ]);
    command
}

async fn command_text(
    program: &str,
    arguments: &[&str],
    timeout: Duration,
) -> Result<String, String> {
    let output = tokio::time::timeout(
        timeout,
        Command::new(program)
            .args(arguments)
            .stdin(Stdio::null())
            .output(),
    )
    .await
    .map_err(|_| format!("{program} timed out"))?
    .map_err(|error| format!("could not start {program}: {error}"))?;
    if !output.status.success() {
        return Err(format!("{program} failed"));
    }
    String::from_utf8(output.stdout).map_err(|_| format!("{program} returned non-UTF-8 output"))
}

async fn run_host_command(
    program: &str,
    arguments: &[&str],
    timeout: Duration,
) -> Result<Duration, CommandFailure> {
    let started = Instant::now();
    let result = tokio::time::timeout(
        timeout,
        Command::new(program)
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status(),
    )
    .await;
    let duration = started.elapsed();
    match result {
        Ok(Ok(status)) if status.success() => Ok(duration),
        Ok(Ok(status)) => Err(CommandFailure {
            duration,
            detail: format!(
                "{program} exited with {}",
                status
                    .code()
                    .map_or_else(|| "a signal".to_owned(), |code| format!("status {code}"))
            ),
        }),
        Ok(Err(error)) => Err(CommandFailure {
            duration,
            detail: format!("could not start {program}: {error}"),
        }),
        Err(_) => Err(CommandFailure {
            duration,
            detail: format!("{program} timed out"),
        }),
    }
}

struct CommandFailure {
    duration: Duration,
    detail: String,
}

struct RuntimeFailure {
    class: FailureClass,
    phase: String,
    summary: String,
    checks: Vec<CheckReport>,
}

impl RuntimeFailure {
    fn new(
        class: FailureClass,
        phase: impl Into<String>,
        summary: impl Into<String>,
        checks: Vec<CheckReport>,
    ) -> Self {
        Self {
            class,
            phase: phase.into(),
            summary: summary.into(),
            checks,
        }
    }
}

fn step_script(spec: &CellSpec, step: &Step, api_key: Option<&str>) -> Zeroizing<String> {
    let input = spec.guest.input_path();
    let output = spec.guest.output_path();
    let mut script = Zeroizing::new(format!(
        "set -euo pipefail\nexport NAN_CANARY_INPUT={}\nexport NAN_CANARY_OUTPUT={}\nexport NAN_CANARY_HARNESS={}\n",
        shell_quote(input),
        shell_quote(output),
        shell_quote(&spec.harness.to_string())
    ));
    if let Some(model) = &spec.model {
        writeln!(script, "export NAN_CANARY_MODEL={}", shell_quote(model))
            .expect("writing to a String cannot fail");
    }
    if step.requires_api_key {
        let api_key = api_key.expect("the API key is loaded before live steps");
        writeln!(script, "export NAN_API_KEY={}", shell_quote(api_key))
            .expect("writing to a String cannot fail");
    }
    script.push_str(
        &step
            .script
            .replace("{{input}}", input)
            .replace("{{output}}", output)
            .replace("{{harness}}", &spec.harness.to_string())
            .replace("{{model}}", spec.model.as_deref().unwrap_or_default()),
    );
    script.push('\n');
    script
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn safe_log_name(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' {
                character
            } else {
                '-'
            }
        })
        .collect()
}

fn passed_check(name: &str, duration: Duration, attempts: u8) -> CheckReport {
    CheckReport {
        name: name.to_owned(),
        status: CheckStatus::Passed,
        duration_milliseconds: milliseconds(duration),
        attempts,
        detail: None,
    }
}

fn failed_check(name: &str, duration: Duration, attempts: u8, detail: &str) -> CheckReport {
    CheckReport {
        name: name.to_owned(),
        status: CheckStatus::Failed,
        duration_milliseconds: milliseconds(duration),
        attempts,
        detail: Some(detail.to_owned()),
    }
}

fn milliseconds(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn remaining(deadline: Instant, requested: Duration) -> Duration {
    deadline
        .saturating_duration_since(Instant::now())
        .min(requested)
        .max(Duration::from_millis(1))
}

fn timestamp() -> Result<String, CellError> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(CellError::Timestamp)
}

fn run_id(cell_id: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!("{cell_id}-{nanos}-{}", std::process::id())
}

fn vm_name(cell_id: &str) -> String {
    let sanitized = cell_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    format!("nan-harness-canary-{sanitized}-{}", std::process::id())
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

fn validate_relative_path(path: &Path, field: &'static str) -> Result<(), CellError> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(CellError::UnsafeRelativePath(field));
    }
    Ok(())
}

fn validate_file_name(name: &str) -> Result<(), CellError> {
    if name.is_empty() || Path::new(name).components().count() != 1 || matches!(name, "." | "..") {
        return Err(CellError::InvalidArtifactName(name.to_owned()));
    }
    Ok(())
}

const fn default_boot_timeout() -> u64 {
    DEFAULT_BOOT_TIMEOUT_SECONDS
}

const fn default_clone_timeout() -> u64 {
    DEFAULT_CLONE_TIMEOUT_SECONDS
}

const fn default_step_timeout() -> u64 {
    DEFAULT_STEP_TIMEOUT_SECONDS
}

const fn default_overall_timeout() -> u64 {
    DEFAULT_OVERALL_TIMEOUT_SECONDS
}

const fn default_attempts() -> u8 {
    1
}

#[derive(Debug, Error)]
pub(crate) enum CellError {
    #[error("could not read cell spec '{}': {source}", path.display())]
    ReadSpec {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not parse cell spec '{}': {source}", path.display())]
    ParseSpec {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("cell spec schema {0} is unsupported")]
    UnsupportedSpecSchema(u8),
    #[error("cell spec field {0} must not be empty")]
    EmptySpecField(&'static str),
    #[error("cell spec nan-harness version is invalid: {0}")]
    InvalidNanHarnessVersion(String),
    #[error("cell spec must contain at least one step")]
    MissingSteps,
    #[error("cell spec timeouts and attempts must be greater than zero")]
    InvalidTimeout,
    #[error("cell spec contains an empty step name or script")]
    InvalidStep,
    #[error("cell spec field {0} must contain a safe relative path")]
    UnsafeRelativePath(&'static str),
    #[error("cell spec artifact name '{0}' is invalid")]
    InvalidArtifactName(String),
    #[error("cell spec path '{}' has no parent directory", .0.display())]
    InvalidSpecPath(PathBuf),
    #[error("could not create the cell workspace: {0}")]
    CreateWorkspace(std::io::Error),
    #[error("could not create the private log directory: {0}")]
    CreatePrivateLogDirectory(std::io::Error),
    #[error("could not read private cell logs: {0}")]
    ReadPrivateLogs(std::io::Error),
    #[error("could not preserve a private cell log: {0}")]
    PreservePrivateLog(std::io::Error),
    #[error("could not read cell artifact '{}': {source}", path.display())]
    ReadArtifact {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not copy cell artifact '{}': {source}", path.display())]
    CopyArtifact {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not read NAN_API_KEY from the canary credential store: {0}")]
    ReadCredential(credentials::CredentialError),
    #[error("could not format a cell timestamp: {0}")]
    Timestamp(time::error::Format),
    #[error(transparent)]
    Report(#[from] crate::report::ReportError),
    #[error("could not serialize the cell report: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("the reproduction spec does not match the original cell report")]
    ReproductionMismatch,
    #[error("the canary failed; safe evidence was written to '{}'", .0.display())]
    CanaryFailed(PathBuf),
}

#[cfg(test)]
mod tests {
    use super::{CellSpec, GuestOperatingSystem, shell_quote, ssh_command, step_script};

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
}
