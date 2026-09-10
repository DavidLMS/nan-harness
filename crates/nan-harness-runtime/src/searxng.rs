//! Explicit, standalone `SearXNG` installation support.
//!
//! This module deliberately has no connection to launch preparation or process supervision.
//! Callers must first build a [`SearxngInstallPlan`] and then explicitly execute it with an
//! archive supplied by the caller. In particular, discovering search configuration does not
//! download Python, install dependencies, or start `SearXNG`.

use nan_harness_private_fs::{create_private_dir, create_private_dir_all, open_private_new};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;
use url::Url;

/// The upstream `SearXNG` repository.
pub const OFFICIAL_SEARXNG_REPOSITORY: &str = "https://github.com/searxng/searxng";
/// The upstream source archive for the pinned `SearXNG` snapshot.
pub const OFFICIAL_SEARXNG_ARCHIVE_URL: &str =
    "https://github.com/searxng/searxng/archive/931fd9787b1517d88af2876175d8c31b03e11671.tar.gz";
/// The immutable upstream revision used by [`SearxngSourceMetadata::official`].
pub const OFFICIAL_SEARXNG_REVISION: &str = "931fd9787b1517d88af2876175d8c31b03e11671";
/// The `SearXNG` version generated from the pinned upstream revision.
pub const OFFICIAL_SEARXNG_VERSION: &str = "2026.9.10+931fd978";
/// SHA-256 of [`OFFICIAL_SEARXNG_ARCHIVE_URL`].
pub const OFFICIAL_SEARXNG_ARCHIVE_SHA256: &str =
    "a9d031bd3d4ae3c48bae5cd342326f7f1067318649dc861051d742fff5b661dd";
/// The upstream license for `SearXNG`.
pub const OFFICIAL_SEARXNG_LICENSE_URL: &str =
    "https://github.com/searxng/searxng/blob/931fd9787b1517d88af2876175d8c31b03e11671/LICENSE";

const INSTALL_SCHEMA_VERSION: u8 = 1;
const OWNER_MARKER: &str = ".nanh-owned";
const ROOT_OWNER_MARKER: &str = "nanh-searxng-root-v1\n";
const INSTALL_OWNER_MARKER: &str = "nanh-searxng-install-v1\n";
const STAGING_DIRECTORY: &str = ".staging";
const ACTIVE_DIRECTORY: &str = "current";
const BACKUP_DIRECTORY: &str = ".previous";
const ARCHIVE_NAME: &str = "searxng.tar.gz";
const SOURCE_DIRECTORY: &str = "source";
const PYTHON_ENVIRONMENT_DIRECTORY: &str = "python";
const INSTALL_METADATA_NAME: &str = "install.json";
const HEX: &[u8; 16] = b"0123456789abcdef";

/// Supported Unix `SearXNG` installation targets.
///
/// The enum is intentionally independent of the machine on which the plan is built. This lets
/// release tooling and tests select each supported target without pretending that the host is the
/// target. Windows is not represented because this standalone recipe is Unix-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SearxngPlatform {
    /// Intel macOS (`x86_64-apple-darwin`).
    MacOsX86_64,
    /// Apple Silicon macOS (`aarch64-apple-darwin`).
    MacOsArm64,
    /// Intel/AMD Linux (`x86_64-unknown-linux-*`).
    LinuxX86_64,
    /// 64-bit ARM Linux (`aarch64-unknown-linux-*`).
    LinuxArm64,
}

impl SearxngPlatform {
    /// Selects a platform from explicit target operating-system and architecture names.
    #[must_use]
    pub fn from_target(target_os: &str, target_arch: &str) -> Option<Self> {
        match (target_os, target_arch) {
            ("macos" | "darwin", "x86_64") => Some(Self::MacOsX86_64),
            ("macos" | "darwin", "aarch64" | "arm64") => Some(Self::MacOsArm64),
            ("linux", "x86_64" | "amd64") => Some(Self::LinuxX86_64),
            ("linux", "aarch64" | "arm64") => Some(Self::LinuxArm64),
            _ => None,
        }
    }

    /// Selects a platform from a Rust target triple.
    #[must_use]
    pub fn from_target_triple(target: &str) -> Option<Self> {
        let mut components = target.split('-');
        let arch = components.next()?;
        let vendor_or_os = components.next()?;
        let os = components.next()?;
        if vendor_or_os == "apple" && os == "darwin" {
            return Self::from_target("macos", arch);
        }
        if os == "linux" {
            return Self::from_target("linux", arch);
        }
        None
    }

    /// Selects this process's platform, if it is one of the supported Unix targets.
    #[must_use]
    pub fn current() -> Option<Self> {
        Self::from_target(std::env::consts::OS, std::env::consts::ARCH)
    }

    /// Stable metadata value used in installation receipts and plans.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MacOsX86_64 => "macos-x86_64",
            Self::MacOsArm64 => "macos-arm64",
            Self::LinuxX86_64 => "linux-x86_64",
            Self::LinuxArm64 => "linux-arm64",
        }
    }

    fn python_command(self) -> PathBuf {
        // Python is deliberately resolved by the explicit recipe selection rather than by
        // looking at the host's PATH while constructing a plan. The executor may still apply its
        // normal PATH policy when the plan is explicitly run.
        let _ = self;
        PathBuf::from("python3")
    }
}

/// Immutable source and integrity metadata for one `SearXNG` snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearxngSourceMetadata {
    /// Human-readable upstream package version.
    pub version: String,
    /// Full immutable Git revision used to produce the archive.
    pub revision: String,
    /// Official archive URL.
    pub archive_url: String,
    /// SHA-256 digest of the exact archive bytes.
    pub archive_sha256: String,
    /// Upstream repository URL for attribution and diagnostics.
    pub repository_url: String,
    /// Upstream license URL.
    pub license_url: String,
}

impl SearxngSourceMetadata {
    /// Returns the pinned upstream metadata shipped with this release.
    #[must_use]
    pub fn official() -> Self {
        Self {
            version: OFFICIAL_SEARXNG_VERSION.to_owned(),
            revision: OFFICIAL_SEARXNG_REVISION.to_owned(),
            archive_url: OFFICIAL_SEARXNG_ARCHIVE_URL.to_owned(),
            archive_sha256: OFFICIAL_SEARXNG_ARCHIVE_SHA256.to_owned(),
            repository_url: OFFICIAL_SEARXNG_REPOSITORY.to_owned(),
            license_url: OFFICIAL_SEARXNG_LICENSE_URL.to_owned(),
        }
    }

    /// Checks that metadata is complete enough to be used for a plan or integrity check.
    ///
    /// # Errors
    ///
    /// Returns [`SearxngInstallError::InvalidSourceMetadata`] for missing or malformed fields.
    pub fn validate(&self) -> Result<(), SearxngInstallError> {
        if self.version.is_empty() || self.version.chars().any(char::is_whitespace) {
            return Err(SearxngInstallError::InvalidSourceMetadata("version"));
        }
        if self.revision.is_empty() || self.revision.chars().any(char::is_whitespace) {
            return Err(SearxngInstallError::InvalidSourceMetadata("revision"));
        }
        validate_https_url(&self.archive_url, "archive URL")?;
        validate_https_url(&self.repository_url, "repository URL")?;
        validate_https_url(&self.license_url, "license URL")?;
        if self.archive_sha256.len() != 64
            || !self
                .archive_sha256
                .chars()
                .all(|character| character.is_ascii_hexdigit())
        {
            return Err(SearxngInstallError::InvalidSourceMetadata(
                "archive SHA-256",
            ));
        }
        Ok(())
    }
}

fn validate_https_url(value: &str, field: &'static str) -> Result<(), SearxngInstallError> {
    let url = Url::parse(value).map_err(|_| SearxngInstallError::InvalidSourceMetadata(field))?;
    if url.scheme() != "https" || url.host_str().is_none() {
        return Err(SearxngInstallError::InvalidSourceMetadata(field));
    }
    Ok(())
}

/// User-selected installation behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearxngInstallRequest {
    /// Whether standalone `SearXNG` has been explicitly configured.
    pub configured: bool,
    /// Whether the recipe may create a virtualenv and install Python dependencies.
    pub bootstrap_python: bool,
}

impl SearxngInstallRequest {
    /// Returns the inert default. No source or dependencies are touched.
    #[must_use]
    pub const fn unconfigured() -> Self {
        Self {
            configured: false,
            bootstrap_python: false,
        }
    }

    /// Requests source installation while using an already available Python toolchain.
    #[must_use]
    pub const fn source_only() -> Self {
        Self {
            configured: true,
            bootstrap_python: false,
        }
    }

    /// Requests source installation and explicit Python/dependency bootstrapping.
    #[must_use]
    pub const fn with_python_bootstrap() -> Self {
        Self {
            configured: true,
            bootstrap_python: true,
        }
    }
}

/// Backwards-compatible name for callers that model this as options rather than a request.
pub type SearxngInstallOptions = SearxngInstallRequest;

/// All files owned by one standalone installation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearxngInstallPaths {
    root: PathBuf,
}

impl SearxngInstallPaths {
    /// Creates paths below an explicit, testable root directory.
    #[must_use]
    pub fn under(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Chooses the platform's per-user data directory below `home`.
    #[must_use]
    pub fn for_user_home(home: &Path, platform: SearxngPlatform) -> Self {
        let root = match platform {
            SearxngPlatform::MacOsX86_64 | SearxngPlatform::MacOsArm64 => home
                .join("Library")
                .join("Application Support")
                .join("nan-harness")
                .join("searxng"),
            SearxngPlatform::LinuxX86_64 | SearxngPlatform::LinuxArm64 => home
                .join(".local")
                .join("share")
                .join("nan-harness")
                .join("searxng"),
        };
        Self::under(root)
    }

    /// The private nan-harness `SearXNG` root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The atomically published active installation directory.
    #[must_use]
    pub fn active(&self) -> PathBuf {
        self.root.join(ACTIVE_DIRECTORY)
    }

    /// The private staging directory used before publication.
    #[must_use]
    pub fn staging(&self) -> PathBuf {
        self.root.join(STAGING_DIRECTORY)
    }

    /// The private rollback directory used during atomic publication.
    #[must_use]
    pub fn previous(&self) -> PathBuf {
        self.root.join(BACKUP_DIRECTORY)
    }

    /// The source checkout inside a supplied installation directory.
    #[must_use]
    pub fn source_in(&self, installation: &Path) -> PathBuf {
        installation.join(SOURCE_DIRECTORY)
    }

    /// The virtualenv inside a supplied installation directory.
    #[must_use]
    pub fn python_in(&self, installation: &Path) -> PathBuf {
        installation.join(PYTHON_ENVIRONMENT_DIRECTORY)
    }

    /// The install receipt inside a supplied installation directory.
    #[must_use]
    pub fn metadata_in(&self, installation: &Path) -> PathBuf {
        installation.join(INSTALL_METADATA_NAME)
    }
}

/// A process invocation in an installation recipe.
///
/// This is data, not an invocation. Constructing or inspecting a command never starts a process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearxngCommand {
    /// Program to execute.
    pub program: PathBuf,
    /// Positional arguments passed without shell interpolation.
    pub arguments: Vec<String>,
    /// Directory from which the program is run.
    pub current_directory: PathBuf,
}

impl SearxngCommand {
    fn new(program: impl Into<PathBuf>, current_directory: &Path) -> Self {
        Self {
            program: program.into(),
            arguments: Vec::new(),
            current_directory: current_directory.to_path_buf(),
        }
    }

    fn with_arguments(mut self, arguments: impl IntoIterator<Item = String>) -> Self {
        self.arguments = arguments.into_iter().collect();
        self
    }
}

/// A pure setup plan. It is safe to serialize, display, or test without touching the filesystem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearxngInstallPlan {
    platform: SearxngPlatform,
    source: SearxngSourceMetadata,
    paths: SearxngInstallPaths,
    bootstrap_python: bool,
    commands: Vec<SearxngCommand>,
}

impl SearxngInstallPlan {
    /// The target selected for this plan.
    #[must_use]
    pub const fn platform(&self) -> SearxngPlatform {
        self.platform
    }

    /// The source metadata selected for this plan.
    #[must_use]
    pub fn source(&self) -> &SearxngSourceMetadata {
        &self.source
    }

    /// The private paths selected for this plan.
    #[must_use]
    pub const fn paths(&self) -> &SearxngInstallPaths {
        &self.paths
    }

    /// Whether this plan includes Python and dependency bootstrap commands.
    #[must_use]
    pub const fn bootstraps_python(&self) -> bool {
        self.bootstrap_python
    }

    /// Commands that an explicit installer may execute in order.
    #[must_use]
    pub fn commands(&self) -> &[SearxngCommand] {
        &self.commands
    }

    /// Builds the command used to run the installed `SearXNG` instance.
    ///
    /// This method only returns command data; it does not start `SearXNG`.
    #[must_use]
    pub fn runtime_command(&self) -> SearxngCommand {
        let active_source = self.paths.source_in(&self.paths.active());
        let program = if self.bootstrap_python {
            self.paths
                .python_in(&self.paths.active())
                .join("bin/python")
        } else {
            self.platform.python_command()
        };
        SearxngCommand::new(program, &active_source).with_arguments(["searx/webapp.py".to_owned()])
    }
}

/// Pure no-op or setup result from [`plan_searxng_installation`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearxngSetupPlan {
    /// `SearXNG` is not configured; nothing may be read, written, or executed.
    NoOp,
    /// Explicit setup recipe.
    Setup(SearxngInstallPlan),
}

impl SearxngSetupPlan {
    /// Returns the setup plan when configured, or `None` for the intentional no-op.
    #[must_use]
    pub fn as_install_plan(&self) -> Option<&SearxngInstallPlan> {
        match self {
            Self::NoOp => None,
            Self::Setup(plan) => Some(plan),
        }
    }
}

/// Creates a pure `SearXNG` installation recipe.
///
/// This function never creates directories, downloads source, invokes Python, or starts a
/// process. An unconfigured request returns [`SearxngSetupPlan::NoOp`] before validating the
/// platform or metadata, so normal launch paths can safely pass their configuration through it.
///
/// # Errors
///
/// Returns an error when a configured request targets an unsupported platform or has malformed
/// source metadata.
pub fn plan_searxng_installation(
    request: SearxngInstallRequest,
    platform: Option<SearxngPlatform>,
    paths: SearxngInstallPaths,
    source: SearxngSourceMetadata,
) -> Result<SearxngSetupPlan, SearxngInstallError> {
    if !request.configured {
        return Ok(SearxngSetupPlan::NoOp);
    }
    let platform = platform.ok_or(SearxngInstallError::UnsupportedPlatform)?;
    source.validate()?;
    let staging = paths.staging();
    let source_directory = paths.source_in(&staging);
    let mut commands = vec![SearxngCommand::new("tar", paths.root()).with_arguments([
        "--extract".to_owned(),
        "--gzip".to_owned(),
        "--file".to_owned(),
        staging.join(ARCHIVE_NAME).display().to_string(),
        "--strip-components=1".to_owned(),
        "--directory".to_owned(),
        source_directory.display().to_string(),
    ])];
    if request.bootstrap_python {
        let python = platform.python_command();
        let environment = paths.python_in(&staging);
        commands.extend([
            SearxngCommand::new(&python, &source_directory).with_arguments([
                "-m".to_owned(),
                "venv".to_owned(),
                environment.display().to_string(),
            ]),
            SearxngCommand::new(environment.join("bin/python"), &source_directory).with_arguments(
                [
                    "-m".to_owned(),
                    "pip".to_owned(),
                    "install".to_owned(),
                    "--upgrade".to_owned(),
                    "pip".to_owned(),
                    "setuptools".to_owned(),
                    "wheel".to_owned(),
                ],
            ),
            SearxngCommand::new(environment.join("bin/python"), &source_directory).with_arguments(
                [
                    "-m".to_owned(),
                    "pip".to_owned(),
                    "install".to_owned(),
                    "--requirement".to_owned(),
                    source_directory
                        .join("requirements.txt")
                        .display()
                        .to_string(),
                ],
            ),
            SearxngCommand::new(environment.join("bin/python"), &source_directory).with_arguments(
                [
                    "-m".to_owned(),
                    "pip".to_owned(),
                    "install".to_owned(),
                    "--no-build-isolation".to_owned(),
                    "--editable".to_owned(),
                    ".".to_owned(),
                ],
            ),
        ]);
    }
    Ok(SearxngSetupPlan::Setup(SearxngInstallPlan {
        platform,
        source,
        paths,
        bootstrap_python: request.bootstrap_python,
        commands,
    }))
}

/// Plans for an explicit target triple without consulting the host platform.
///
/// # Errors
///
/// Returns [`SearxngInstallError::UnsupportedPlatform`] for an unsupported target or propagates
/// source metadata validation errors for a configured request.
pub fn plan_searxng_installation_for_target(
    request: SearxngInstallRequest,
    target: &str,
    paths: SearxngInstallPaths,
    source: SearxngSourceMetadata,
) -> Result<SearxngSetupPlan, SearxngInstallError> {
    plan_searxng_installation(
        request,
        SearxngPlatform::from_target_triple(target),
        paths,
        source,
    )
}

/// Executes explicit commands for an installation plan.
pub trait SearxngCommandExecutor {
    /// Executes one command without exposing its output through the installer API.
    ///
    /// # Errors
    ///
    /// Returns an error when the command cannot start or exits unsuccessfully.
    fn execute(&self, command: &SearxngCommand) -> Result<(), SearxngInstallError>;
}

/// The production command executor. It is only used by callers that explicitly install `SearXNG`.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessSearxngCommandExecutor;

impl SearxngCommandExecutor for ProcessSearxngCommandExecutor {
    fn execute(&self, command: &SearxngCommand) -> Result<(), SearxngInstallError> {
        let status = Command::new(&command.program)
            .args(&command.arguments)
            .current_dir(&command.current_directory)
            .status()
            .map_err(|source| SearxngInstallError::CommandStart {
                program: command.program.display().to_string(),
                source,
            })?;
        if status.success() {
            Ok(())
        } else {
            Err(SearxngInstallError::CommandFailed {
                program: command.program.display().to_string(),
                status: status.code(),
            })
        }
    }
}

/// Result of an explicit setup operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearxngInstallOutcome {
    /// The request was unconfigured and no state was changed.
    NoOp,
    /// The setup was published atomically.
    Installed(SearxngInstallation),
}

/// Receipt for a published `SearXNG` installation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearxngInstallation {
    /// Published active directory.
    pub active_directory: PathBuf,
    /// Pinned upstream version installed.
    pub version: String,
    /// Explicit target platform.
    pub platform: SearxngPlatform,
    /// Whether Python/dependencies were bootstrapped.
    pub python_bootstrapped: bool,
}

/// Downloads are intentionally outside this API: verify caller-provided bytes and install them.
///
/// The digest is calculated before any state is created. A mismatch therefore cannot leave a
/// partially initialized `SearXNG` root behind.
///
/// # Errors
///
/// Returns [`SearxngInstallError::InvalidSourceMetadata`] when the metadata cannot be validated,
/// or [`SearxngInstallError::IntegrityMismatch`] when the bytes do not match the pinned digest.
pub fn verify_searxng_archive(
    source: &SearxngSourceMetadata,
    archive: &[u8],
) -> Result<(), SearxngInstallError> {
    source.validate()?;
    let actual = digest_hex(archive);
    if actual.eq_ignore_ascii_case(&source.archive_sha256) {
        Ok(())
    } else {
        Err(SearxngInstallError::IntegrityMismatch {
            expected: source.archive_sha256.clone(),
            actual,
        })
    }
}

fn digest_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

/// Executes a previously built setup plan with caller-supplied archive bytes.
///
/// Source acquisition is intentionally not part of this function. Callers may use an isolated
/// downloader, a package cache, or a test fixture, but the installer always verifies the exact
/// bytes before creating its private root. Failed commands leave a marked staging directory for
/// [`recover_interrupted_searxng_install`] to reconcile on the next explicit attempt.
///
/// # Errors
///
/// Returns an integrity, ownership, filesystem, or command error. A failed command leaves marked
/// staging state for the next explicit recovery attempt.
pub fn execute_searxng_install_plan(
    plan: &SearxngSetupPlan,
    archive: &[u8],
    executor: &impl SearxngCommandExecutor,
) -> Result<SearxngInstallOutcome, SearxngInstallError> {
    let SearxngSetupPlan::Setup(plan) = plan else {
        return Ok(SearxngInstallOutcome::NoOp);
    };
    verify_searxng_archive(&plan.source, archive)?;
    ensure_owned_directory(plan.paths.root(), ROOT_OWNER_MARKER)?;
    recover_interrupted_searxng_install(&plan.paths)?;
    ensure_absent_or_owned(&plan.paths.staging())?;
    ensure_absent_or_owned(&plan.paths.previous())?;
    create_private_dir(&plan.paths.staging())
        .map_err(|source| io_error("create staging", plan.paths.staging(), source))?;
    write_private_marker(&plan.paths.staging(), INSTALL_OWNER_MARKER)?;
    create_private_dir(&plan.paths.source_in(&plan.paths.staging())).map_err(|source| {
        io_error(
            "create source staging",
            plan.paths.source_in(&plan.paths.staging()),
            source,
        )
    })?;
    write_install_metadata(plan, &plan.paths.staging())?;
    write_private_file(&plan.paths.staging().join(ARCHIVE_NAME), archive)?;

    for command in &plan.commands {
        executor.execute(command)?;
    }
    publish_staging(plan)?;
    Ok(SearxngInstallOutcome::Installed(SearxngInstallation {
        active_directory: plan.paths.active(),
        version: plan.source.version.clone(),
        platform: plan.platform,
        python_bootstrapped: plan.bootstrap_python,
    }))
}

fn publish_staging(plan: &SearxngInstallPlan) -> Result<(), SearxngInstallError> {
    let active = plan.paths.active();
    let previous = plan.paths.previous();
    let staging = plan.paths.staging();
    let mut moved_active = false;
    if path_exists(&active) {
        ensure_owned_installation(&active)?;
        fs::rename(&active, &previous)
            .map_err(|source| io_error("move active installation", previous.clone(), source))?;
        moved_active = true;
    }
    if let Err(source) = fs::rename(&staging, &active) {
        if moved_active {
            let _ = fs::rename(&previous, &active);
        }
        return Err(io_error("publish staged installation", active, source));
    }
    if moved_active {
        remove_owned_installation(&previous)?;
    }
    Ok(())
}

fn write_install_metadata(
    plan: &SearxngInstallPlan,
    installation: &Path,
) -> Result<(), SearxngInstallError> {
    let metadata = SearxngInstallMetadata {
        schema_version: INSTALL_SCHEMA_VERSION,
        owner: INSTALL_OWNER_MARKER.trim_end().to_owned(),
        platform: plan.platform.as_str().to_owned(),
        version: plan.source.version.clone(),
        revision: plan.source.revision.clone(),
        archive_url: plan.source.archive_url.clone(),
        archive_sha256: plan.source.archive_sha256.clone(),
        repository_url: plan.source.repository_url.clone(),
        license_url: plan.source.license_url.clone(),
        python_bootstrapped: plan.bootstrap_python,
    };
    let contents = serde_json::to_vec_pretty(&metadata)
        .map_err(|source| SearxngInstallError::SerializeMetadata(source.to_string()))?;
    write_private_file(&plan.paths.metadata_in(installation), &contents)
}

/// Versioned receipt persisted in the published installation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearxngInstallMetadata {
    /// Receipt schema version.
    pub schema_version: u8,
    /// Exact ownership marker value.
    pub owner: String,
    /// Explicit target platform.
    pub platform: String,
    /// Installed upstream version.
    pub version: String,
    /// Installed upstream revision.
    pub revision: String,
    /// Source archive URL.
    pub archive_url: String,
    /// Source archive SHA-256.
    pub archive_sha256: String,
    /// Upstream repository URL.
    pub repository_url: String,
    /// Upstream license URL.
    pub license_url: String,
    /// Whether Python/dependencies were bootstrapped.
    pub python_bootstrapped: bool,
}

/// Reads and validates the published installation receipt, if an active installation exists.
///
/// # Errors
///
/// Returns an ownership, filesystem, metadata parsing, or metadata validation error. An active
/// directory without a valid receipt is never treated as an installed `SearXNG` version.
pub fn read_searxng_install_metadata(
    paths: &SearxngInstallPaths,
) -> Result<Option<SearxngInstallMetadata>, SearxngInstallError> {
    let active = paths.active();
    if !path_exists(&active) {
        return Ok(None);
    }
    ensure_owned_installation(&active)?;
    let path = paths.metadata_in(&active);
    let contents = fs::read(&path)
        .map_err(|source| io_error("read SearXNG install metadata", path.clone(), source))?;
    let metadata: SearxngInstallMetadata = serde_json::from_slice(&contents)
        .map_err(|source| SearxngInstallError::ParseMetadata(source.to_string()))?;
    if metadata.schema_version != INSTALL_SCHEMA_VERSION {
        return Err(SearxngInstallError::UnsupportedMetadataSchema(
            metadata.schema_version,
        ));
    }
    if metadata.owner != INSTALL_OWNER_MARKER.trim_end() {
        return Err(SearxngInstallError::InvalidMetadata("owner"));
    }
    if parse_platform(&metadata.platform).is_none() {
        return Err(SearxngInstallError::InvalidMetadata("platform"));
    }
    SearxngSourceMetadata {
        version: metadata.version.clone(),
        revision: metadata.revision.clone(),
        archive_url: metadata.archive_url.clone(),
        archive_sha256: metadata.archive_sha256.clone(),
        repository_url: metadata.repository_url.clone(),
        license_url: metadata.license_url.clone(),
    }
    .validate()?;
    Ok(Some(metadata))
}

fn parse_platform(value: &str) -> Option<SearxngPlatform> {
    match value {
        "macos-x86_64" => Some(SearxngPlatform::MacOsX86_64),
        "macos-arm64" => Some(SearxngPlatform::MacOsArm64),
        "linux-x86_64" => Some(SearxngPlatform::LinuxX86_64),
        "linux-arm64" => Some(SearxngPlatform::LinuxArm64),
        _ => None,
    }
}

/// Reconciles marked staging and rollback directories after an interrupted explicit install.
///
/// Unknown directories and files are never removed. A staging directory without the exact
/// nan-harness ownership marker is left untouched, which prevents cleanup from claiming a user's
/// similarly named directory.
///
/// # Errors
///
/// Returns an ownership or filesystem error when marked state cannot be inspected or reconciled.
pub fn recover_interrupted_searxng_install(
    paths: &SearxngInstallPaths,
) -> Result<(), SearxngInstallError> {
    if !path_exists(paths.root()) {
        return Ok(());
    }
    ensure_owned_root(paths.root())?;
    let active = paths.active();
    let previous = paths.previous();
    if path_exists(&previous) {
        ensure_owned_installation(&previous)?;
        if !path_exists(&active) {
            fs::rename(&previous, &active).map_err(|source| {
                io_error("restore previous installation", active.clone(), source)
            })?;
        } else if is_owned_directory(&active)? {
            remove_owned_installation(&previous)?;
        }
    }
    let staging = paths.staging();
    if path_exists(&staging) && is_owned_directory(&staging)? {
        remove_owned_installation(&staging)?;
    }
    Ok(())
}

/// Removes only `SearXNG` directories carrying nan-harness ownership markers.
///
/// The root itself is retained when it contains unknown user files. This makes cleanup safe for
/// a root that was intentionally shared with another tool after installation.
///
/// # Errors
///
/// Returns an ownership or filesystem error when owned state cannot be inspected or removed.
pub fn cleanup_owned_searxng_install(
    paths: &SearxngInstallPaths,
) -> Result<(), SearxngInstallError> {
    if !path_exists(paths.root()) {
        return Ok(());
    }
    ensure_owned_root(paths.root())?;
    for path in [paths.active(), paths.staging(), paths.previous()] {
        if path_exists(&path) && is_owned_directory(&path)? {
            remove_owned_installation(&path)?;
        }
    }
    if fs::read_dir(paths.root())
        .map_err(|source| io_error("inspect SearXNG root", paths.root().to_path_buf(), source))?
        .next()
        .is_none()
    {
        fs::remove_dir(paths.root()).map_err(|source| {
            io_error(
                "remove empty SearXNG root",
                paths.root().to_path_buf(),
                source,
            )
        })?;
    }
    Ok(())
}

fn ensure_owned_root(path: &Path) -> Result<(), SearxngInstallError> {
    ensure_owned_directory(path, ROOT_OWNER_MARKER)
}

fn ensure_owned_installation(path: &Path) -> Result<(), SearxngInstallError> {
    if !is_owned_directory(path)? {
        return Err(SearxngInstallError::OwnershipConflict(path.to_path_buf()));
    }
    Ok(())
}

fn ensure_owned_directory(path: &Path, marker: &str) -> Result<(), SearxngInstallError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => {
            if !is_owned_directory_with_marker(path, marker)? {
                return Err(SearxngInstallError::OwnershipConflict(path.to_path_buf()));
            }
            Ok(())
        }
        Ok(_) => Err(SearxngInstallError::NotDirectory(path.to_path_buf())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let parent = path
                .parent()
                .ok_or_else(|| SearxngInstallError::InvalidPath(path.to_path_buf()))?;
            create_private_dir_all(parent).map_err(|source| {
                io_error("create SearXNG parent", parent.to_path_buf(), source)
            })?;
            create_private_dir(path).map_err(|source| {
                io_error("create SearXNG directory", path.to_path_buf(), source)
            })?;
            write_private_marker(path, marker)
        }
        Err(source) => Err(io_error(
            "inspect SearXNG directory",
            path.to_path_buf(),
            source,
        )),
    }
}

fn ensure_absent_or_owned(path: &Path) -> Result<(), SearxngInstallError> {
    if path_exists(path) {
        ensure_owned_installation(path)?;
        remove_owned_installation(path)?;
    }
    Ok(())
}

fn is_owned_directory(path: &Path) -> Result<bool, SearxngInstallError> {
    is_owned_directory_with_marker(path, INSTALL_OWNER_MARKER)
}

fn is_owned_directory_with_marker(
    path: &Path,
    expected_marker: &str,
) -> Result<bool, SearxngInstallError> {
    let marker = path.join(OWNER_MARKER);
    let directory_metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(source) => {
            return Err(io_error(
                "inspect SearXNG ownership directory",
                path.to_path_buf(),
                source,
            ));
        }
    };
    if !directory_metadata.is_dir() {
        return Ok(false);
    }
    let marker_metadata = match fs::symlink_metadata(&marker) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(source) => {
            return Err(io_error(
                "inspect SearXNG ownership marker",
                marker.clone(),
                source,
            ));
        }
    };
    if !marker_metadata.is_file() {
        return Ok(false);
    }
    match fs::read(&marker) {
        Ok(contents) => Ok(contents == expected_marker.as_bytes()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(io_error("read SearXNG ownership marker", marker, source)),
    }
}

fn write_private_marker(path: &Path, contents: &str) -> Result<(), SearxngInstallError> {
    write_private_file(&path.join(OWNER_MARKER), contents.as_bytes())
}

fn write_private_file(path: &Path, contents: &[u8]) -> Result<(), SearxngInstallError> {
    let mut file = open_private_new(path)
        .map_err(|source| io_error("create private SearXNG file", path.to_path_buf(), source))?;
    file.write_all(contents)
        .and_then(|()| file.sync_all())
        .map_err(|source| io_error("write private SearXNG file", path.to_path_buf(), source))
}

fn remove_owned_installation(path: &Path) -> Result<(), SearxngInstallError> {
    if !is_owned_directory(path)? {
        return Err(SearxngInstallError::OwnershipConflict(path.to_path_buf()));
    }
    fs::remove_dir_all(path).map_err(|source| {
        io_error(
            "remove owned SearXNG installation",
            path.to_path_buf(),
            source,
        )
    })
}

fn path_exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

fn io_error(operation: &'static str, path: PathBuf, source: io::Error) -> SearxngInstallError {
    SearxngInstallError::Io {
        operation,
        path,
        source,
    }
}

/// Errors from planning, integrity verification, and explicit `SearXNG` setup.
#[derive(Debug, Error)]
pub enum SearxngInstallError {
    #[error("SearXNG standalone installation is unsupported on this platform")]
    UnsupportedPlatform,
    #[error("SearXNG source metadata is invalid: {0}")]
    InvalidSourceMetadata(&'static str),
    #[error("SearXNG archive integrity mismatch (expected {expected}, got {actual})")]
    IntegrityMismatch { expected: String, actual: String },
    #[error("SearXNG path is not a directory: {}", .0.display())]
    NotDirectory(PathBuf),
    #[error("SearXNG path is invalid: {}", .0.display())]
    InvalidPath(PathBuf),
    #[error("SearXNG path is not owned by nan-harness: {}", .0.display())]
    OwnershipConflict(PathBuf),
    #[error("could not {operation} '{}': {source}", path.display())]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not serialize SearXNG install metadata: {0}")]
    SerializeMetadata(String),
    #[error("could not parse SearXNG install metadata: {0}")]
    ParseMetadata(String),
    #[error("SearXNG install metadata schema {0} is unsupported")]
    UnsupportedMetadataSchema(u8),
    #[error("SearXNG install metadata field is invalid: {0}")]
    InvalidMetadata(&'static str),
    #[error("could not start SearXNG setup command '{program}': {source}")]
    CommandStart {
        program: String,
        #[source]
        source: io::Error,
    },
    #[error("SearXNG setup command '{program}' exited with status {status:?}")]
    CommandFailed {
        program: String,
        status: Option<i32>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn test_source(bytes: &[u8]) -> SearxngSourceMetadata {
        let digest = digest_hex(bytes);
        SearxngSourceMetadata {
            version: "2026.1.1+fixture".to_owned(),
            revision: "fixture-revision".to_owned(),
            archive_url: "https://example.test/searxng.tar.gz".to_owned(),
            archive_sha256: digest,
            repository_url: "https://example.test/searxng".to_owned(),
            license_url: "https://example.test/searxng/LICENSE".to_owned(),
        }
    }

    fn setup_plan(
        root: &Path,
        request: SearxngInstallRequest,
        source: SearxngSourceMetadata,
    ) -> SearxngSetupPlan {
        plan_searxng_installation(
            request,
            Some(SearxngPlatform::LinuxX86_64),
            SearxngInstallPaths::under(root),
            source,
        )
        .expect("fixture plan should build")
    }

    #[test]
    fn unconfigured_requests_are_no_op_even_with_invalid_platform_and_metadata() {
        let result = plan_searxng_installation(
            SearxngInstallRequest::unconfigured(),
            None,
            SearxngInstallPaths::under("/definitely/not-created"),
            SearxngSourceMetadata {
                version: String::new(),
                revision: String::new(),
                archive_url: String::new(),
                archive_sha256: String::new(),
                repository_url: String::new(),
                license_url: String::new(),
            },
        )
        .expect("unconfigured plan should not validate setup inputs");
        assert_eq!(result, SearxngSetupPlan::NoOp);
    }

    #[test]
    fn interrupted_setup_removes_only_marked_staging_directory() {
        let root = tempfile::tempdir().expect("temporary root should exist");
        let paths = SearxngInstallPaths::under(root.path().join("searxng"));
        ensure_owned_root(paths.root()).expect("root should be owned");
        create_private_dir(&paths.staging()).expect("staging should exist");
        write_private_marker(&paths.staging(), INSTALL_OWNER_MARKER)
            .expect("staging marker should exist");
        let foreign = paths.root().join("foreign-staging");
        fs::create_dir(&foreign).expect("foreign directory should exist");

        recover_interrupted_searxng_install(&paths).expect("recovery should succeed");

        assert!(!paths.staging().exists());
        assert!(foreign.exists());
    }

    #[test]
    fn archive_integrity_mismatch_fails_before_creating_state() {
        let source = test_source(b"known source");
        let root = tempfile::tempdir().expect("temporary root should exist");
        let plan = setup_plan(
            &root.path().join("searxng"),
            SearxngInstallRequest::source_only(),
            source,
        );
        let executor = RecordingExecutor::default();

        let error = execute_searxng_install_plan(&plan, b"tampered source", &executor)
            .expect_err("tampered archive should be rejected");

        assert!(matches!(
            error,
            SearxngInstallError::IntegrityMismatch { .. }
        ));
        assert_eq!(executor.commands.lock().expect("lock").len(), 0);
        assert!(!root.path().join("searxng").exists());
    }

    #[test]
    fn failed_setup_is_recoverable_without_publishing_partial_install() {
        let archive = b"known source";
        let root = tempfile::tempdir().expect("temporary root should exist");
        let plan = setup_plan(
            &root.path().join("searxng"),
            SearxngInstallRequest::source_only(),
            test_source(archive),
        );
        let executor = FailingExecutor;

        let error = execute_searxng_install_plan(&plan, archive, &executor)
            .expect_err("failed command should interrupt setup");

        assert!(matches!(error, SearxngInstallError::CommandFailed { .. }));
        let SearxngSetupPlan::Setup(plan) = &plan else {
            panic!("fixture plan should produce setup");
        };
        assert!(plan.paths().staging().exists());
        assert!(!plan.paths().active().exists());
        recover_interrupted_searxng_install(plan.paths()).expect("recovery should succeed");
        assert!(!plan.paths().staging().exists());
    }

    #[test]
    fn cleanup_preserves_unowned_paths_and_removes_owned_installation() {
        let root = tempfile::tempdir().expect("temporary root should exist");
        let paths = SearxngInstallPaths::under(root.path().join("searxng"));
        ensure_owned_root(paths.root()).expect("root should be owned");
        create_private_dir(&paths.active()).expect("active should exist");
        write_private_marker(&paths.active(), INSTALL_OWNER_MARKER).expect("active marker");
        let foreign = paths.root().join("foreign");
        fs::create_dir(&foreign).expect("foreign path should exist");

        cleanup_owned_searxng_install(&paths).expect("cleanup should succeed");

        assert!(!paths.active().exists());
        assert!(foreign.exists());
        assert!(paths.root().exists());
    }

    #[test]
    fn platform_mapping_is_explicit_for_all_supported_targets() {
        assert_eq!(
            SearxngPlatform::from_target_triple("x86_64-apple-darwin"),
            Some(SearxngPlatform::MacOsX86_64)
        );
        assert_eq!(
            SearxngPlatform::from_target_triple("aarch64-apple-darwin"),
            Some(SearxngPlatform::MacOsArm64)
        );
        assert_eq!(
            SearxngPlatform::from_target_triple("x86_64-unknown-linux-gnu"),
            Some(SearxngPlatform::LinuxX86_64)
        );
        assert_eq!(
            SearxngPlatform::from_target_triple("aarch64-unknown-linux-musl"),
            Some(SearxngPlatform::LinuxArm64)
        );
        assert_eq!(
            SearxngPlatform::from_target_triple("x86_64-pc-windows-msvc"),
            None
        );
    }

    #[test]
    fn python_bootstrap_is_present_only_when_explicitly_requested() {
        let root = PathBuf::from("/fixture");
        let source = test_source(b"source");
        let source_only = setup_plan(&root, SearxngInstallRequest::source_only(), source.clone());
        let bootstrap = setup_plan(
            &root,
            SearxngInstallRequest::with_python_bootstrap(),
            source,
        );
        let SearxngSetupPlan::Setup(source_only) = source_only else {
            panic!("source-only request should produce setup");
        };
        let SearxngSetupPlan::Setup(bootstrap) = bootstrap else {
            panic!("bootstrap request should produce setup");
        };
        assert_eq!(source_only.commands().len(), 1);
        assert!(bootstrap.commands().len() > source_only.commands().len());
        assert!(!source_only.bootstraps_python());
        assert!(bootstrap.bootstraps_python());
    }

    #[test]
    fn published_receipt_preserves_version_and_integrity_metadata() {
        let archive = b"known source";
        let root = tempfile::tempdir().expect("temporary root should exist");
        let plan = setup_plan(
            &root.path().join("searxng"),
            SearxngInstallRequest::source_only(),
            test_source(archive),
        );
        let executor = RecordingExecutor::default();

        execute_searxng_install_plan(&plan, archive, &executor)
            .expect("fixture setup should publish");
        let SearxngSetupPlan::Setup(plan) = &plan else {
            panic!("fixture plan should produce setup");
        };
        let metadata = read_searxng_install_metadata(plan.paths())
            .expect("receipt should be readable")
            .expect("receipt should exist");

        assert_eq!(metadata.version, "2026.1.1+fixture");
        assert_eq!(metadata.platform, "linux-x86_64");
        assert_eq!(metadata.archive_sha256, digest_hex(archive));
        assert!(!metadata.python_bootstrapped);
    }

    #[derive(Default)]
    struct RecordingExecutor {
        commands: Arc<Mutex<Vec<SearxngCommand>>>,
    }

    impl SearxngCommandExecutor for RecordingExecutor {
        fn execute(&self, command: &SearxngCommand) -> Result<(), SearxngInstallError> {
            self.commands.lock().expect("lock").push(command.clone());
            Ok(())
        }
    }

    struct FailingExecutor;

    impl SearxngCommandExecutor for FailingExecutor {
        fn execute(&self, command: &SearxngCommand) -> Result<(), SearxngInstallError> {
            Err(SearxngInstallError::CommandFailed {
                program: command.program.display().to_string(),
                status: Some(1),
            })
        }
    }
}
