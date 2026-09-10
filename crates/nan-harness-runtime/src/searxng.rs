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

/// Windows x64 installation metadata and ownership primitives.
///
/// This module intentionally remains a contract-only layer: it does not download source, install
/// Python, register startup tasks, or start a process. A future Windows supervisor can consume the
/// layout, receipt, and direct command while retaining explicit process ownership.
pub mod windows {
    use nan_harness_private_fs::{
        PrivatePathKind, create_private_dir_all, open_private_new, open_private_read,
        open_private_read_write, restrict_path,
    };
    use serde::{Deserialize, Serialize};
    use sha2::{Digest as _, Sha256};
    use std::fs::{self, File, TryLockError};
    use std::io::{ErrorKind, Read as _, Write as _};
    use std::path::{Path, PathBuf};
    use thiserror::Error;

    pub const WINDOWS_X64_TARGET: &str = "x86_64-pc-windows-msvc";
    pub const SEARXNG_SOURCE_REPOSITORY: &str = "https://github.com/searxng/searxng";
    pub const SEARXNG_LISTEN_HOST: &str = "127.0.0.1";
    pub const SEARXNG_LISTEN_PORT: u16 = 8888;

    const RECEIPT_SCHEMA_VERSION: u8 = 1;
    const RECEIPT_FILE_NAME: &str = "install-receipt.json";
    const INSTALL_LOCK_FILE_NAME: &str = ".install.lock";
    const STAGING_DIRECTORY_NAME: &str = ".staging";
    const STAGING_MARKER_FILE_NAME: &str = ".nan-harness-searxng-staging";
    const STAGING_MARKER: &[u8] = b"nan-harness searxng staging v1\n";

    #[must_use]
    pub fn pinned_windows_x64_release() -> SearxngRelease {
        SearxngRelease {
            target: WINDOWS_X64_TARGET.to_owned(),
            version: "2026.9.8".to_owned(),
            commit: "3fdc6d753".to_owned(),
            source_url: "https://github.com/searxng/searxng/archive/3fdc6d753.tar.gz".to_owned(),
            source_sha256: "99656d7b2b72b97c716b0d43f83536183dc8cec94aeb6dbda604c5d4f0b672e0"
                .to_owned(),
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    pub struct SearxngRelease {
        pub target: String,
        pub version: String,
        pub commit: String,
        pub source_url: String,
        pub source_sha256: String,
    }

    impl SearxngRelease {
        /// Validates that release metadata is an HTTPS `SearXNG` source record.
        ///
        /// # Errors
        ///
        /// Returns an error when the target, source URL, or source digest is unsupported.
        pub fn validate(&self) -> Result<(), SearxngError> {
            if self.target != WINDOWS_X64_TARGET {
                return Err(SearxngError::UnsupportedTarget(self.target.clone()));
            }
            if self.version.trim().is_empty() || self.commit.trim().is_empty() {
                return Err(SearxngError::InvalidRelease(
                    "version and commit are required".to_owned(),
                ));
            }
            let valid_digest = self.source_sha256.len() == 64
                && self
                    .source_sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase());
            if !valid_digest {
                return Err(SearxngError::InvalidRelease(
                    "sourceSha256 must be 64 lowercase hexadecimal characters".to_owned(),
                ));
            }
            if !self
                .source_url
                .starts_with("https://github.com/searxng/searxng/")
            {
                return Err(SearxngError::InvalidRelease(
                    "sourceUrl must be an HTTPS URL from the SearXNG repository".to_owned(),
                ));
            }
            Ok(())
        }

        /// Verifies source bytes against the pinned digest.
        ///
        /// # Errors
        ///
        /// Returns an error when the release metadata is invalid or the bytes do not match.
        pub fn verify_source(&self, source: &[u8]) -> Result<(), SearxngError> {
            self.validate()?;
            let actual = hex_digest(source);
            if actual == self.source_sha256 {
                Ok(())
            } else {
                Err(SearxngError::SourceDigestMismatch {
                    expected: self.source_sha256.clone(),
                    actual,
                })
            }
        }
    }

    fn hex_digest(bytes: &[u8]) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut digest = String::with_capacity(64);
        for byte in Sha256::digest(bytes) {
            digest.push(char::from(HEX[usize::from(byte >> 4)]));
            digest.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
        digest
    }

    /// Private filesystem layout for one user-owned Windows installation.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct SearxngWindowsLayout {
        root: PathBuf,
    }

    impl SearxngWindowsLayout {
        /// Creates a layout from an absolute installation root.
        ///
        /// # Errors
        ///
        /// Returns an error when `root` is not absolute.
        pub fn new(root: impl Into<PathBuf>) -> Result<Self, SearxngError> {
            let root = root.into();
            if !root.is_absolute() {
                return Err(SearxngError::InvalidRoot(root));
            }
            Ok(Self { root })
        }

        #[must_use]
        pub fn root(&self) -> &Path {
            &self.root
        }

        #[must_use]
        pub fn source_directory(&self) -> PathBuf {
            self.root.join("searxng-src")
        }

        #[must_use]
        pub fn virtual_environment_directory(&self) -> PathBuf {
            self.root.join("searx-pyenv")
        }

        #[must_use]
        pub fn python_executable(&self) -> PathBuf {
            self.virtual_environment_directory()
                .join("Scripts")
                .join("python.exe")
        }

        #[must_use]
        pub fn configuration_directory(&self) -> PathBuf {
            self.root.join("config")
        }

        #[must_use]
        pub fn settings_path(&self) -> PathBuf {
            self.configuration_directory().join("settings.yml")
        }

        #[must_use]
        pub fn state_directory(&self) -> PathBuf {
            self.root.join("state")
        }

        #[must_use]
        pub fn log_directory(&self) -> PathBuf {
            self.root.join("logs")
        }

        #[must_use]
        pub fn receipt_path(&self) -> PathBuf {
            self.root.join(RECEIPT_FILE_NAME)
        }

        #[must_use]
        pub fn install_lock_path(&self) -> PathBuf {
            self.root.join(INSTALL_LOCK_FILE_NAME)
        }

        #[must_use]
        pub fn staging_directory(&self) -> PathBuf {
            self.root.join(STAGING_DIRECTORY_NAME)
        }

        #[must_use]
        pub fn default_root(local_app_data: &Path) -> PathBuf {
            local_app_data.join("nan-harness").join("searxng")
        }

        /// Creates and hardens every directory owned by this recipe.
        ///
        /// # Errors
        ///
        /// Returns an error when a directory cannot be created or hardened privately.
        pub fn ensure_private(&self) -> Result<(), SearxngError> {
            for directory in [
                self.root.clone(),
                self.source_directory(),
                self.virtual_environment_directory(),
                self.configuration_directory(),
                self.state_directory(),
                self.log_directory(),
            ] {
                create_private_dir_all(&directory).map_err(|source| SearxngError::Filesystem {
                    operation: "create private directory",
                    path: directory.clone(),
                    source,
                })?;
                restrict_path(&directory, PrivatePathKind::Directory).map_err(|source| {
                    SearxngError::Filesystem {
                        operation: "harden private directory",
                        path: directory,
                        source,
                    }
                })?;
            }
            Ok(())
        }
    }

    /// A Windows x64 standalone `SearXNG` recipe.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct SearxngWindowsRecipe {
        layout: SearxngWindowsLayout,
        release: SearxngRelease,
    }

    impl SearxngWindowsRecipe {
        /// Creates a recipe without changing the filesystem or starting anything.
        ///
        /// # Errors
        ///
        /// Returns an error when `release` does not describe the supported Windows x64 source.
        pub fn new(
            layout: SearxngWindowsLayout,
            release: SearxngRelease,
        ) -> Result<Self, SearxngError> {
            release.validate()?;
            Ok(Self { layout, release })
        }

        /// Creates a recipe using the pinned `SearXNG` source record.
        ///
        /// # Errors
        ///
        /// Returns an error if the pinned source record is invalid.
        pub fn pinned(layout: SearxngWindowsLayout) -> Result<Self, SearxngError> {
            Self::new(layout, pinned_windows_x64_release())
        }

        #[must_use]
        pub fn layout(&self) -> &SearxngWindowsLayout {
            &self.layout
        }

        #[must_use]
        pub fn release(&self) -> &SearxngRelease {
            &self.release
        }

        #[must_use]
        pub const fn startup_policy(&self) -> StartupPolicy {
            StartupPolicy::ExplicitOnly
        }

        /// Acquires the lock shared by installation and interrupted cleanup.
        ///
        /// # Errors
        ///
        /// Returns an error when the installation cannot be hardened or another owner holds it.
        pub fn acquire_install_lock(&self) -> Result<SearxngInstallLock, SearxngError> {
            self.layout.ensure_private()?;
            let path = self.layout.install_lock_path();
            let file =
                open_private_read_write(&path).map_err(|source| SearxngError::Filesystem {
                    operation: "open installation lock",
                    path: path.clone(),
                    source,
                })?;
            match file.try_lock() {
                Ok(()) => Ok(SearxngInstallLock { file }),
                Err(TryLockError::WouldBlock) => Err(SearxngError::InstallationBusy),
                Err(TryLockError::Error(source)) => Err(SearxngError::Filesystem {
                    operation: "lock installation",
                    path,
                    source,
                }),
            }
        }

        /// Creates marker-owned staging state. Hold the install lock until publication completes.
        ///
        /// # Errors
        ///
        /// Returns an error when the staging path exists or private setup fails.
        pub fn create_staging(&self) -> Result<SearxngStaging, SearxngError> {
            self.layout.ensure_private()?;
            let staging = self.layout.staging_directory();
            if fs::symlink_metadata(&staging).is_ok() {
                return Err(SearxngError::StagingExists(staging));
            }
            fs::create_dir(&staging).map_err(|source| SearxngError::Filesystem {
                operation: "create staging directory",
                path: staging.clone(),
                source,
            })?;
            if let Err(source) = restrict_path(&staging, PrivatePathKind::Directory) {
                let _ = fs::remove_dir(&staging);
                return Err(SearxngError::Filesystem {
                    operation: "harden staging directory",
                    path: staging,
                    source,
                });
            }
            let marker = staging.join(STAGING_MARKER_FILE_NAME);
            let mut marker_file = match open_private_new(&marker) {
                Ok(file) => file,
                Err(source) => {
                    let _ = fs::remove_dir(&staging);
                    return Err(SearxngError::Filesystem {
                        operation: "create staging ownership marker",
                        path: marker,
                        source,
                    });
                }
            };
            if let Err(source) = marker_file
                .write_all(STAGING_MARKER)
                .and_then(|()| marker_file.sync_data())
            {
                drop(marker_file);
                let _ = fs::remove_dir_all(&staging);
                return Err(SearxngError::Filesystem {
                    operation: "write staging ownership marker",
                    path: marker,
                    source,
                });
            }
            Ok(SearxngStaging { path: staging })
        }

        /// Removes interrupted staging only when its exact ownership marker is present.
        ///
        /// # Errors
        ///
        /// Returns an error when staging metadata cannot be inspected or owned state cannot be
        /// removed.
        pub fn cleanup_interrupted(&self) -> Result<CleanupOutcome, SearxngError> {
            let staging = self.layout.staging_directory();
            let metadata = match fs::symlink_metadata(&staging) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == ErrorKind::NotFound => {
                    return Ok(CleanupOutcome::NothingToDo);
                }
                Err(source) => {
                    return Err(SearxngError::Filesystem {
                        operation: "inspect staging directory",
                        path: staging,
                        source,
                    });
                }
            };
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(SearxngError::UnsafeStaging(staging));
            }
            let marker = staging.join(STAGING_MARKER_FILE_NAME);
            let marker_metadata = match fs::symlink_metadata(&marker) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == ErrorKind::NotFound => {
                    return Ok(CleanupOutcome::PreservedForeign);
                }
                Err(source) => {
                    return Err(SearxngError::Filesystem {
                        operation: "inspect staging ownership marker",
                        path: marker,
                        source,
                    });
                }
            };
            if marker_metadata.file_type().is_symlink() || !marker_metadata.is_file() {
                return Ok(CleanupOutcome::PreservedForeign);
            }
            let (mut file, _) =
                open_private_read(&marker).map_err(|source| SearxngError::Filesystem {
                    operation: "read staging ownership marker",
                    path: marker,
                    source,
                })?;
            let mut contents = Vec::new();
            file.read_to_end(&mut contents)
                .map_err(|source| SearxngError::Filesystem {
                    operation: "read staging ownership marker",
                    path: staging.clone(),
                    source,
                })?;
            if contents != STAGING_MARKER {
                return Ok(CleanupOutcome::PreservedForeign);
            }
            fs::remove_dir_all(&staging).map_err(|source| SearxngError::Filesystem {
                operation: "remove interrupted staging directory",
                path: staging,
                source,
            })?;
            Ok(CleanupOutcome::RemovedOwnedStaging)
        }

        /// Builds a direct `python.exe -m searx.webapp` command without a shell.
        #[must_use]
        pub fn command(&self, python: &Path) -> SearxngCommand {
            SearxngCommand {
                executable: python.to_path_buf(),
                arguments: vec!["-m".to_owned(), "searx.webapp".to_owned()],
                working_directory: self.layout.source_directory(),
                settings_path: self.layout.settings_path(),
                listen_host: SEARXNG_LISTEN_HOST.to_owned(),
                listen_port: SEARXNG_LISTEN_PORT,
            }
        }

        /// Writes a private, atomically published installation receipt.
        ///
        /// # Errors
        ///
        /// Returns an error when the receipt cannot be serialized, written, or hardened.
        pub fn write_receipt(
            &self,
            selection: &WindowsPythonSelection,
        ) -> Result<SearxngInstallReceipt, SearxngError> {
            self.layout.ensure_private()?;
            let receipt = SearxngInstallReceipt {
                schema_version: RECEIPT_SCHEMA_VERSION,
                target: WINDOWS_X64_TARGET.to_owned(),
                release: self.release.clone(),
                python: selection.clone(),
            };
            let payload = serde_json::to_vec_pretty(&receipt)
                .map_err(|source| SearxngError::SerializeReceipt(source.to_string()))?;
            let receipt_path = self.layout.receipt_path();
            let parent = receipt_path
                .parent()
                .ok_or_else(|| SearxngError::InvalidRoot(self.layout.root().to_path_buf()))?;
            let mut temporary = tempfile::Builder::new()
                .prefix(".receipt-")
                .tempfile_in(parent)
                .map_err(|source| SearxngError::Filesystem {
                    operation: "create receipt staging file",
                    path: parent.to_path_buf(),
                    source,
                })?;
            let temporary_path = temporary.path().to_path_buf();
            // `tempfile` owns the handle, while `restrict_path` can harden its named file on
            // Windows without requiring the default temporary handle to request `WRITE_DAC`.
            restrict_path(&temporary_path, PrivatePathKind::File).map_err(|source| {
                SearxngError::Filesystem {
                    operation: "harden receipt staging file",
                    path: temporary_path.clone(),
                    source,
                }
            })?;
            temporary
                .write_all(&payload)
                .and_then(|()| temporary.write_all(b"\n"))
                .and_then(|()| temporary.flush())
                .and_then(|()| temporary.as_file().sync_all())
                .map_err(|source| SearxngError::Filesystem {
                    operation: "write installation receipt",
                    path: temporary_path.clone(),
                    source,
                })?;
            temporary
                .persist(&receipt_path)
                .map_err(|error| SearxngError::Filesystem {
                    operation: "publish installation receipt",
                    path: receipt_path.clone(),
                    source: error.error,
                })?;
            restrict_path(&receipt_path, PrivatePathKind::File).map_err(|source| {
                SearxngError::Filesystem {
                    operation: "harden installation receipt",
                    path: receipt_path,
                    source,
                }
            })?;
            Ok(receipt)
        }

        /// Reads and validates the private installation receipt, if present.
        ///
        /// # Errors
        ///
        /// Returns an error when the receipt cannot be read, parsed, validated, or matched.
        pub fn read_receipt(&self) -> Result<Option<SearxngInstallReceipt>, SearxngError> {
            let path = self.layout.receipt_path();
            let (mut file, _) = match open_private_read(&path) {
                Ok(result) => result,
                Err(source) if source.kind() == ErrorKind::NotFound => return Ok(None),
                Err(source) => {
                    return Err(SearxngError::Filesystem {
                        operation: "open installation receipt",
                        path,
                        source,
                    });
                }
            };
            let mut payload = Vec::new();
            file.read_to_end(&mut payload)
                .map_err(|source| SearxngError::Filesystem {
                    operation: "read installation receipt",
                    path: self.layout.receipt_path(),
                    source,
                })?;
            let receipt: SearxngInstallReceipt = serde_json::from_slice(&payload)
                .map_err(|source| SearxngError::ParseReceipt(source.to_string()))?;
            receipt.validate()?;
            if receipt.release != self.release {
                return Err(SearxngError::InvalidRelease(
                    "installation receipt does not match the selected source record".to_owned(),
                ));
            }
            Ok(Some(receipt))
        }
    }

    #[derive(Debug)]
    pub struct SearxngInstallLock {
        file: File,
    }

    impl Drop for SearxngInstallLock {
        fn drop(&mut self) {
            let _ = self.file.unlock();
        }
    }

    #[derive(Debug)]
    pub struct SearxngStaging {
        path: PathBuf,
    }

    impl SearxngStaging {
        #[must_use]
        pub fn path(&self) -> &Path {
            &self.path
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum CleanupOutcome {
        NothingToDo,
        RemovedOwnedStaging,
        PreservedForeign,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum StartupPolicy {
        ExplicitOnly,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    pub struct WindowsPythonSelection {
        pub executable: PathBuf,
        pub source: WindowsPythonSource,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    pub enum WindowsPythonSource {
        ExplicitOverride,
        VirtualEnvironment,
        Path,
        PythonLauncher,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct WindowsPythonCandidates {
        explicit: Option<PathBuf>,
        virtual_environment: PathBuf,
        path_directories: Vec<PathBuf>,
        launcher: Option<PathBuf>,
    }

    impl WindowsPythonCandidates {
        #[must_use]
        pub fn new(
            explicit: Option<PathBuf>,
            virtual_environment: PathBuf,
            path_directories: Vec<PathBuf>,
            launcher: Option<PathBuf>,
        ) -> Self {
            Self {
                explicit,
                virtual_environment,
                path_directories,
                launcher,
            }
        }

        #[must_use]
        pub fn paths(&self) -> Vec<(PathBuf, WindowsPythonSource)> {
            self.explicit
                .iter()
                .cloned()
                .map(|path| (path, WindowsPythonSource::ExplicitOverride))
                .chain(std::iter::once((
                    self.virtual_environment.join("Scripts").join("python.exe"),
                    WindowsPythonSource::VirtualEnvironment,
                )))
                .chain(
                    self.path_directories
                        .iter()
                        .map(|directory| (directory.join("python.exe"), WindowsPythonSource::Path)),
                )
                .chain(
                    self.launcher
                        .iter()
                        .cloned()
                        .map(|path| (path, WindowsPythonSource::PythonLauncher)),
                )
                .collect()
        }

        pub fn select(&self, is_usable: impl Fn(&Path) -> bool) -> Option<WindowsPythonSelection> {
            self.paths()
                .into_iter()
                .find(|(path, _)| is_usable(path))
                .map(|(executable, source)| WindowsPythonSelection { executable, source })
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct SearxngCommand {
        pub executable: PathBuf,
        pub arguments: Vec<String>,
        pub working_directory: PathBuf,
        pub settings_path: PathBuf,
        pub listen_host: String,
        pub listen_port: u16,
    }

    impl SearxngCommand {
        /// Applies the command to Tokio without invoking a shell.
        pub fn configure(&self, command: &mut tokio::process::Command) {
            command
                .args(&self.arguments)
                .current_dir(&self.working_directory)
                .env("SEARXNG_SETTINGS_PATH", &self.settings_path)
                .env("SEARXNG_BIND_ADDRESS", &self.listen_host)
                .env("SEARXNG_PORT", self.listen_port.to_string());
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    pub struct SearxngInstallReceipt {
        pub schema_version: u8,
        pub target: String,
        pub release: SearxngRelease,
        pub python: WindowsPythonSelection,
    }

    impl SearxngInstallReceipt {
        fn validate(&self) -> Result<(), SearxngError> {
            if self.schema_version != RECEIPT_SCHEMA_VERSION {
                return Err(SearxngError::UnsupportedReceiptSchema(self.schema_version));
            }
            if self.target != WINDOWS_X64_TARGET {
                return Err(SearxngError::UnsupportedTarget(self.target.clone()));
            }
            self.release.validate()
        }
    }

    #[derive(Debug, Error)]
    pub enum SearxngError {
        #[error("SearXNG Windows recipe requires an absolute installation root: '{0}'")]
        InvalidRoot(PathBuf),
        #[error("SearXNG recipe does not support target '{0}'")]
        UnsupportedTarget(String),
        #[error("SearXNG release metadata is invalid: {0}")]
        InvalidRelease(String),
        #[error("SearXNG source digest mismatch (expected {expected}, got {actual})")]
        SourceDigestMismatch { expected: String, actual: String },
        #[error("SearXNG installation is already locked by another process")]
        InstallationBusy,
        #[error("SearXNG staging directory already exists: '{0}'")]
        StagingExists(PathBuf),
        #[error("SearXNG staging path is not a private directory: '{0}'")]
        UnsafeStaging(PathBuf),
        #[error("could not {operation} '{}': {source}", path.display())]
        Filesystem {
            operation: &'static str,
            path: PathBuf,
            #[source]
            source: std::io::Error,
        },
        #[error("could not serialize SearXNG installation receipt: {0}")]
        SerializeReceipt(String),
        #[error("could not parse SearXNG installation receipt: {0}")]
        ParseReceipt(String),
        #[error("SearXNG installation receipt uses unsupported schema version {0}")]
        UnsupportedReceiptSchema(u8),
    }

    #[cfg(windows)]
    /// Resolves the conventional per-user Windows installation root.
    ///
    /// # Errors
    ///
    /// Returns an error when neither `LOCALAPPDATA` nor a usable `USERPROFILE` fallback exists.
    pub fn default_windows_layout() -> Result<SearxngWindowsLayout, SearxngError> {
        let local_app_data = std::env::var_os("LOCALAPPDATA")
            .or_else(|| {
                std::env::var_os("USERPROFILE")
                    .map(|home| PathBuf::from(home).join("AppData/Local"))
            })
            .ok_or_else(|| SearxngError::InvalidRoot(PathBuf::from("<LOCALAPPDATA>")))?;
        SearxngWindowsLayout::new(SearxngWindowsLayout::default_root(&local_app_data))
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::fs;

        #[test]
        fn layout_is_private_and_deterministic() {
            let root = tempfile::tempdir().expect("temporary root should exist");
            let layout = SearxngWindowsLayout::new(root.path().join("install"))
                .expect("fixture root should be absolute");
            layout.ensure_private().expect("layout should be private");
            assert_eq!(layout.source_directory(), layout.root().join("searxng-src"));
            assert_eq!(
                layout.python_executable(),
                layout.root().join("searx-pyenv/Scripts/python.exe")
            );
            assert_eq!(
                layout.settings_path(),
                layout.root().join("config/settings.yml")
            );
            assert!(layout.log_directory().is_dir());
        }

        #[test]
        fn candidate_order_prefers_explicit_then_environment_then_path_then_launcher() {
            let candidates = WindowsPythonCandidates::new(
                Some(PathBuf::from("/tools/python.exe")),
                PathBuf::from("/install/searx-pyenv"),
                vec![PathBuf::from("/Python")],
                Some(PathBuf::from("/Windows/py.exe")),
            );
            let paths = candidates.paths();
            assert_eq!(paths[0].1, WindowsPythonSource::ExplicitOverride);
            assert_eq!(paths[1].1, WindowsPythonSource::VirtualEnvironment);
            assert_eq!(paths[2].1, WindowsPythonSource::Path);
            assert_eq!(paths[3].1, WindowsPythonSource::PythonLauncher);
            let selection = candidates
                .select(|path| path == Path::new("/Python/python.exe"))
                .expect("usable candidate should be selected");
            assert_eq!(selection.source, WindowsPythonSource::Path);
        }

        #[test]
        fn command_is_direct_and_receipt_round_trip_preserves_integrity() {
            let root = tempfile::tempdir().expect("temporary root should exist");
            let layout = SearxngWindowsLayout::new(root.path().join("install"))
                .expect("fixture root should be absolute");
            let recipe =
                SearxngWindowsRecipe::pinned(layout).expect("pinned release should validate");
            let command = recipe.command(Path::new("C:/Python/python.exe"));
            assert_eq!(command.arguments, ["-m", "searx.webapp"]);
            assert_eq!(
                command.working_directory,
                recipe.layout().source_directory()
            );
            assert_eq!(command.listen_host, SEARXNG_LISTEN_HOST);
            let selection = WindowsPythonSelection {
                executable: PathBuf::from("C:/Python/python.exe"),
                source: WindowsPythonSource::ExplicitOverride,
            };
            let expected = recipe
                .write_receipt(&selection)
                .expect("receipt should write");
            assert_eq!(
                recipe.read_receipt().expect("receipt should read"),
                Some(expected)
            );
        }

        #[test]
        fn recovery_removes_only_exact_marker_owned_staging() {
            let root = tempfile::tempdir().expect("temporary root should exist");
            let layout = SearxngWindowsLayout::new(root.path().join("install"))
                .expect("fixture root should be absolute");
            let recipe =
                SearxngWindowsRecipe::pinned(layout).expect("pinned release should validate");
            let lock = recipe
                .acquire_install_lock()
                .expect("lock should be acquired");
            let staging = recipe.create_staging().expect("staging should be created");
            fs::write(staging.path().join("partial.tar.gz"), b"partial")
                .expect("partial payload should be written");
            drop(staging);
            assert_eq!(
                recipe
                    .cleanup_interrupted()
                    .expect("cleanup should succeed"),
                CleanupOutcome::RemovedOwnedStaging
            );
            assert_eq!(
                recipe
                    .cleanup_interrupted()
                    .expect("retry should be harmless"),
                CleanupOutcome::NothingToDo
            );
            drop(lock);
        }

        #[test]
        fn installation_lock_rejects_a_second_owner_until_drop() {
            let root = tempfile::tempdir().expect("temporary root should exist");
            let layout = SearxngWindowsLayout::new(root.path().join("install"))
                .expect("fixture root should be absolute");
            let recipe =
                SearxngWindowsRecipe::pinned(layout).expect("pinned release should validate");
            let first = recipe
                .acquire_install_lock()
                .expect("first owner should acquire lock");
            assert!(matches!(
                recipe.acquire_install_lock(),
                Err(SearxngError::InstallationBusy)
            ));
            drop(first);
            recipe
                .acquire_install_lock()
                .expect("lock should be reusable after drop");
        }

        #[test]
        fn recovery_preserves_foreign_staging() {
            let root = tempfile::tempdir().expect("temporary root should exist");
            let layout = SearxngWindowsLayout::new(root.path().join("install"))
                .expect("fixture root should be absolute");
            let recipe =
                SearxngWindowsRecipe::pinned(layout).expect("pinned release should validate");
            recipe
                .layout()
                .ensure_private()
                .expect("layout should be private");
            fs::create_dir(recipe.layout().staging_directory())
                .expect("foreign staging should exist");
            assert_eq!(
                recipe
                    .cleanup_interrupted()
                    .expect("foreign staging should be preserved"),
                CleanupOutcome::PreservedForeign
            );
            assert!(recipe.layout().staging_directory().exists());
        }

        #[test]
        fn release_digest_rejects_tampering() {
            let mut release = pinned_windows_x64_release();
            assert!(release.verify_source(b"tampered").is_err());
            release.source_sha256 = hex_digest(b"known source");
            assert!(release.verify_source(b"known source").is_ok());
        }
    }
}
