use nan_harness_core::HarnessKind;
use semver::Version;
use std::io;
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum InstallError {
    #[error("could not prompt for installation: {0}")]
    Prompt(io::Error),
    #[error("{0} does not have an official installer for this platform")]
    UnsupportedPlatform(HarnessKind),
    #[error("{0} does not have a configured official installer")]
    UnsupportedHarness(HarnessKind),
    #[error("could not read embedded runtime compatibility requirements: {0}")]
    CompatibilityManifest(String),
    #[error("the embedded runtime command '{command}' for {harness} is invalid")]
    InvalidRuntimeCommand {
        harness: HarnessKind,
        command: String,
    },
    #[error(
        "could not run required runtime command '{command}' for {harness}: {source}. Node.js >= {minimum} is required.{hint}"
    )]
    RuntimeCommandStart {
        harness: HarnessKind,
        command: String,
        minimum: Version,
        hint: String,
        #[source]
        source: io::Error,
    },
    #[error(
        "required runtime command '{command}' for {harness} failed{}; Node.js >= {minimum} is required.{hint}",
        exit_code_suffix(*exit_code)
    )]
    RuntimeCommandFailed {
        harness: HarnessKind,
        command: String,
        minimum: Version,
        exit_code: Option<i32>,
        hint: String,
    },
    #[error("{harness} requires Node.js >= {minimum}, but detected Node.js {detected}.{hint}")]
    RuntimeUnsupported {
        harness: HarnessKind,
        detected: String,
        minimum: Version,
        hint: String,
    },
    #[error(
        "{harness} requires Node.js >= {minimum}, but could not parse the runtime version '{detected}'.{hint}"
    )]
    RuntimeUnparseable {
        harness: HarnessKind,
        detected: String,
        minimum: Version,
        hint: String,
    },
    #[error("could not start the {harness} installer download from {url}: {source}")]
    DownloadStart {
        harness: HarnessKind,
        url: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("could not prepare the downloaded {harness} installer: {source}")]
    PrepareInstaller {
        harness: HarnessKind,
        #[source]
        source: io::Error,
    },
    #[error("could not start the {harness} installer with {interpreter}: {source}")]
    InstallerStart {
        harness: HarnessKind,
        interpreter: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("the {harness} installer download failed{}", exit_code_suffix(*exit_code))]
    DownloadFailed {
        harness: HarnessKind,
        exit_code: Option<i32>,
    },
    #[error("the {harness} installer failed with {interpreter}{}", exit_code_suffix(*exit_code))]
    InstallerFailed {
        harness: HarnessKind,
        interpreter: &'static str,
        exit_code: Option<i32>,
    },
    #[error("could not start the {harness} installer command {program}: {source}")]
    CommandStart {
        harness: HarnessKind,
        program: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("the {harness} installer command {program} failed{}", exit_code_suffix(*exit_code))]
    CommandFailed {
        harness: HarnessKind,
        program: &'static str,
        exit_code: Option<i32>,
    },
    #[error("could not run the post-install check '{command}' for {harness}: {source}")]
    PostInstallCheckStart {
        harness: HarnessKind,
        command: String,
        #[source]
        source: io::Error,
    },
    #[error("could not prepare an isolated post-install check for {harness}: {source}")]
    PostInstallCheckPrepare {
        harness: HarnessKind,
        #[source]
        source: io::Error,
    },
    #[error(
        "{harness} was installed, but its startup check '{command}' failed{}: {details}",
        exit_code_suffix(*exit_code)
    )]
    PostInstallCheckFailed {
        harness: HarnessKind,
        command: String,
        exit_code: Option<i32>,
        details: String,
    },
}

impl InstallError {
    pub(crate) const fn code() -> &'static str {
        "NH-INSTALL-001"
    }

    pub(crate) const fn is_runtime_precondition(&self) -> bool {
        matches!(
            self,
            Self::RuntimeCommandStart { .. }
                | Self::RuntimeCommandFailed { .. }
                | Self::RuntimeUnsupported { .. }
                | Self::RuntimeUnparseable { .. }
        )
    }
}

fn exit_code_suffix(code: Option<i32>) -> String {
    match code {
        Some(code) => format!(" with exit code {code}"),
        None => String::new(),
    }
}
