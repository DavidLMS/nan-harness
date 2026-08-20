use nan_harness_core::HarnessKind;
use std::env;
use std::fs;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::NamedTempFile;
use thiserror::Error;

const CLAUDE_CODE_INSTALL_URL: &str = "https://claude.ai/install.sh";
const CODEX_INSTALL_URL: &str = "https://chatgpt.com/codex/install.sh";
const OPENCODE_INSTALL_URL: &str = "https://opencode.ai/install";
const HERMES_INSTALL_URL: &str = "https://hermes-agent.nousresearch.com/install.sh";
const PI_INSTALL_URL: &str = "https://pi.dev/install.sh";
const PRIME_AGENT_INSTALL_URL: &str = "https://app.primeintellect.ai/prime-agent/install.sh";
const OPENCLAW_INSTALL_URL: &str = "https://openclaw.ai/install.sh";
const CLINE_INSTALL_URL: &str = "https://docs.cline.bot/getting-started/installing-cline";
const QWEN_CODE_INSTALL_URL: &str =
    "https://qwen-code-assets.oss-cn-hangzhou.aliyuncs.com/installation/install-qwen.sh";
const KIMI_CODE_INSTALL_URL: &str = "https://code.kimi.com/kimi-code/install.sh";
const AIDER_INSTALL_URL: &str = "https://aider.chat/install.sh";
const GOOSE_INSTALL_URL: &str =
    "https://github.com/aaif-goose/goose/releases/download/stable/download_cli.sh";
const DEEPSEEK_HARNESS_INSTALL_URL: &str = "https://github.com/HenryZ838978/deepseek-harness";
const FX_INSTALL_URL: &str = "https://fx.sh/setup.sh";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InstallDecision {
    NotInteractive,
    Declined,
    Installed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstallMethod {
    ShellScript {
        url: &'static str,
        interpreter: &'static str,
        arguments: &'static [&'static str],
    },
    PowerShellScript {
        url: &'static str,
        command: &'static str,
    },
    Command {
        program: &'static str,
        arguments: &'static [&'static str],
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InstallSpec {
    kind: HarnessKind,
    display_name: &'static str,
    official_url: &'static str,
    unix: InstallMethod,
    windows: Option<InstallMethod>,
}

const INSTALL_SPECS: &[InstallSpec] = &[
    InstallSpec {
        kind: HarnessKind::ClaudeCode,
        display_name: "Claude Code",
        official_url: CLAUDE_CODE_INSTALL_URL,
        unix: shell_script(CLAUDE_CODE_INSTALL_URL, "bash", &[]),
        windows: Some(powershell_script(
            "https://claude.ai/install.ps1",
            "irm https://claude.ai/install.ps1 | iex",
        )),
    },
    InstallSpec {
        kind: HarnessKind::Codex,
        display_name: "Codex",
        official_url: CODEX_INSTALL_URL,
        unix: shell_script(CODEX_INSTALL_URL, "sh", &[]),
        windows: Some(command(
            "npm",
            &["install", "--global", "@openai/codex@latest"],
        )),
    },
    InstallSpec {
        kind: HarnessKind::OpenCode,
        display_name: "OpenCode",
        official_url: OPENCODE_INSTALL_URL,
        unix: shell_script(OPENCODE_INSTALL_URL, "bash", &[]),
        windows: Some(command(
            "npm",
            &["install", "--global", "opencode-ai@latest"],
        )),
    },
    InstallSpec {
        kind: HarnessKind::Hermes,
        display_name: "Hermes Agent",
        official_url: HERMES_INSTALL_URL,
        unix: shell_script(HERMES_INSTALL_URL, "bash", &[]),
        windows: Some(powershell_script(
            "https://hermes-agent.nousresearch.com/install.ps1",
            "irm https://hermes-agent.nousresearch.com/install.ps1 | iex",
        )),
    },
    InstallSpec {
        kind: HarnessKind::Pi,
        display_name: "Pi",
        official_url: PI_INSTALL_URL,
        unix: shell_script(PI_INSTALL_URL, "sh", &[]),
        windows: Some(command(
            "npm",
            &[
                "install",
                "--global",
                "--ignore-scripts",
                "@earendil-works/pi-coding-agent@latest",
            ],
        )),
    },
    InstallSpec {
        kind: HarnessKind::PrimeAgent,
        display_name: "Prime Agent",
        official_url: PRIME_AGENT_INSTALL_URL,
        unix: shell_script(PRIME_AGENT_INSTALL_URL, "sh", &[]),
        windows: None,
    },
    InstallSpec {
        kind: HarnessKind::OpenClaw,
        display_name: "OpenClaw",
        official_url: OPENCLAW_INSTALL_URL,
        unix: shell_script(OPENCLAW_INSTALL_URL, "bash", &["--no-onboard"]),
        windows: Some(powershell_script(
            "https://openclaw.ai/install.ps1",
            "& ([scriptblock]::Create((iwr -useb https://openclaw.ai/install.ps1))) -NoOnboard",
        )),
    },
    InstallSpec {
        kind: HarnessKind::Cline,
        display_name: "Cline",
        official_url: CLINE_INSTALL_URL,
        unix: command("npm", &["install", "--global", "cline@latest"]),
        windows: Some(command("npm", &["install", "--global", "cline@latest"])),
    },
    InstallSpec {
        kind: HarnessKind::QwenCode,
        display_name: "Qwen Code",
        official_url: QWEN_CODE_INSTALL_URL,
        unix: shell_script(QWEN_CODE_INSTALL_URL, "bash", &["--source", "website"]),
        windows: Some(command(
            "npm",
            &["install", "--global", "@qwen-code/qwen-code@latest"],
        )),
    },
    InstallSpec {
        kind: HarnessKind::KimiCode,
        display_name: "Kimi Code",
        official_url: KIMI_CODE_INSTALL_URL,
        unix: shell_script(KIMI_CODE_INSTALL_URL, "bash", &[]),
        windows: Some(powershell_script(
            "https://code.kimi.com/kimi-code/install.ps1",
            "irm https://code.kimi.com/kimi-code/install.ps1 | iex",
        )),
    },
    InstallSpec {
        kind: HarnessKind::Aider,
        display_name: "Aider",
        official_url: AIDER_INSTALL_URL,
        unix: shell_script(AIDER_INSTALL_URL, "sh", &[]),
        windows: Some(powershell_script(
            "https://aider.chat/install.ps1",
            "irm https://aider.chat/install.ps1 | iex",
        )),
    },
    InstallSpec {
        kind: HarnessKind::Goose,
        display_name: "Goose",
        official_url: GOOSE_INSTALL_URL,
        unix: shell_script(GOOSE_INSTALL_URL, "bash", &[]),
        windows: None,
    },
    InstallSpec {
        kind: HarnessKind::DeepSeekHarness,
        display_name: "DeepSeek Harness",
        official_url: DEEPSEEK_HARNESS_INSTALL_URL,
        unix: command(
            "python3",
            &[
                "-m",
                "pip",
                "install",
                "--user",
                "--upgrade",
                "deepseek-harness-cli",
            ],
        ),
        windows: Some(command(
            "py",
            &[
                "-m",
                "pip",
                "install",
                "--user",
                "--upgrade",
                "deepseek-harness-cli",
            ],
        )),
    },
    InstallSpec {
        kind: HarnessKind::Fx,
        display_name: "fx",
        official_url: FX_INSTALL_URL,
        unix: shell_script(FX_INSTALL_URL, "bash", &[]),
        windows: None,
    },
];

const fn shell_script(
    url: &'static str,
    interpreter: &'static str,
    arguments: &'static [&'static str],
) -> InstallMethod {
    InstallMethod::ShellScript {
        url,
        interpreter,
        arguments,
    }
}

const fn powershell_script(url: &'static str, command: &'static str) -> InstallMethod {
    InstallMethod::PowerShellScript { url, command }
}

const fn command(program: &'static str, arguments: &'static [&'static str]) -> InstallMethod {
    InstallMethod::Command { program, arguments }
}

pub(crate) fn install_spec(kind: HarnessKind) -> Option<&'static InstallSpec> {
    INSTALL_SPECS.iter().find(|spec| spec.kind == kind)
}

pub(crate) fn offer_install(kind: HarnessKind) -> Result<InstallDecision, InstallError> {
    let spec = install_spec(kind).ok_or(InstallError::UnsupportedHarness(kind))?;
    if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        return Ok(InstallDecision::NotInteractive);
    }

    let mut input = io::stdin().lock();
    let mut output = io::stderr().lock();
    writeln!(
        output,
        "{} was not found. Install the latest official release now?",
        spec.display_name
    )
    .map_err(InstallError::Prompt)?;
    writeln!(
        output,
        "Official installer: {}",
        official_install_command(spec)?
    )
    .map_err(InstallError::Prompt)?;
    write!(output, "Install {} [y/N]: ", spec.display_name).map_err(InstallError::Prompt)?;
    output.flush().map_err(InstallError::Prompt)?;

    let mut response = String::new();
    input
        .read_line(&mut response)
        .map_err(InstallError::Prompt)?;
    if !is_affirmative(&response) {
        return Ok(InstallDecision::Declined);
    }

    install(spec)?;
    Ok(InstallDecision::Installed)
}

fn is_affirmative(response: &str) -> bool {
    matches!(response.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

fn official_install_command(spec: &InstallSpec) -> Result<String, InstallError> {
    let method = if cfg!(windows) {
        spec.windows
            .ok_or(InstallError::UnsupportedPlatform(spec.kind))?
    } else {
        spec.unix
    };
    Ok(match method {
        InstallMethod::ShellScript {
            url,
            interpreter,
            arguments: [],
        } => format!("curl -fsSL {url} | {interpreter}"),
        InstallMethod::ShellScript {
            url,
            interpreter,
            arguments,
        } => format!(
            "curl -fsSL {url} | {interpreter} -s -- {}",
            arguments.join(" ")
        ),
        InstallMethod::PowerShellScript { command, .. } => command.to_owned(),
        InstallMethod::Command { program, arguments } => {
            format!("{program} {}", arguments.join(" "))
        }
    })
}

pub(crate) fn executable_from_known_locations(kind: HarnessKind) -> Option<PathBuf> {
    let home = env::var_os("HOME").or_else(|| env::var_os("USERPROFILE"))?;
    find_executable(kind, &PathBuf::from(home))
}

fn find_executable(kind: HarnessKind, home: &Path) -> Option<PathBuf> {
    executable_candidates(kind, home)
        .into_iter()
        .find(|executable| fs::metadata(executable).is_ok_and(|metadata| metadata.is_file()))
}

fn executable_candidates(kind: HarnessKind, home: &Path) -> Vec<PathBuf> {
    let executable_name = if cfg!(windows) {
        format!("{}.exe", kind.binary_name())
    } else {
        kind.binary_name().to_owned()
    };
    let candidates = match kind {
        HarnessKind::ClaudeCode
        | HarnessKind::Hermes
        | HarnessKind::Aider
        | HarnessKind::Goose
        | HarnessKind::DeepSeekHarness
        | HarnessKind::Fx => vec![home.join(".local/bin")],
        HarnessKind::Codex => vec![home.join(".local/bin"), home.join(".codex/bin")],
        HarnessKind::OpenCode => vec![home.join(".opencode/bin"), home.join(".local/bin")],
        HarnessKind::Pi => vec![
            home.join(".local/bin"),
            home.join(".npm-global/bin"),
            home.join(".local/share/pi-node/current/bin"),
        ],
        HarnessKind::PrimeAgent | HarnessKind::QwenCode | HarnessKind::Cline => {
            vec![home.join(".local/bin"), home.join(".npm-global/bin")]
        }
        HarnessKind::OpenClaw => vec![
            home.join(".local/bin"),
            home.join(".openclaw/bin"),
            home.join(".npm-global/bin"),
        ],
        HarnessKind::KimiCode => vec![home.join(".kimi-code/bin"), home.join(".local/bin")],
    };
    candidates
        .into_iter()
        .map(|directory| directory.join(&executable_name))
        .collect()
}

fn install(spec: &InstallSpec) -> Result<(), InstallError> {
    let method = if cfg!(windows) {
        spec.windows
            .ok_or(InstallError::UnsupportedPlatform(spec.kind))?
    } else {
        spec.unix
    };
    eprintln!(
        "Installing {} with the official installer...",
        spec.display_name
    );
    match method {
        InstallMethod::ShellScript {
            url,
            interpreter,
            arguments,
        } => install_shell_script(spec.kind, url, interpreter, arguments),
        InstallMethod::PowerShellScript { url, command } => {
            install_powershell_script(spec.kind, url, command)
        }
        InstallMethod::Command { program, arguments } => {
            let status = Command::new(program)
                .args(arguments)
                .status()
                .map_err(|source| InstallError::CommandStart {
                    harness: spec.kind,
                    program,
                    source,
                })?;
            if status.success() {
                Ok(())
            } else {
                Err(InstallError::CommandFailed {
                    harness: spec.kind,
                    program,
                    exit_code: status.code(),
                })
            }
        }
    }
}

fn install_shell_script(
    harness: HarnessKind,
    url: &'static str,
    interpreter: &'static str,
    arguments: &[&str],
) -> Result<(), InstallError> {
    install_shell_script_with_downloader(harness, url, interpreter, arguments, |path| {
        Command::new("curl")
            .args(["-fsSL", "--output"])
            .arg(path)
            .arg(url)
            .status()
    })
}

fn install_shell_script_with_downloader(
    harness: HarnessKind,
    url: &'static str,
    interpreter: &'static str,
    arguments: &[&str],
    download: impl FnOnce(&Path) -> io::Result<std::process::ExitStatus>,
) -> Result<(), InstallError> {
    let installer = NamedTempFile::new()
        .map_err(|source| InstallError::PrepareInstaller { harness, source })?;
    let download_status =
        download(installer.path()).map_err(|source| InstallError::DownloadStart {
            harness,
            url,
            source,
        })?;
    if !download_status.success() {
        return Err(InstallError::DownloadFailed {
            harness,
            exit_code: download_status.code(),
        });
    }
    let installer_input = installer
        .reopen()
        .map_err(|source| InstallError::PrepareInstaller { harness, source })?;
    let mut installer = Command::new(interpreter);
    installer.arg("-s").arg("--").args(arguments);
    let installer_status = installer
        .stdin(Stdio::from(installer_input))
        .status()
        .map_err(|source| InstallError::InstallerStart {
            harness,
            interpreter,
            source,
        })?;
    if !installer_status.success() {
        return Err(InstallError::InstallerFailed {
            harness,
            interpreter,
            exit_code: installer_status.code(),
        });
    }
    Ok(())
}

fn install_powershell_script(
    harness: HarnessKind,
    _url: &'static str,
    command: &str,
) -> Result<(), InstallError> {
    let status = Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            command,
        ])
        .status()
        .map_err(|source| InstallError::InstallerStart {
            harness,
            interpreter: "powershell",
            source,
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(InstallError::InstallerFailed {
            harness,
            interpreter: "powershell",
            exit_code: status.code(),
        })
    }
}

#[derive(Debug, Error)]
pub(crate) enum InstallError {
    #[error("could not prompt for installation: {0}")]
    Prompt(io::Error),
    #[error("{0} does not have an official installer for this platform")]
    UnsupportedPlatform(HarnessKind),
    #[error("{0} does not have a configured official installer")]
    UnsupportedHarness(HarnessKind),
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
}

impl InstallError {
    pub(crate) const fn code() -> &'static str {
        "NH-INSTALL-001"
    }
}

fn exit_code_suffix(code: Option<i32>) -> String {
    match code {
        Some(code) => format!(" with exit code {code}"),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AIDER_INSTALL_URL, CLAUDE_CODE_INSTALL_URL, CLINE_INSTALL_URL, CODEX_INSTALL_URL,
        DEEPSEEK_HARNESS_INSTALL_URL, FX_INSTALL_URL, GOOSE_INSTALL_URL, HERMES_INSTALL_URL,
        KIMI_CODE_INSTALL_URL, OPENCLAW_INSTALL_URL, OPENCODE_INSTALL_URL, PI_INSTALL_URL,
        PRIME_AGENT_INSTALL_URL, QWEN_CODE_INSTALL_URL, executable_candidates, find_executable,
        install_spec, is_affirmative, official_install_command,
    };
    use nan_harness_core::HarnessKind;
    use std::fs;

    #[test]
    fn official_installers_are_current_and_use_latest_release_defaults() {
        let expected = [
            (HarnessKind::ClaudeCode, CLAUDE_CODE_INSTALL_URL),
            (HarnessKind::Codex, CODEX_INSTALL_URL),
            (HarnessKind::OpenCode, OPENCODE_INSTALL_URL),
            (HarnessKind::Hermes, HERMES_INSTALL_URL),
            (HarnessKind::Pi, PI_INSTALL_URL),
            (HarnessKind::PrimeAgent, PRIME_AGENT_INSTALL_URL),
            (HarnessKind::OpenClaw, OPENCLAW_INSTALL_URL),
            (HarnessKind::Cline, CLINE_INSTALL_URL),
            (HarnessKind::QwenCode, QWEN_CODE_INSTALL_URL),
            (HarnessKind::KimiCode, KIMI_CODE_INSTALL_URL),
            (HarnessKind::Aider, AIDER_INSTALL_URL),
            (HarnessKind::Goose, GOOSE_INSTALL_URL),
            (HarnessKind::DeepSeekHarness, DEEPSEEK_HARNESS_INSTALL_URL),
            (HarnessKind::Fx, FX_INSTALL_URL),
        ];

        for (kind, url) in expected {
            let spec = install_spec(kind).expect("installable harness should have a spec");
            assert_eq!(spec.official_url, url);
        }
    }

    #[test]
    fn missing_and_declined_install_responses_are_nonfatal() {
        assert!(!is_affirmative(""));
        assert!(!is_affirmative("no"));
        assert!(!is_affirmative("N"));
        assert!(is_affirmative("y"));
        assert!(is_affirmative("YES\n"));
    }

    #[test]
    fn official_install_prompt_contains_the_executable_command() {
        let spec = install_spec(HarnessKind::Cline).expect("Cline should be installable");
        let command = official_install_command(spec).expect("Cline command should be available");

        assert_eq!(command, "npm install --global cline@latest");
    }

    #[test]
    fn finds_installed_executables_in_official_user_directories() {
        let directory = tempfile::tempdir().expect("temporary home should exist");
        let executable = directory.path().join(".opencode/bin/opencode");
        fs::create_dir_all(executable.parent().expect("executable parent should exist"))
            .expect("OpenCode bin directory should be created");
        fs::write(&executable, "fake opencode executable")
            .expect("fake executable should be written");

        assert_eq!(
            find_executable(HarnessKind::OpenCode, directory.path()),
            Some(executable)
        );
    }

    #[cfg(unix)]
    #[test]
    fn candidates_include_paths_used_by_script_installers() {
        let home = std::path::Path::new("/tmp/nan-test-home");
        let candidates = executable_candidates(HarnessKind::KimiCode, home);
        assert!(candidates.contains(&home.join(".kimi-code/bin/kimi")));
        assert!(candidates.contains(&home.join(".local/bin/kimi")));
    }

    #[cfg(unix)]
    #[test]
    fn failed_installer_command_is_reported_with_exit_code() {
        let spec = super::InstallSpec {
            kind: HarnessKind::KimiCode,
            display_name: "Kimi Code",
            official_url: KIMI_CODE_INSTALL_URL,
            unix: super::command("sh", &["-c", "exit 23"]),
            windows: None,
        };

        let error = super::install(&spec).expect_err("failed installer should be reported");
        assert!(matches!(
            error,
            super::InstallError::CommandFailed {
                harness: HarnessKind::KimiCode,
                program: "sh",
                exit_code: Some(23),
            }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn unavailable_installer_interpreter_is_reported_as_start_error() {
        let error = super::install_powershell_script(
            HarnessKind::KimiCode,
            "https://code.kimi.com/kimi-code/install.ps1",
            "exit",
        )
        .expect_err("missing powershell should be reported");
        assert!(matches!(
            error,
            super::InstallError::InstallerStart {
                harness: HarnessKind::KimiCode,
                interpreter: "powershell",
                ..
            }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn failed_downloads_never_execute_partial_installers() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let marker = directory.path().join("executed");
        let error = super::install_shell_script_with_downloader(
            HarnessKind::KimiCode,
            KIMI_CODE_INSTALL_URL,
            "sh",
            &[],
            |path| {
                fs::write(path, format!("touch '{}'\n", marker.display()))?;
                std::process::Command::new("sh")
                    .args(["-c", "exit 23"])
                    .status()
            },
        )
        .expect_err("failed download should be reported");

        assert!(matches!(
            error,
            super::InstallError::DownloadFailed {
                harness: HarnessKind::KimiCode,
                exit_code: Some(23),
            }
        ));
        assert!(!marker.exists());
    }

    #[cfg(unix)]
    #[test]
    fn completed_downloads_execute_the_buffered_installer() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let marker = directory.path().join("executed");
        super::install_shell_script_with_downloader(
            HarnessKind::KimiCode,
            KIMI_CODE_INSTALL_URL,
            "sh",
            &[],
            |path| {
                fs::write(path, format!("printf INSTALLED > '{}'\n", marker.display()))?;
                std::process::Command::new("sh")
                    .args(["-c", "exit 0"])
                    .status()
            },
        )
        .expect("completed download should execute");

        assert_eq!(
            fs::read_to_string(marker).expect("installer marker should exist"),
            "INSTALLED"
        );
    }
}
