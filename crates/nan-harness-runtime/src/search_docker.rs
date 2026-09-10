//! Explicit lifecycle support for the managed image backend.
//!
//! This module is deliberately not part of launch preparation or search request handling.
//! Constructing plans and inspecting a plan never invokes Docker. Callers must invoke one of the
//! lifecycle methods below explicitly when they want to create, update, inspect, stop, or remove
//! the managed container.

use nan_harness_private_fs::{
    create_private_dir, create_private_dir_all, open_private_new, open_private_truncate,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;

pub const OFFICIAL_SEARXNG_IMAGE_REPOSITORY: &str = "docker.io/searxng/searxng";
pub const OFFICIAL_SEARXNG_IMAGE_TAG: &str = "latest";
pub const OFFICIAL_SEARXNG_IMAGE_DIGEST: &str =
    "sha256:2fb0fa85096fe6df5c3ab98ecb4d6e0ee2ef66b8fb96ce6fce0f75b51c4bd90a";
pub const OFFICIAL_SEARXNG_IMAGE: &str = "docker.io/searxng/searxng:latest@sha256:2fb0fa85096fe6df5c3ab98ecb4d6e0ee2ef66b8fb96ce6fce0f75b51c4bd90a";

pub const MANAGED_CONTAINER_NAME: &str = "nanh-search-searxng";
pub const UPDATE_BACKUP_CONTAINER_NAME: &str = "nanh-search-searxng.previous";
pub const UPDATE_CANDIDATE_CONTAINER_NAME: &str = "nanh-search-searxng.next";
pub const MANAGED_LABEL_KEY: &str = "io.nan-harness.managed";
pub const MANAGED_LABEL_VALUE: &str = "true";
pub const COMPONENT_LABEL_KEY: &str = "io.nan-harness.component";
pub const COMPONENT_LABEL_VALUE: &str = "search";
pub const OWNER_LABEL_KEY: &str = "io.nan-harness.owner";
pub const OWNER_LABEL_VALUE: &str = "nanh-search-docker-v1";
pub const SEARXNG_CONTAINER_PORT: u16 = 8080;
pub const DEFAULT_HOST_PORT: u16 = 8080;

const ROOT_MARKER_NAME: &str = ".nanh-owned";
const ROOT_MARKER: &[u8] = b"nanh-search-docker-root-v1\n";
const RECEIPT_NAME: &str = "container.json";
const RECOVERY_NAME: &str = "recovery.json";
const RECEIPT_SCHEMA_VERSION: u8 = 1;
const RECOVERY_SCHEMA_VERSION: u8 = 1;
const STORAGE_MOUNT: &str = "/var/cache/searxng";
const CONFIG_MOUNT: &str = "/etc/searxng";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockerSearchPaths {
    root: PathBuf,
}

impl DockerSearchPaths {
    #[must_use]
    pub fn under(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    #[must_use]
    pub fn for_user_home(home: &Path) -> Self {
        #[cfg(target_os = "macos")]
        let root = home
            .join("Library")
            .join("Application Support")
            .join("nan-harness")
            .join("searxng-docker");
        #[cfg(not(target_os = "macos"))]
        let root = home
            .join(".local")
            .join("share")
            .join("nan-harness")
            .join("searxng-docker");
        Self::under(root)
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn storage(&self) -> PathBuf {
        self.root.join("storage")
    }

    #[must_use]
    pub fn config(&self) -> PathBuf {
        self.root.join("config")
    }

    #[must_use]
    pub fn receipt(&self) -> PathBuf {
        self.root.join(RECEIPT_NAME)
    }

    #[must_use]
    pub fn recovery(&self) -> PathBuf {
        self.root.join(RECOVERY_NAME)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DockerSearchRequest {
    pub configured: bool,
    pub host_port: u16,
}

impl DockerSearchRequest {
    #[must_use]
    pub const fn unconfigured() -> Self {
        Self {
            configured: false,
            host_port: DEFAULT_HOST_PORT,
        }
    }

    #[must_use]
    pub const fn configured(host_port: u16) -> Self {
        Self {
            configured: true,
            host_port,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockerCommand {
    pub program: PathBuf,
    pub arguments: Vec<String>,
}

/// Backwards-compatible descriptive name for a managed Docker command.
pub type DockerSearchCommand = DockerCommand;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockerOperation {
    Create,
    Start,
    Inspect,
    Stop,
    Remove,
    Update,
}

impl DockerOperation {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Start => "start",
            Self::Inspect => "inspect",
            Self::Stop => "stop",
            Self::Remove => "remove",
            Self::Update => "update",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockerSearchPlan {
    operation: DockerOperation,
    paths: DockerSearchPaths,
    container_name: String,
    host_port: u16,
    image: String,
    commands: Vec<DockerCommand>,
}

impl DockerSearchPlan {
    #[must_use]
    pub const fn operation(&self) -> DockerOperation {
        self.operation
    }

    #[must_use]
    pub const fn paths(&self) -> &DockerSearchPaths {
        &self.paths
    }

    #[must_use]
    pub fn container_name(&self) -> &str {
        &self.container_name
    }

    #[must_use]
    pub const fn host_port(&self) -> u16 {
        self.host_port
    }

    #[must_use]
    pub fn image(&self) -> &str {
        &self.image
    }

    #[must_use]
    pub fn endpoint(&self) -> String {
        format!("http://127.0.0.1:{}/", self.host_port)
    }

    #[must_use]
    pub fn commands(&self) -> &[DockerCommand] {
        &self.commands
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DockerSetupPlan {
    NoOp,
    Setup(DockerSearchPlan),
}

/// Descriptive alias for callers that model setup as installation.
pub type DockerSearchSetupPlan = DockerSetupPlan;
/// Descriptive alias for the pure lifecycle plan.
pub type DockerSearchInstallPlan = DockerSearchPlan;

impl DockerSetupPlan {
    #[must_use]
    pub fn as_plan(&self) -> Option<&DockerSearchPlan> {
        match self {
            Self::NoOp => None,
            Self::Setup(plan) => Some(plan),
        }
    }
}

/// Builds a pure setup plan.
///
/// # Errors
///
/// Returns an error for an invalid host port or image contract.
pub fn plan_docker_search_setup(
    request: DockerSearchRequest,
    paths: DockerSearchPaths,
) -> Result<DockerSetupPlan, DockerSearchError> {
    if !request.configured {
        return Ok(DockerSetupPlan::NoOp);
    }
    validate_request(request)?;
    validate_pinned_image_contract()?;
    Ok(DockerSetupPlan::Setup(build_setup_plan(request, paths)))
}

/// Builds a pure create plan.
///
/// # Errors
///
/// Returns an error for an invalid host port or image contract.
pub fn plan_docker_search_create(
    request: DockerSearchRequest,
    paths: DockerSearchPaths,
) -> Result<DockerSearchPlan, DockerSearchError> {
    validate_request(request)?;
    validate_pinned_image_contract()?;
    Ok(build_create_plan(request, paths, MANAGED_CONTAINER_NAME))
}

/// Builds a pure start plan.
///
/// # Errors
///
/// Returns an error for an invalid host port.
pub fn plan_docker_search_start(
    request: DockerSearchRequest,
    paths: DockerSearchPaths,
) -> Result<DockerSearchPlan, DockerSearchError> {
    validate_request(request)?;
    Ok(single_command_plan(
        DockerOperation::Start,
        request,
        paths,
        MANAGED_CONTAINER_NAME,
        container_command(["container", "start", MANAGED_CONTAINER_NAME]),
    ))
}

/// Builds a pure inspect plan.
///
/// # Errors
///
/// Returns an error for an invalid host port.
pub fn plan_docker_search_inspect(
    request: DockerSearchRequest,
    paths: DockerSearchPaths,
) -> Result<DockerSearchPlan, DockerSearchError> {
    validate_request(request)?;
    Ok(single_command_plan(
        DockerOperation::Inspect,
        request,
        paths,
        MANAGED_CONTAINER_NAME,
        inspect_command(MANAGED_CONTAINER_NAME),
    ))
}

/// Builds a pure stop plan.
///
/// # Errors
///
/// Returns an error for an invalid host port.
pub fn plan_docker_search_stop(
    request: DockerSearchRequest,
    paths: DockerSearchPaths,
) -> Result<DockerSearchPlan, DockerSearchError> {
    validate_request(request)?;
    Ok(single_command_plan(
        DockerOperation::Stop,
        request,
        paths,
        MANAGED_CONTAINER_NAME,
        container_command(["container", "stop", "--time", "10", MANAGED_CONTAINER_NAME]),
    ))
}

/// Builds a pure remove plan.
///
/// # Errors
///
/// Returns an error for an invalid host port.
pub fn plan_docker_search_remove(
    request: DockerSearchRequest,
    paths: DockerSearchPaths,
) -> Result<DockerSearchPlan, DockerSearchError> {
    validate_request(request)?;
    Ok(single_command_plan(
        DockerOperation::Remove,
        request,
        paths,
        MANAGED_CONTAINER_NAME,
        container_command(["container", "rm", MANAGED_CONTAINER_NAME]),
    ))
}

/// Builds a pure update plan.
///
/// # Errors
///
/// Returns an error for an invalid host port or image contract.
pub fn plan_docker_search_update(
    request: DockerSearchRequest,
    paths: DockerSearchPaths,
) -> Result<DockerSearchPlan, DockerSearchError> {
    validate_request(request)?;
    validate_pinned_image_contract()?;
    let commands = vec![
        inspect_command(MANAGED_CONTAINER_NAME),
        inspect_command(UPDATE_BACKUP_CONTAINER_NAME),
        inspect_command(UPDATE_CANDIDATE_CONTAINER_NAME),
        container_command(["image", "pull", OFFICIAL_SEARXNG_IMAGE]),
        image_inspect_command(),
        container_command(["container", "stop", "--time", "10", MANAGED_CONTAINER_NAME]),
        container_command([
            "container",
            "rename",
            MANAGED_CONTAINER_NAME,
            UPDATE_BACKUP_CONTAINER_NAME,
        ]),
        create_command(request.host_port, &paths, UPDATE_CANDIDATE_CONTAINER_NAME),
        container_command(["container", "start", UPDATE_CANDIDATE_CONTAINER_NAME]),
        inspect_command(UPDATE_CANDIDATE_CONTAINER_NAME),
        container_command([
            "container",
            "rename",
            UPDATE_CANDIDATE_CONTAINER_NAME,
            MANAGED_CONTAINER_NAME,
        ]),
        container_command(["container", "rm", UPDATE_BACKUP_CONTAINER_NAME]),
    ];
    Ok(DockerSearchPlan {
        operation: DockerOperation::Update,
        paths,
        container_name: MANAGED_CONTAINER_NAME.to_owned(),
        host_port: request.host_port,
        image: OFFICIAL_SEARXNG_IMAGE.to_owned(),
        commands,
    })
}

/// Executes an explicit setup plan through the supplied executor.
///
/// # Errors
///
/// Returns an error for active sessions, Docker failures, invalid image
/// integrity, or ownership conflicts. Unconfigured plans are a no-op.
pub fn execute_docker_search_setup<E: DockerExecutor>(
    plan: &DockerSetupPlan,
    active_sessions: usize,
    executor: E,
) -> Result<DockerSearchOutcome, DockerSearchError> {
    let DockerSetupPlan::Setup(plan) = plan else {
        return Ok(DockerSearchOutcome::NoOp);
    };
    let manager = DockerSearchManager::new(
        executor,
        plan.paths.clone(),
        DockerSearchRequest::configured(plan.host_port),
    )?;
    manager.setup(active_sessions)
}

/// Executes an explicit update plan through the supplied executor.
///
/// # Errors
///
/// Returns an error for active sessions, missing or foreign state, Docker
/// failures, invalid image integrity, or a failed rollback.
pub fn execute_docker_search_update<E: DockerExecutor>(
    plan: &DockerSearchPlan,
    active_sessions: usize,
    executor: E,
) -> Result<DockerSearchOutcome, DockerSearchError> {
    if plan.operation != DockerOperation::Update {
        return Err(DockerSearchError::InvalidOperationPlan);
    }
    let manager = DockerSearchManager::new(
        executor,
        plan.paths.clone(),
        DockerSearchRequest::configured(plan.host_port),
    )?;
    manager.update(active_sessions)
}

fn build_setup_plan(request: DockerSearchRequest, paths: DockerSearchPaths) -> DockerSearchPlan {
    let commands = vec![
        container_command(["image", "pull", OFFICIAL_SEARXNG_IMAGE]),
        image_inspect_command(),
        inspect_command(MANAGED_CONTAINER_NAME),
        create_command(request.host_port, &paths, MANAGED_CONTAINER_NAME),
        container_command(["container", "start", MANAGED_CONTAINER_NAME]),
    ];
    DockerSearchPlan {
        operation: DockerOperation::Create,
        paths,
        container_name: MANAGED_CONTAINER_NAME.to_owned(),
        host_port: request.host_port,
        image: OFFICIAL_SEARXNG_IMAGE.to_owned(),
        commands,
    }
}

fn build_create_plan(
    request: DockerSearchRequest,
    paths: DockerSearchPaths,
    name: &str,
) -> DockerSearchPlan {
    let command = create_command(request.host_port, &paths, name);
    DockerSearchPlan {
        operation: DockerOperation::Create,
        paths,
        container_name: name.to_owned(),
        host_port: request.host_port,
        image: OFFICIAL_SEARXNG_IMAGE.to_owned(),
        commands: vec![command],
    }
}

fn single_command_plan(
    operation: DockerOperation,
    request: DockerSearchRequest,
    paths: DockerSearchPaths,
    name: &str,
    command: DockerCommand,
) -> DockerSearchPlan {
    DockerSearchPlan {
        operation,
        paths,
        container_name: name.to_owned(),
        host_port: request.host_port,
        image: OFFICIAL_SEARXNG_IMAGE.to_owned(),
        commands: vec![command],
    }
}

fn validate_request(request: DockerSearchRequest) -> Result<(), DockerSearchError> {
    if request.host_port == 0 {
        return Err(DockerSearchError::InvalidHostPort);
    }
    Ok(())
}

/// Validates the immutable official image contract without contacting Docker.
///
/// # Errors
///
/// Returns an error if the compiled image reference or digest is malformed.
pub fn validate_pinned_image_contract() -> Result<(), DockerSearchError> {
    if !OFFICIAL_SEARXNG_IMAGE_DIGEST.starts_with("sha256:")
        || OFFICIAL_SEARXNG_IMAGE_DIGEST.len() != 71
        || !OFFICIAL_SEARXNG_IMAGE_DIGEST[7..]
            .chars()
            .all(|character| character.is_ascii_hexdigit())
        || !OFFICIAL_SEARXNG_IMAGE.starts_with(OFFICIAL_SEARXNG_IMAGE_REPOSITORY)
        || !OFFICIAL_SEARXNG_IMAGE.ends_with(OFFICIAL_SEARXNG_IMAGE_DIGEST)
    {
        return Err(DockerSearchError::InvalidImageContract);
    }
    Ok(())
}

fn docker_command(arguments: impl IntoIterator<Item = String>) -> DockerCommand {
    DockerCommand {
        program: PathBuf::from("docker"),
        arguments: arguments.into_iter().collect(),
    }
}

fn container_command<const N: usize>(arguments: [&str; N]) -> DockerCommand {
    docker_command(arguments.into_iter().map(str::to_owned))
}

fn inspect_command(name: &str) -> DockerCommand {
    container_command(["container", "inspect", "--format", "{{json .}}", name])
}

fn image_inspect_command() -> DockerCommand {
    container_command([
        "image",
        "inspect",
        "--format",
        "{{json .RepoDigests}}",
        OFFICIAL_SEARXNG_IMAGE,
    ])
}

fn create_command(host_port: u16, paths: &DockerSearchPaths, name: &str) -> DockerCommand {
    docker_command([
        "container".to_owned(),
        "create".to_owned(),
        "--name".to_owned(),
        name.to_owned(),
        "--label".to_owned(),
        format!("{MANAGED_LABEL_KEY}={MANAGED_LABEL_VALUE}"),
        "--label".to_owned(),
        format!("{COMPONENT_LABEL_KEY}={COMPONENT_LABEL_VALUE}"),
        "--label".to_owned(),
        format!("{OWNER_LABEL_KEY}={OWNER_LABEL_VALUE}"),
        "--publish".to_owned(),
        format!("127.0.0.1:{host_port}:{SEARXNG_CONTAINER_PORT}"),
        "--mount".to_owned(),
        format!(
            "type=bind,source={},destination={STORAGE_MOUNT}",
            paths.storage().display()
        ),
        "--mount".to_owned(),
        format!(
            "type=bind,source={},destination={CONFIG_MOUNT}",
            paths.config().display()
        ),
        OFFICIAL_SEARXNG_IMAGE.to_owned(),
    ])
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockerCommandOutput {
    pub status: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl DockerCommandOutput {
    #[must_use]
    pub const fn success() -> Self {
        Self {
            status: Some(0),
            stdout: Vec::new(),
            stderr: Vec::new(),
        }
    }

    fn is_success(&self) -> bool {
        self.status == Some(0)
    }
}

pub trait DockerExecutor {
    /// Executes one already planned Docker command.
    ///
    /// # Errors
    ///
    /// Returns an executor or command-start error.
    fn execute(&self, command: &DockerCommand) -> Result<DockerCommandOutput, DockerSearchError>;
}

/// Descriptive alias for the executor contract.
pub use DockerExecutor as DockerSearchCommandExecutor;

#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessDockerExecutor;

/// Descriptive alias for the production executor.
pub type ProcessDockerSearchCommandExecutor = ProcessDockerExecutor;

impl DockerExecutor for ProcessDockerExecutor {
    fn execute(&self, command: &DockerCommand) -> Result<DockerCommandOutput, DockerSearchError> {
        let output = Command::new(&command.program)
            .args(&command.arguments)
            .output()
            .map_err(|source| DockerSearchError::CommandStart {
                program: command.program.display().to_string(),
                source,
            })?;
        Ok(DockerCommandOutput {
            status: output.status.code(),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockerContainerStatus {
    pub id: String,
    pub name: String,
    pub image: String,
    pub labels: BTreeMap<String, String>,
    pub running: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DockerSearchStatus {
    Absent,
    Stopped(DockerContainerStatus),
    Running(DockerContainerStatus),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DockerSearchOutcome {
    NoOp,
    Created { endpoint: String },
    AlreadyPresent(DockerSearchStatus),
    Started,
    Updated { endpoint: String },
    Stopped,
    Removed,
}

#[derive(Debug)]
pub struct DockerSearchManager<E> {
    executor: E,
    paths: DockerSearchPaths,
    request: DockerSearchRequest,
}

impl<E: DockerExecutor> DockerSearchManager<E> {
    /// Creates a manager without inspecting paths or invoking Docker.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid host port or image contract.
    pub fn new(
        executor: E,
        paths: DockerSearchPaths,
        request: DockerSearchRequest,
    ) -> Result<Self, DockerSearchError> {
        validate_request(request)?;
        validate_pinned_image_contract()?;
        Ok(Self {
            executor,
            paths,
            request,
        })
    }

    /// Returns a pure setup plan.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid host port or image contract.
    pub fn setup_plan(&self) -> Result<DockerSetupPlan, DockerSearchError> {
        plan_docker_search_setup(self.request, self.paths.clone())
    }

    /// Returns a pure plan for a lifecycle operation.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid host port or image contract.
    pub fn plan(&self, operation: DockerOperation) -> Result<DockerSearchPlan, DockerSearchError> {
        match operation {
            DockerOperation::Create => plan_docker_search_create(self.request, self.paths.clone()),
            DockerOperation::Start => plan_docker_search_start(self.request, self.paths.clone()),
            DockerOperation::Inspect => {
                plan_docker_search_inspect(self.request, self.paths.clone())
            }
            DockerOperation::Stop => plan_docker_search_stop(self.request, self.paths.clone()),
            DockerOperation::Remove => plan_docker_search_remove(self.request, self.paths.clone()),
            DockerOperation::Update => plan_docker_search_update(self.request, self.paths.clone()),
        }
    }

    /// Inspects only the exact managed container name and never starts it.
    ///
    /// # Errors
    ///
    /// Returns an error for Docker failures, malformed inspection output, or foreign ownership.
    pub fn status(&self) -> Result<DockerSearchStatus, DockerSearchError> {
        self.status_for(MANAGED_CONTAINER_NAME)
    }

    /// Alias for status that makes the no-start side effect explicit.
    ///
    /// # Errors
    ///
    /// Returns the same errors as the status method.
    pub fn status_without_starting(&self) -> Result<DockerSearchStatus, DockerSearchError> {
        self.status()
    }

    /// Explicitly starts a stopped owned container.
    ///
    /// # Errors
    ///
    /// Returns an error for active sessions, a missing or foreign container, or Docker failures.
    pub fn start(&self, active_sessions: usize) -> Result<DockerSearchOutcome, DockerSearchError> {
        ensure_no_active_sessions("start", active_sessions)?;
        let status = match self.status()? {
            DockerSearchStatus::Absent => return Err(DockerSearchError::ContainerNotFound),
            DockerSearchStatus::Running(status) => {
                return Ok(DockerSearchOutcome::AlreadyPresent(
                    DockerSearchStatus::Running(status),
                ));
            }
            DockerSearchStatus::Stopped(status) => status,
        };
        ensure_owned_container(&status, MANAGED_CONTAINER_NAME)?;
        self.run_required(
            &container_command(["container", "start", MANAGED_CONTAINER_NAME]),
            DockerOperation::Start,
        )?;
        Ok(DockerSearchOutcome::Started)
    }

    /// Explicitly creates and starts the managed container, preserving existing owned state.
    ///
    /// # Errors
    ///
    /// Returns an error for active sessions, Docker failures, invalid image integrity, or foreign
    /// filesystem/container ownership. A failed setup leaves a recovery record.
    pub fn setup(&self, active_sessions: usize) -> Result<DockerSearchOutcome, DockerSearchError> {
        ensure_no_active_sessions("setup", active_sessions)?;
        if !self.request.configured {
            return Ok(DockerSearchOutcome::NoOp);
        }
        let existing = self.status()?;
        if !matches!(existing, DockerSearchStatus::Absent) {
            return Ok(DockerSearchOutcome::AlreadyPresent(existing));
        }
        prepare_paths(&self.paths)?;
        let result = self.setup_new();
        match result {
            Ok(outcome) => {
                clear_owned_file(&self.paths.recovery())?;
                Ok(outcome)
            }
            Err(error) => {
                record_recovery(
                    &self.paths,
                    DockerOperation::Create,
                    "setup failed; inspect and remove the owned container if present",
                    false,
                )?;
                Err(error)
            }
        }
    }

    fn setup_new(&self) -> Result<DockerSearchOutcome, DockerSearchError> {
        self.run_required(
            &container_command(["image", "pull", OFFICIAL_SEARXNG_IMAGE]),
            DockerOperation::Create,
        )?;
        let image = self.run_required(&image_inspect_command(), DockerOperation::Inspect)?;
        validate_image_inspect_digests(&image.stdout)?;
        let create = create_command(self.request.host_port, &self.paths, MANAGED_CONTAINER_NAME);
        self.run_required(&create, DockerOperation::Create)?;
        self.run_required(
            &container_command(["container", "start", MANAGED_CONTAINER_NAME]),
            DockerOperation::Start,
        )?;
        write_receipt(&self.paths, self.request.host_port)?;
        Ok(DockerSearchOutcome::Created {
            endpoint: format!("http://127.0.0.1:{}/", self.request.host_port),
        })
    }

    /// Explicitly replaces the managed container and rolls back to the prior one on failure.
    ///
    /// # Errors
    ///
    /// Returns an error for active sessions, missing or foreign state, Docker failures, invalid
    /// image integrity, or a failed rollback. A failed update leaves a recovery record.
    pub fn update(&self, active_sessions: usize) -> Result<DockerSearchOutcome, DockerSearchError> {
        ensure_no_active_sessions("update", active_sessions)?;
        if !self.request.configured {
            return Ok(DockerSearchOutcome::NoOp);
        }
        prepare_paths(&self.paths)?;
        let prior = match self.status()? {
            DockerSearchStatus::Absent => return Err(DockerSearchError::ContainerNotFound),
            DockerSearchStatus::Stopped(status) | DockerSearchStatus::Running(status) => status,
        };
        let prior_running = prior.running;
        let backup = self.inspect_named(UPDATE_BACKUP_CONTAINER_NAME)?;
        if let Some(status) = backup {
            ensure_owned_container(&status, UPDATE_BACKUP_CONTAINER_NAME)?;
            self.remove_named(UPDATE_BACKUP_CONTAINER_NAME, status.running)?;
        }
        let candidate = self.inspect_named(UPDATE_CANDIDATE_CONTAINER_NAME)?;
        if let Some(status) = candidate {
            ensure_owned_container(&status, UPDATE_CANDIDATE_CONTAINER_NAME)?;
            self.remove_named(UPDATE_CANDIDATE_CONTAINER_NAME, status.running)?;
        }

        let mut renamed_prior = false;
        let mut stopped_prior = false;
        let result = self.update_inner(prior_running, &mut renamed_prior, &mut stopped_prior);
        match result {
            Ok(outcome) => {
                clear_owned_file(&self.paths.recovery())?;
                Ok(outcome)
            }
            Err(error) => {
                let rollback = if renamed_prior {
                    self.rollback_update(prior_running)
                } else if stopped_prior {
                    self.run_required(
                        &container_command(["container", "start", MANAGED_CONTAINER_NAME]),
                        DockerOperation::Start,
                    )
                    .map(|_| ())
                } else {
                    Ok(())
                };
                record_recovery(
                    &self.paths,
                    DockerOperation::Update,
                    if rollback.is_ok() {
                        "update failed and the prior owned container was restored"
                    } else {
                        "update and automatic rollback failed; inspect owned transaction names"
                    },
                    prior_running,
                )?;
                match rollback {
                    Ok(()) => Err(error),
                    Err(rollback_error) => Err(DockerSearchError::RollbackFailed {
                        operation: Box::new(error),
                        rollback: Box::new(rollback_error),
                    }),
                }
            }
        }
    }

    fn update_inner(
        &self,
        prior_running: bool,
        renamed_prior: &mut bool,
        stopped_prior: &mut bool,
    ) -> Result<DockerSearchOutcome, DockerSearchError> {
        self.run_required(
            &container_command(["image", "pull", OFFICIAL_SEARXNG_IMAGE]),
            DockerOperation::Update,
        )?;
        let image = self.run_required(&image_inspect_command(), DockerOperation::Inspect)?;
        validate_image_inspect_digests(&image.stdout)?;
        if prior_running {
            self.run_required(
                &container_command(["container", "stop", "--time", "10", MANAGED_CONTAINER_NAME]),
                DockerOperation::Stop,
            )?;
            *stopped_prior = true;
        }
        self.run_required(
            &container_command([
                "container",
                "rename",
                MANAGED_CONTAINER_NAME,
                UPDATE_BACKUP_CONTAINER_NAME,
            ]),
            DockerOperation::Update,
        )?;
        *renamed_prior = true;
        self.run_required(
            &create_command(
                self.request.host_port,
                &self.paths,
                UPDATE_CANDIDATE_CONTAINER_NAME,
            ),
            DockerOperation::Create,
        )?;
        self.run_required(
            &container_command(["container", "start", UPDATE_CANDIDATE_CONTAINER_NAME]),
            DockerOperation::Start,
        )?;
        let candidate = self
            .inspect_named(UPDATE_CANDIDATE_CONTAINER_NAME)?
            .ok_or(DockerSearchError::ContainerNotFound)?;
        ensure_owned_container(&candidate, UPDATE_CANDIDATE_CONTAINER_NAME)?;
        if !candidate.running {
            return Err(DockerSearchError::ContainerNotRunning);
        }
        // Persist the new desired state while the old container still has its original name.
        // A receipt failure can therefore still roll back without colliding with a new stable
        // container name.
        write_receipt(&self.paths, self.request.host_port)?;
        self.run_required(
            &container_command([
                "container",
                "rename",
                UPDATE_CANDIDATE_CONTAINER_NAME,
                MANAGED_CONTAINER_NAME,
            ]),
            DockerOperation::Update,
        )?;
        self.run_required(
            &container_command(["container", "rm", UPDATE_BACKUP_CONTAINER_NAME]),
            DockerOperation::Remove,
        )?;
        Ok(DockerSearchOutcome::Updated {
            endpoint: format!("http://127.0.0.1:{}/", self.request.host_port),
        })
    }

    fn rollback_update(&self, prior_running: bool) -> Result<(), DockerSearchError> {
        let backup = self.inspect_named(UPDATE_BACKUP_CONTAINER_NAME)?;
        if let Some(candidate) = self.inspect_named(UPDATE_CANDIDATE_CONTAINER_NAME)? {
            ensure_owned_container(&candidate, UPDATE_CANDIDATE_CONTAINER_NAME)?;
            self.remove_named(UPDATE_CANDIDATE_CONTAINER_NAME, candidate.running)?;
        }
        let backup = backup.ok_or(DockerSearchError::ContainerNotFound)?;
        ensure_owned_container(&backup, UPDATE_BACKUP_CONTAINER_NAME)?;
        self.run_required(
            &container_command([
                "container",
                "rename",
                UPDATE_BACKUP_CONTAINER_NAME,
                MANAGED_CONTAINER_NAME,
            ]),
            DockerOperation::Update,
        )?;
        if prior_running {
            self.run_required(
                &container_command(["container", "start", MANAGED_CONTAINER_NAME]),
                DockerOperation::Start,
            )?;
        }
        Ok(())
    }

    /// Explicitly stops the managed container.
    ///
    /// # Errors
    ///
    /// Returns an error for active sessions, Docker failures, or foreign ownership.
    pub fn stop(&self, active_sessions: usize) -> Result<DockerSearchOutcome, DockerSearchError> {
        ensure_no_active_sessions("stop", active_sessions)?;
        let status = match self.status()? {
            DockerSearchStatus::Absent | DockerSearchStatus::Stopped(_) => {
                return Ok(DockerSearchOutcome::Stopped);
            }
            DockerSearchStatus::Running(status) => status,
        };
        ensure_owned_container(&status, MANAGED_CONTAINER_NAME)?;
        self.run_required(
            &container_command(["container", "stop", "--time", "10", MANAGED_CONTAINER_NAME]),
            DockerOperation::Stop,
        )?;
        Ok(DockerSearchOutcome::Stopped)
    }

    /// Explicitly removes the managed container while retaining private data for recovery.
    ///
    /// # Errors
    ///
    /// Returns an error for active sessions, Docker failures, or foreign ownership.
    pub fn remove(&self, active_sessions: usize) -> Result<DockerSearchOutcome, DockerSearchError> {
        ensure_no_active_sessions("remove", active_sessions)?;
        let stable = self.inspect_named(MANAGED_CONTAINER_NAME)?;
        if let Some(status) = &stable {
            ensure_owned_container(status, MANAGED_CONTAINER_NAME)?;
        }
        // An interrupted update can leave one of the transaction names behind. Inspect and
        // validate both names before removing anything so a foreign collision cannot turn an
        // otherwise safe uninstall into a partial destructive operation.
        let transactions = self.transaction_containers()?;
        if let Some(status) = stable {
            self.remove_named(MANAGED_CONTAINER_NAME, status.running)?;
        }
        for (name, running) in transactions {
            self.remove_named(&name, running)?;
        }
        clear_owned_file(&self.paths.receipt())?;
        clear_owned_file(&self.paths.recovery())?;
        Ok(DockerSearchOutcome::Removed)
    }

    fn transaction_containers(&self) -> Result<Vec<(String, bool)>, DockerSearchError> {
        let mut containers = Vec::new();
        for name in [
            UPDATE_BACKUP_CONTAINER_NAME,
            UPDATE_CANDIDATE_CONTAINER_NAME,
        ] {
            if let Some(status) = self.inspect_named(name)? {
                ensure_owned_container(&status, name)?;
                containers.push((name.to_owned(), status.running));
            }
        }
        Ok(containers)
    }

    fn status_for(&self, name: &str) -> Result<DockerSearchStatus, DockerSearchError> {
        let Some(status) = self.inspect_named(name)? else {
            return Ok(DockerSearchStatus::Absent);
        };
        ensure_owned_container(&status, name)?;
        if status.running {
            Ok(DockerSearchStatus::Running(status))
        } else {
            Ok(DockerSearchStatus::Stopped(status))
        }
    }

    fn inspect_named(
        &self,
        name: &str,
    ) -> Result<Option<DockerContainerStatus>, DockerSearchError> {
        let command = inspect_command(name);
        let output = self.executor.execute(&command)?;
        if !output.is_success() {
            if is_missing_container(&output.stderr) {
                return Ok(None);
            }
            return Err(command_failed(DockerOperation::Inspect, &output));
        }
        Ok(Some(parse_container_status(&output.stdout, name)?))
    }

    fn remove_named(&self, name: &str, running: bool) -> Result<(), DockerSearchError> {
        if running {
            self.run_required(
                &container_command(["container", "stop", "--time", "10", name]),
                DockerOperation::Stop,
            )?;
        }
        self.run_required(
            &container_command(["container", "rm", name]),
            DockerOperation::Remove,
        )?;
        Ok(())
    }

    fn run_required(
        &self,
        command: &DockerCommand,
        operation: DockerOperation,
    ) -> Result<DockerCommandOutput, DockerSearchError> {
        let output = self.executor.execute(command)?;
        if output.is_success() {
            Ok(output)
        } else {
            Err(command_failed(operation, &output))
        }
    }
}

/// Removes only the private data directories owned by the managed Docker backend.
///
/// Container removal deliberately remains a separate operation so callers can validate active
/// sessions and exact container ownership first. This helper never invokes Docker and never
/// removes the official image; unknown entries below the owned root are retained.
///
/// # Errors
///
/// Returns an ownership or filesystem error when the root or its owned state cannot be validated
/// or removed safely.
pub fn cleanup_owned_docker_search_data(
    paths: &DockerSearchPaths,
) -> Result<(), DockerSearchError> {
    let metadata = match fs::symlink_metadata(paths.root()) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(io_error(
                "inspect Docker data root",
                paths.root().to_path_buf(),
                source,
            ));
        }
    };
    if !metadata.is_dir() {
        return Err(DockerSearchError::NotDirectory(paths.root().to_path_buf()));
    }
    ensure_existing_owned_root(paths.root())?;
    for path in [paths.receipt(), paths.recovery()] {
        validate_owned_file(&path)?;
    }

    for path in [paths.storage(), paths.config()] {
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(&path)
                .map_err(|source| io_error("remove owned Docker data", path, source))?,
            Ok(_) => {
                return Err(DockerSearchError::OwnershipConflict {
                    resource: path.display().to_string(),
                });
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(io_error("inspect Docker data", path, source));
            }
        }
    }
    for path in [paths.receipt(), paths.recovery()] {
        clear_owned_file(&path)?;
    }
    fs::remove_file(paths.root().join(ROOT_MARKER_NAME)).map_err(|source| {
        io_error(
            "remove Docker ownership marker",
            paths.root().join(ROOT_MARKER_NAME),
            source,
        )
    })?;
    if fs::read_dir(paths.root())
        .map_err(|source| {
            io_error(
                "inspect Docker data root",
                paths.root().to_path_buf(),
                source,
            )
        })?
        .next()
        .is_none()
    {
        fs::remove_dir(paths.root()).map_err(|source| {
            io_error(
                "remove empty Docker data root",
                paths.root().to_path_buf(),
                source,
            )
        })?;
    }
    Ok(())
}

fn ensure_existing_owned_root(path: &Path) -> Result<(), DockerSearchError> {
    let marker = path.join(ROOT_MARKER_NAME);
    let marker_metadata =
        fs::symlink_metadata(&marker).map_err(|_| DockerSearchError::OwnershipConflict {
            resource: path.display().to_string(),
        })?;
    if !marker_metadata.is_file() {
        return Err(DockerSearchError::OwnershipConflict {
            resource: path.display().to_string(),
        });
    }
    let contents = fs::read(&marker).map_err(|_| DockerSearchError::OwnershipConflict {
        resource: path.display().to_string(),
    })?;
    if contents != ROOT_MARKER {
        return Err(DockerSearchError::OwnershipConflict {
            resource: path.display().to_string(),
        });
    }
    Ok(())
}

fn validate_owned_file(path: &Path) -> Result<(), DockerSearchError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(source) => return Err(io_error("inspect Docker state", path.to_path_buf(), source)),
    };
    if !metadata.is_file() {
        return Err(DockerSearchError::OwnershipConflict {
            resource: path.display().to_string(),
        });
    }
    let contents = fs::read(path)
        .map_err(|source| io_error("read Docker state", path.to_path_buf(), source))?;
    let owned = serde_json::from_slice::<serde_json::Value>(&contents)
        .ok()
        .and_then(|value| {
            value
                .get("owner")
                .and_then(serde_json::Value::as_str)
                .map(|owner| owner == OWNER_LABEL_VALUE)
        })
        .unwrap_or(false);
    if owned {
        Ok(())
    } else {
        Err(DockerSearchError::OwnershipConflict {
            resource: path.display().to_string(),
        })
    }
}

fn command_failed(operation: DockerOperation, output: &DockerCommandOutput) -> DockerSearchError {
    DockerSearchError::CommandFailed {
        operation,
        status: output.status,
    }
}

fn ensure_no_active_sessions(
    operation: &'static str,
    count: usize,
) -> Result<(), DockerSearchError> {
    if count != 0 {
        return Err(DockerSearchError::ActiveSessions { operation, count });
    }
    Ok(())
}

fn is_missing_container(stderr: &[u8]) -> bool {
    let text = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    text.contains("no such container")
        || text.contains("no such object")
        || text.contains("not found")
}

fn parse_container_status(
    stdout: &[u8],
    expected_name: &str,
) -> Result<DockerContainerStatus, DockerSearchError> {
    let value: serde_json::Value =
        serde_json::from_slice(stdout).map_err(|_| DockerSearchError::InvalidInspection)?;
    let object = if let Some(array) = value.as_array() {
        array.first().ok_or(DockerSearchError::InvalidInspection)?
    } else {
        &value
    };
    let id = object
        .get("Id")
        .and_then(serde_json::Value::as_str)
        .filter(|id| !id.is_empty())
        .ok_or(DockerSearchError::InvalidInspection)?
        .to_owned();
    let name = object
        .get("Name")
        .and_then(serde_json::Value::as_str)
        .map(|name| name.trim_start_matches('/').to_owned())
        .ok_or(DockerSearchError::InvalidInspection)?;
    if name != expected_name {
        return Err(DockerSearchError::OwnershipConflict {
            resource: expected_name.to_owned(),
        });
    }
    let config = object
        .get("Config")
        .ok_or(DockerSearchError::InvalidInspection)?;
    let image = config
        .get("Image")
        .and_then(serde_json::Value::as_str)
        .ok_or(DockerSearchError::InvalidInspection)?
        .to_owned();
    let labels = config
        .get("Labels")
        .and_then(serde_json::Value::as_object)
        .map(|labels| {
            labels
                .iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|value| (key.clone(), value.to_owned()))
                })
                .collect()
        })
        .unwrap_or_default();
    let running = object
        .get("State")
        .and_then(|state| state.get("Running"))
        .and_then(serde_json::Value::as_bool)
        .ok_or(DockerSearchError::InvalidInspection)?;
    Ok(DockerContainerStatus {
        id,
        name,
        image,
        labels,
        running,
    })
}

fn ensure_owned_container(
    status: &DockerContainerStatus,
    expected_name: &str,
) -> Result<(), DockerSearchError> {
    if status.name != expected_name
        || status.labels.get(MANAGED_LABEL_KEY).map(String::as_str) != Some(MANAGED_LABEL_VALUE)
        || status.labels.get(COMPONENT_LABEL_KEY).map(String::as_str) != Some(COMPONENT_LABEL_VALUE)
        || status.labels.get(OWNER_LABEL_KEY).map(String::as_str) != Some(OWNER_LABEL_VALUE)
    {
        return Err(DockerSearchError::OwnershipConflict {
            resource: expected_name.to_owned(),
        });
    }
    if !is_pinned_official_image(&status.image) {
        return Err(DockerSearchError::ImageIntegrityMismatch {
            expected: OFFICIAL_SEARXNG_IMAGE_DIGEST.to_owned(),
            actual: status.image.clone(),
        });
    }
    Ok(())
}

/// Validates the digest returned by docker image inspect.
///
/// # Errors
///
/// Returns an error for malformed output or a digest other than the pinned official image.
pub fn validate_image_inspect_digests(stdout: &[u8]) -> Result<(), DockerSearchError> {
    let digests: Vec<String> =
        serde_json::from_slice(stdout).map_err(|_| DockerSearchError::InvalidImageInspection)?;
    if digests.iter().any(|digest| {
        let Some((repository, image_digest)) = digest.rsplit_once('@') else {
            return false;
        };
        (repository == OFFICIAL_SEARXNG_IMAGE_REPOSITORY || repository == "searxng/searxng")
            && image_digest == OFFICIAL_SEARXNG_IMAGE_DIGEST
    }) {
        Ok(())
    } else {
        Err(DockerSearchError::ImageIntegrityMismatch {
            expected: OFFICIAL_SEARXNG_IMAGE_DIGEST.to_owned(),
            actual: digests.join(","),
        })
    }
}

fn is_pinned_official_image(image: &str) -> bool {
    let Some((repository, digest)) = image.rsplit_once('@') else {
        return false;
    };
    let repository = repository.strip_suffix(":latest").unwrap_or(repository);
    (repository == OFFICIAL_SEARXNG_IMAGE_REPOSITORY || repository == "searxng/searxng")
        && digest == OFFICIAL_SEARXNG_IMAGE_DIGEST
}

fn prepare_paths(paths: &DockerSearchPaths) -> Result<(), DockerSearchError> {
    ensure_owned_root(paths.root())?;
    for path in [paths.storage(), paths.config()] {
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => return Err(DockerSearchError::NotDirectory(path)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                create_private_dir(&path)
                    .map_err(|source| io_error("create private Docker path", path, source))?;
            }
            Err(source) => return Err(io_error("inspect private Docker path", path, source)),
        }
    }
    Ok(())
}

fn ensure_owned_root(path: &Path) -> Result<(), DockerSearchError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => {
            let marker = path.join(ROOT_MARKER_NAME);
            let marker_metadata = fs::symlink_metadata(&marker).map_err(|_| {
                DockerSearchError::OwnershipConflict {
                    resource: path.display().to_string(),
                }
            })?;
            if !marker_metadata.is_file() {
                return Err(DockerSearchError::OwnershipConflict {
                    resource: path.display().to_string(),
                });
            }
            let contents = fs::read(&marker).map_err(|_| DockerSearchError::OwnershipConflict {
                resource: path.display().to_string(),
            })?;
            if contents != ROOT_MARKER {
                return Err(DockerSearchError::OwnershipConflict {
                    resource: path.display().to_string(),
                });
            }
            Ok(())
        }
        Ok(_) => Err(DockerSearchError::NotDirectory(path.to_path_buf())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let parent = path
                .parent()
                .ok_or_else(|| DockerSearchError::InvalidPath(path.to_path_buf()))?;
            create_private_dir_all(parent).map_err(|source| {
                io_error("create Docker data parent", parent.to_path_buf(), source)
            })?;
            create_private_dir(path).map_err(|source| {
                io_error("create Docker data root", path.to_path_buf(), source)
            })?;
            let marker = path.join(ROOT_MARKER_NAME);
            let mut file = open_private_new(&marker).map_err(|source| {
                io_error("create Docker ownership marker", marker.clone(), source)
            })?;
            file.write_all(ROOT_MARKER)
                .and_then(|()| file.sync_all())
                .map_err(|source| io_error("write Docker ownership marker", marker, source))
        }
        Err(source) => Err(io_error(
            "inspect Docker data root",
            path.to_path_buf(),
            source,
        )),
    }
}

fn write_receipt(paths: &DockerSearchPaths, host_port: u16) -> Result<(), DockerSearchError> {
    let receipt = DockerReceipt {
        schema_version: RECEIPT_SCHEMA_VERSION,
        owner: OWNER_LABEL_VALUE.to_owned(),
        container_name: MANAGED_CONTAINER_NAME.to_owned(),
        image: OFFICIAL_SEARXNG_IMAGE.to_owned(),
        host_port,
    };
    let contents =
        serde_json::to_vec_pretty(&receipt).map_err(|_| DockerSearchError::InvalidReceipt)?;
    write_owned_file(&paths.receipt(), &contents)
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DockerReceipt {
    schema_version: u8,
    owner: String,
    container_name: String,
    image: String,
    host_port: u16,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DockerRecovery {
    schema_version: u8,
    owner: String,
    operation: String,
    message: String,
    prior_running: bool,
}

fn record_recovery(
    paths: &DockerSearchPaths,
    operation: DockerOperation,
    message: &str,
    prior_running: bool,
) -> Result<(), DockerSearchError> {
    let recovery = DockerRecovery {
        schema_version: RECOVERY_SCHEMA_VERSION,
        owner: OWNER_LABEL_VALUE.to_owned(),
        operation: operation.as_str().to_owned(),
        message: message.to_owned(),
        prior_running,
    };
    let contents =
        serde_json::to_vec_pretty(&recovery).map_err(|_| DockerSearchError::InvalidRecovery)?;
    write_owned_file(&paths.recovery(), &contents)
}

fn write_owned_file(path: &Path, contents: &[u8]) -> Result<(), DockerSearchError> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if !metadata.is_file() {
            return Err(DockerSearchError::OwnershipConflict {
                resource: path.display().to_string(),
            });
        }
        let existing = fs::read(path)
            .map_err(|source| io_error("read owned Docker state", path.to_path_buf(), source))?;
        let owned = serde_json::from_slice::<serde_json::Value>(&existing)
            .ok()
            .and_then(|value| {
                value
                    .get("owner")
                    .and_then(serde_json::Value::as_str)
                    .map(|owner| owner == OWNER_LABEL_VALUE)
            })
            .unwrap_or(false);
        if !owned {
            return Err(DockerSearchError::OwnershipConflict {
                resource: path.display().to_string(),
            });
        }
    }
    let mut file = open_private_truncate(path)
        .map_err(|source| io_error("open private Docker state", path.to_path_buf(), source))?;
    file.write_all(contents)
        .and_then(|()| file.sync_all())
        .map_err(|source| io_error("write private Docker state", path.to_path_buf(), source))
}

fn clear_owned_file(path: &Path) -> Result<(), DockerSearchError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(source) => return Err(io_error("inspect Docker state", path.to_path_buf(), source)),
    };
    if !metadata.is_file() {
        return Err(DockerSearchError::OwnershipConflict {
            resource: path.display().to_string(),
        });
    }
    let contents = fs::read(path)
        .map_err(|source| io_error("read Docker state", path.to_path_buf(), source))?;
    let owned = serde_json::from_slice::<serde_json::Value>(&contents)
        .ok()
        .and_then(|value| {
            value
                .get("owner")
                .and_then(serde_json::Value::as_str)
                .map(|owner| owner == OWNER_LABEL_VALUE)
        })
        .unwrap_or(false);
    if !owned {
        return Err(DockerSearchError::OwnershipConflict {
            resource: path.display().to_string(),
        });
    }
    fs::remove_file(path)
        .map_err(|source| io_error("remove Docker state", path.to_path_buf(), source))
}

fn io_error(operation: &'static str, path: PathBuf, source: io::Error) -> DockerSearchError {
    DockerSearchError::Io {
        operation,
        path,
        source,
    }
}

#[derive(Debug, Error)]
pub enum DockerSearchError {
    #[error("managed Docker search requires a non-zero host port")]
    InvalidHostPort,
    #[error("the pinned official SearXNG image contract is invalid")]
    InvalidImageContract,
    #[error("Docker resource is not owned by nan-harness: {resource}")]
    OwnershipConflict { resource: String },
    #[error("Docker path is not a directory: {0}")]
    NotDirectory(PathBuf),
    #[error("Docker path is invalid: {0}")]
    InvalidPath(PathBuf),
    #[error("Docker container was not found")]
    ContainerNotFound,
    #[error("Docker container is not running")]
    ContainerNotRunning,
    #[error("Docker operation plan has the wrong operation")]
    InvalidOperationPlan,
    #[error("cannot {operation} while {count} search session(s) are active")]
    ActiveSessions {
        operation: &'static str,
        count: usize,
    },
    #[error("could not start Docker command '{program}': {source}")]
    CommandStart {
        program: String,
        #[source]
        source: io::Error,
    },
    #[error("Docker command for {operation:?} exited with status {status:?}")]
    CommandFailed {
        operation: DockerOperation,
        status: Option<i32>,
    },
    #[error("Docker inspection output is invalid")]
    InvalidInspection,
    #[error("Docker image inspection output is invalid")]
    InvalidImageInspection,
    #[error("Docker image integrity mismatch (expected {expected}, got {actual})")]
    ImageIntegrityMismatch { expected: String, actual: String },
    #[error("Docker state receipt is invalid")]
    InvalidReceipt,
    #[error("Docker recovery record is invalid")]
    InvalidRecovery,
    #[error("Docker update rollback failed after the update failed: {rollback}")]
    RollbackFailed {
        operation: Box<Self>,
        rollback: Box<Self>,
    },
    #[error("could not {operation} '{path}': {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    fn owned_json(name: &str, running: bool) -> Vec<u8> {
        serde_json::json!({
            "Id": format!("id-{name}"),
            "Name": format!("/{name}"),
            "Config": {
                "Image": OFFICIAL_SEARXNG_IMAGE,
                "Labels": {
                    MANAGED_LABEL_KEY: MANAGED_LABEL_VALUE,
                    COMPONENT_LABEL_KEY: COMPONENT_LABEL_VALUE,
                    OWNER_LABEL_KEY: OWNER_LABEL_VALUE
                }
            },
            "State": {"Running": running}
        })
        .to_string()
        .into_bytes()
    }

    fn missing() -> DockerCommandOutput {
        DockerCommandOutput {
            status: Some(1),
            stdout: Vec::new(),
            stderr: b"Error: No such container".to_vec(),
        }
    }

    #[derive(Clone, Default)]
    struct FakeExecutor {
        outputs: Arc<Mutex<VecDeque<DockerCommandOutput>>>,
        commands: Arc<Mutex<Vec<DockerCommand>>>,
    }

    impl FakeExecutor {
        fn with_outputs(outputs: impl IntoIterator<Item = DockerCommandOutput>) -> Self {
            Self {
                outputs: Arc::new(Mutex::new(outputs.into_iter().collect())),
                commands: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    impl DockerExecutor for FakeExecutor {
        fn execute(
            &self,
            command: &DockerCommand,
        ) -> Result<DockerCommandOutput, DockerSearchError> {
            self.commands
                .lock()
                .expect("commands lock")
                .push(command.clone());
            Ok(self
                .outputs
                .lock()
                .expect("outputs lock")
                .pop_front()
                .unwrap_or_else(DockerCommandOutput::success))
        }
    }

    #[test]
    fn create_plan_pins_image_labels_mounts_and_loopback_publish() {
        let paths = DockerSearchPaths::under("/private/search");
        let plan = plan_docker_search_create(DockerSearchRequest::configured(19080), paths)
            .expect("plan should build");
        let args = &plan.commands()[0].arguments;
        assert_eq!(
            args[0..4],
            ["container", "create", "--name", MANAGED_CONTAINER_NAME]
        );
        assert!(
            args.iter()
                .any(|arg| arg == &format!("{MANAGED_LABEL_KEY}={MANAGED_LABEL_VALUE}"))
        );
        assert!(
            args.iter()
                .any(|arg| arg == &format!("{COMPONENT_LABEL_KEY}={COMPONENT_LABEL_VALUE}"))
        );
        assert!(
            args.iter()
                .any(|arg| arg == &format!("{OWNER_LABEL_KEY}={OWNER_LABEL_VALUE}"))
        );
        assert!(args.iter().any(|arg| arg == "127.0.0.1:19080:8080"));
        assert!(
            args.iter()
                .any(|arg| arg.contains("source=/private/search/storage"))
        );
        assert!(
            args.iter()
                .any(|arg| arg.contains("source=/private/search/config"))
        );
        assert_eq!(
            args.last().map(String::as_str),
            Some(OFFICIAL_SEARXNG_IMAGE)
        );
        assert!(
            plan.commands()
                .iter()
                .all(|command| command.program.as_os_str() == "docker")
        );
    }

    #[test]
    fn every_plan_targets_one_exact_container_and_never_engine_wide_state() {
        let request = DockerSearchRequest::configured(DEFAULT_HOST_PORT);
        let paths = DockerSearchPaths::under("/private/search");
        for operation in [
            DockerOperation::Start,
            DockerOperation::Inspect,
            DockerOperation::Stop,
            DockerOperation::Remove,
            DockerOperation::Update,
        ] {
            let plan = match operation {
                DockerOperation::Start => plan_docker_search_start(request, paths.clone()),
                DockerOperation::Inspect => plan_docker_search_inspect(request, paths.clone()),
                DockerOperation::Stop => plan_docker_search_stop(request, paths.clone()),
                DockerOperation::Remove => plan_docker_search_remove(request, paths.clone()),
                DockerOperation::Update => plan_docker_search_update(request, paths.clone()),
                DockerOperation::Create => unreachable!("covered by the create test"),
            }
            .expect("plan should build");
            for command in plan.commands() {
                assert!(
                    !command
                        .arguments
                        .iter()
                        .any(|arg| arg == "ps" || arg == "system")
                );
            }
        }
    }

    #[test]
    fn pinned_digest_validation_rejects_wrong_digest_and_accepts_fixture() {
        let correct =
            format!("[\"{OFFICIAL_SEARXNG_IMAGE_REPOSITORY}@{OFFICIAL_SEARXNG_IMAGE_DIGEST}\"]");
        validate_image_inspect_digests(correct.as_bytes()).expect("pinned digest should pass");
        let wrong = b"[\"docker.io/searxng/searxng@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"]";
        assert!(matches!(
            validate_image_inspect_digests(wrong),
            Err(DockerSearchError::ImageIntegrityMismatch { .. })
        ));
    }

    #[test]
    fn status_inspection_never_starts_container() {
        let fake = FakeExecutor::with_outputs([DockerCommandOutput {
            status: Some(0),
            stdout: owned_json(MANAGED_CONTAINER_NAME, true),
            stderr: Vec::new(),
        }]);
        let commands = fake.commands.clone();
        let manager = DockerSearchManager::new(
            fake,
            DockerSearchPaths::under("/private/search"),
            DockerSearchRequest::configured(DEFAULT_HOST_PORT),
        )
        .expect("manager should build");
        assert!(matches!(
            manager.status_without_starting().expect("status"),
            DockerSearchStatus::Running(_)
        ));
        let calls = commands.lock().expect("commands lock");
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].arguments,
            [
                "container",
                "inspect",
                "--format",
                "{{json .}}",
                MANAGED_CONTAINER_NAME
            ]
        );
    }

    #[test]
    fn foreign_container_is_preserved_and_setup_does_not_create_paths() {
        let foreign = serde_json::json!({
            "Id": "foreign",
            "Name": format!("/{MANAGED_CONTAINER_NAME}"),
            "Config": {"Image": "docker.io/example/foreign:latest", "Labels": {}},
            "State": {"Running": true}
        })
        .to_string()
        .into_bytes();
        let fake = FakeExecutor::with_outputs([DockerCommandOutput {
            status: Some(0),
            stdout: foreign,
            stderr: Vec::new(),
        }]);
        let root = tempfile::tempdir().expect("temporary root");
        let paths = DockerSearchPaths::under(root.path().join("search"));
        let commands = fake.commands.clone();
        let manager = DockerSearchManager::new(
            fake,
            paths.clone(),
            DockerSearchRequest::configured(DEFAULT_HOST_PORT),
        )
        .expect("manager should build");
        assert!(matches!(
            manager.setup(0),
            Err(DockerSearchError::OwnershipConflict { .. })
        ));
        assert!(!paths.root().exists());
        assert_eq!(commands.lock().expect("commands lock").len(), 1);
    }

    #[test]
    fn failed_setup_records_recovery_after_image_command_failure() {
        let fake = FakeExecutor::with_outputs([
            missing(),
            DockerCommandOutput {
                status: Some(1),
                stdout: Vec::new(),
                stderr: b"pull failed".to_vec(),
            },
        ]);
        let root = tempfile::tempdir().expect("temporary root");
        let paths = DockerSearchPaths::under(root.path().join("search"));
        let manager = DockerSearchManager::new(
            fake,
            paths.clone(),
            DockerSearchRequest::configured(DEFAULT_HOST_PORT),
        )
        .expect("manager should build");

        assert!(matches!(
            manager.setup(0),
            Err(DockerSearchError::CommandFailed {
                operation: DockerOperation::Create,
                ..
            })
        ));
        assert!(paths.recovery().exists());
        assert!(paths.storage().exists());
        assert!(paths.config().exists());
    }

    #[test]
    fn wrong_owned_image_is_rejected_before_mutation() {
        let wrong_image = String::from_utf8(owned_json(MANAGED_CONTAINER_NAME, true))
            .expect("fixture JSON should be UTF-8")
            .replace(OFFICIAL_SEARXNG_IMAGE, "docker.io/example/searxng@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        let fake = FakeExecutor::with_outputs([DockerCommandOutput {
            status: Some(0),
            stdout: wrong_image.into_bytes(),
            stderr: Vec::new(),
        }]);
        let manager = DockerSearchManager::new(
            fake,
            DockerSearchPaths::under("/private/search"),
            DockerSearchRequest::configured(DEFAULT_HOST_PORT),
        )
        .expect("manager should build");
        let error = manager.status().expect_err("wrong image should fail");
        assert!(matches!(
            error,
            DockerSearchError::ImageIntegrityMismatch { .. }
        ));
    }

    #[test]
    fn update_failure_restores_prior_container_and_records_recovery() {
        let prior = DockerCommandOutput {
            status: Some(0),
            stdout: owned_json(MANAGED_CONTAINER_NAME, true),
            stderr: Vec::new(),
        };
        let image = DockerCommandOutput {
            status: Some(0),
            stdout: format!(
                "[\"{OFFICIAL_SEARXNG_IMAGE_REPOSITORY}@{OFFICIAL_SEARXNG_IMAGE_DIGEST}\"]"
            )
            .into_bytes(),
            stderr: Vec::new(),
        };
        let candidate_stopped = DockerCommandOutput {
            status: Some(0),
            stdout: owned_json(UPDATE_CANDIDATE_CONTAINER_NAME, false),
            stderr: Vec::new(),
        };
        let backup_stopped = DockerCommandOutput {
            status: Some(0),
            stdout: owned_json(UPDATE_BACKUP_CONTAINER_NAME, false),
            stderr: Vec::new(),
        };
        let start_failure = DockerCommandOutput {
            status: Some(1),
            stdout: Vec::new(),
            stderr: b"failed".to_vec(),
        };
        let fake = FakeExecutor::with_outputs([
            prior,
            missing(),
            missing(),
            DockerCommandOutput::success(),
            image,
            DockerCommandOutput::success(),
            DockerCommandOutput::success(),
            DockerCommandOutput::success(),
            start_failure,
            backup_stopped,
            candidate_stopped,
            DockerCommandOutput::success(),
            DockerCommandOutput::success(),
        ]);
        let root = tempfile::tempdir().expect("temporary root");
        let paths = DockerSearchPaths::under(root.path().join("search"));
        let manager = DockerSearchManager::new(
            fake,
            paths.clone(),
            DockerSearchRequest::configured(DEFAULT_HOST_PORT),
        )
        .expect("manager should build");
        let error = manager
            .update(0)
            .expect_err("start failure should roll back");
        assert!(matches!(
            error,
            DockerSearchError::CommandFailed {
                operation: DockerOperation::Start,
                ..
            }
        ));
        assert!(paths.recovery().exists());
        let recovery: serde_json::Value =
            serde_json::from_slice(&fs::read(paths.recovery()).expect("recovery"))
                .expect("recovery JSON");
        assert_eq!(recovery["operation"], "update");
    }

    #[test]
    fn active_sessions_block_mutating_operations() {
        let manager = DockerSearchManager::new(
            FakeExecutor::default(),
            DockerSearchPaths::under("/private/search"),
            DockerSearchRequest::configured(DEFAULT_HOST_PORT),
        )
        .expect("manager should build");
        assert!(matches!(
            manager.setup(1),
            Err(DockerSearchError::ActiveSessions {
                operation: "setup",
                count: 1
            })
        ));
        assert!(matches!(
            manager.update(1),
            Err(DockerSearchError::ActiveSessions {
                operation: "update",
                count: 1
            })
        ));
        assert!(matches!(
            manager.remove(1),
            Err(DockerSearchError::ActiveSessions {
                operation: "remove",
                count: 1
            })
        ));
    }

    #[test]
    fn remove_cleans_orphaned_update_containers_and_recovery_state() {
        let fake = FakeExecutor::with_outputs([
            DockerCommandOutput {
                status: Some(0),
                stdout: owned_json(MANAGED_CONTAINER_NAME, false),
                stderr: Vec::new(),
            },
            DockerCommandOutput {
                status: Some(0),
                stdout: owned_json(UPDATE_BACKUP_CONTAINER_NAME, false),
                stderr: Vec::new(),
            },
            missing(),
            DockerCommandOutput::success(),
            DockerCommandOutput::success(),
        ]);
        let root = tempfile::tempdir().expect("temporary root");
        let paths = DockerSearchPaths::under(root.path().join("search"));
        fs::create_dir_all(paths.root()).expect("state root");
        fs::write(
            paths.recovery(),
            serde_json::json!({"owner": OWNER_LABEL_VALUE}).to_string(),
        )
        .expect("recovery state");
        let manager = DockerSearchManager::new(
            fake.clone(),
            paths.clone(),
            DockerSearchRequest::configured(DEFAULT_HOST_PORT),
        )
        .expect("manager should build");

        assert_eq!(
            manager
                .remove(0)
                .expect("remove should clean all owned state"),
            DockerSearchOutcome::Removed
        );
        let commands = fake.commands.lock().expect("commands lock");
        assert!(commands.iter().any(|command| command.arguments.last()
            == Some(&MANAGED_CONTAINER_NAME.to_owned())));
        assert!(
            commands.iter().any(|command| command.arguments.last()
                == Some(&UPDATE_BACKUP_CONTAINER_NAME.to_owned()))
        );
        assert!(!paths.recovery().exists());
    }

    #[test]
    fn remove_rejects_foreign_update_container_before_removing_stable_state() {
        let foreign = serde_json::json!({
            "Id": "foreign",
            "Name": format!("/{UPDATE_BACKUP_CONTAINER_NAME}"),
            "Config": {"Image": "docker.io/example/foreign:latest", "Labels": {}},
            "State": {"Running": false}
        })
        .to_string()
        .into_bytes();
        let fake = FakeExecutor::with_outputs([
            DockerCommandOutput {
                status: Some(0),
                stdout: owned_json(MANAGED_CONTAINER_NAME, false),
                stderr: Vec::new(),
            },
            DockerCommandOutput {
                status: Some(0),
                stdout: foreign,
                stderr: Vec::new(),
            },
        ]);
        let manager = DockerSearchManager::new(
            fake.clone(),
            DockerSearchPaths::under("/private/search"),
            DockerSearchRequest::configured(DEFAULT_HOST_PORT),
        )
        .expect("manager should build");

        assert!(matches!(
            manager.remove(0),
            Err(DockerSearchError::OwnershipConflict { resource })
                if resource == UPDATE_BACKUP_CONTAINER_NAME
        ));
        assert_eq!(fake.commands.lock().expect("commands lock").len(), 2);
    }

    #[test]
    fn owned_data_cleanup_removes_backend_state_without_touching_images() {
        let root = tempfile::tempdir().expect("temporary root");
        let paths = DockerSearchPaths::under(root.path().join("search"));
        ensure_owned_root(paths.root()).expect("root should be owned");
        fs::create_dir_all(paths.storage()).expect("storage should exist");
        fs::create_dir_all(paths.config()).expect("config should exist");
        write_owned_file(&paths.receipt(), br#"{"owner":"nanh-search-docker-v1"}"#)
            .expect("receipt should be owned");
        write_owned_file(&paths.recovery(), br#"{"owner":"nanh-search-docker-v1"}"#)
            .expect("recovery should be owned");

        cleanup_owned_docker_search_data(&paths).expect("owned data should be removed");

        assert!(!paths.root().exists());
    }

    #[test]
    fn owned_data_cleanup_preserves_unknown_root_entries() {
        let root = tempfile::tempdir().expect("temporary root");
        let paths = DockerSearchPaths::under(root.path().join("search"));
        ensure_owned_root(paths.root()).expect("root should be owned");
        fs::create_dir_all(paths.storage()).expect("storage should exist");
        let foreign = paths.root().join("foreign");
        fs::write(&foreign, b"preserve").expect("foreign state should exist");

        cleanup_owned_docker_search_data(&paths).expect("owned data should be removed");

        assert!(foreign.exists());
        assert!(paths.root().exists());
        assert!(!paths.storage().exists());
    }
}
