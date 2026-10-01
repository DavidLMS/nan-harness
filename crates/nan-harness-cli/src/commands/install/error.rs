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

    fn missing_npm(&self) -> Option<HarnessKind> {
        match self {
            Self::CommandStart {
                harness,
                program: "npm",
                source,
            } if source.kind() == io::ErrorKind::NotFound => Some(*harness),
            _ => None,
        }
    }

    pub(crate) fn is_runtime_precondition(&self) -> bool {
        self.missing_npm().is_some()
            || matches!(
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

// Terminal localization is separate from canonical Display used by machine contracts.
impl nan_harness_i18n::TerminalMessage for InstallError {
    #[expect(
        clippy::too_many_lines,
        reason = "exhaustive terminal projection keeps every error variant visible"
    )]
    fn terminal_message(&self, locale: nan_harness_i18n::Locale) -> String {
        use nan_harness_i18n::messages as m;
        if let Some(harness) = self.missing_npm() {
            return super::runtime::npm_hint_for(harness, locale);
        }
        if locale == nan_harness_i18n::Locale::En {
            return self.to_string();
        }
        match self {
            Self::Prompt(field_0) => m::error_install_prompt(locale, &(field_0)),
            Self::UnsupportedPlatform(field_0) => {
                m::error_install_unsupported_platform(locale, &(field_0))
            }
            Self::UnsupportedHarness(field_0) => {
                m::error_install_unsupported_harness(locale, &(field_0))
            }
            Self::CompatibilityManifest(field_0) => {
                m::error_install_compatibility_manifest(locale, &(field_0))
            }
            Self::InvalidRuntimeCommand { harness, command } => {
                m::error_install_invalid_runtime_command(locale, &(command), &(harness))
            }
            Self::RuntimeCommandStart {
                harness,
                command,
                minimum,
                hint: _,
                source,
            } => m::error_install_runtime_command_start(
                locale,
                &(command),
                &(harness),
                &(super::runtime::runtime_hint_for(*harness, minimum, locale)),
                &(minimum),
                &(source),
            ),
            Self::RuntimeCommandFailed {
                harness,
                command,
                minimum,
                exit_code,
                hint: _,
            } => m::error_install_runtime_command_failed(
                locale,
                &(command),
                &(harness),
                &(super::runtime::runtime_hint_for(*harness, minimum, locale)),
                &(minimum),
                &(exit_code.map_or_else(String::new, |code| m::error_exit_code(locale, &code))),
            ),
            Self::RuntimeUnsupported {
                harness,
                detected,
                minimum,
                hint: _,
            } => m::error_install_runtime_unsupported(
                locale,
                &(detected),
                &(harness),
                &(super::runtime::runtime_hint_for(*harness, minimum, locale)),
                &(minimum),
            ),
            Self::RuntimeUnparseable {
                harness,
                detected,
                minimum,
                hint: _,
            } => m::error_install_runtime_unparseable(
                locale,
                &(detected),
                &(harness),
                &(super::runtime::runtime_hint_for(*harness, minimum, locale)),
                &(minimum),
            ),
            Self::DownloadStart {
                harness,
                url,
                source,
            } => m::error_install_download_start(locale, &(harness), &(source), &(url)),
            Self::PrepareInstaller { harness, source } => {
                m::error_install_prepare_installer(locale, &(harness), &(source))
            }
            Self::InstallerStart {
                harness,
                interpreter,
                source,
            } => m::error_install_installer_start(locale, &(harness), &(interpreter), &(source)),
            Self::DownloadFailed { harness, exit_code } => m::error_install_download_failed(
                locale,
                &(harness),
                &(exit_code.map_or_else(String::new, |code| m::error_exit_code(locale, &code))),
            ),
            Self::InstallerFailed {
                harness,
                interpreter,
                exit_code,
            } => m::error_install_installer_failed(
                locale,
                &(harness),
                &(interpreter),
                &(exit_code.map_or_else(String::new, |code| m::error_exit_code(locale, &code))),
            ),
            Self::CommandStart {
                harness,
                program,
                source,
            } => m::error_install_command_start(locale, &(harness), &(program), &(source)),
            Self::CommandFailed {
                harness,
                program,
                exit_code,
            } => m::error_install_command_failed(
                locale,
                &(harness),
                &(program),
                &(exit_code.map_or_else(String::new, |code| m::error_exit_code(locale, &code))),
            ),
            Self::PostInstallCheckStart {
                harness,
                command,
                source,
            } => {
                m::error_install_post_install_check_start(locale, &(command), &(harness), &(source))
            }
            Self::PostInstallCheckPrepare { harness, source } => {
                m::error_install_post_install_check_prepare(locale, &(harness), &(source))
            }
            Self::PostInstallCheckFailed {
                harness,
                command,
                exit_code,
                details,
            } => m::error_install_post_install_check_failed(
                locale,
                &(command),
                &(details),
                &(harness),
                &(exit_code.map_or_else(String::new, |code| m::error_exit_code(locale, &code))),
            ),
        }
    }
}
