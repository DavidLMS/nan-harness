use nan_harness_core::{HarnessKind, RuntimeCompatibility};
use nan_harness_runtime::{bundled_compatibility_manifest, is_executable_file};
use semver::Version;
use std::env;
use std::ffi::{OsStr, OsString};
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use tempfile::{NamedTempFile, TempDir};
use thiserror::Error;

const CLAUDE_CODE_INSTALL_URL: &str = "https://claude.ai/install.sh";
const CODEX_INSTALL_URL: &str = "https://chatgpt.com/codex/install.sh";
const OPENCODE_INSTALL_URL: &str = "https://opencode.ai/install";
const HERMES_INSTALL_URL: &str = "https://hermes-agent.nousresearch.com/install.sh";
const PI_INSTALL_URL: &str = "https://pi.dev/install.sh";
const OMP_INSTALL_URL: &str = "https://omp.sh/install";
const PRIME_AGENT_INSTALL_URL: &str = "https://app.primeintellect.ai/prime-agent/install.sh";
const OPENCLAW_INSTALL_URL: &str = "https://openclaw.ai/install.sh";
const CLINE_INSTALL_URL: &str = "https://docs.cline.bot/getting-started/installing-cline";
const QWEN_CODE_INSTALL_URL: &str =
    "https://qwen-code-assets.oss-cn-hangzhou.aliyuncs.com/installation/install-qwen.sh";
const KIMI_CODE_INSTALL_URL: &str = "https://code.kimi.com/kimi-code/install.sh";
const AIDER_INSTALL_URL: &str = "https://aider.chat/install.sh";
const GOOSE_INSTALL_URL: &str =
    "https://github.com/aaif-goose/goose/releases/download/stable/download_cli.sh";
const DEEPSEEK_HARNESS_INSTALL_URL: &str = "https://github.com/deepseek-ai/deepseek-harness";
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
        kind: HarnessKind::Omp,
        display_name: "Oh My Pi",
        official_url: OMP_INSTALL_URL,
        unix: shell_script(OMP_INSTALL_URL, "sh", &["--binary"]),
        windows: Some(powershell_script(
            "https://omp.sh/install.ps1",
            "& ([scriptblock]::Create((irm https://omp.sh/install.ps1))) -Binary",
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
        unix: command(
            "npm",
            &[
                "install",
                "--global",
                "--allow-scripts=cline,protobufjs",
                "cline@latest",
            ],
        ),
        windows: Some(command(
            "npm",
            &[
                "install",
                "--global",
                "--allow-scripts=cline,protobufjs",
                "cline@latest",
            ],
        )),
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
            "npm",
            &[
                "install",
                "--global",
                "--engine-strict",
                "--allow-scripts=@deepseek-ai/dsh-subprocess-local,koffi,node-pty,@google/genai,protobufjs",
                "@deepseek-ai/dsh@latest",
            ],
        ),
        windows: Some(command(
            "npm",
            &[
                "install",
                "--global",
                "--engine-strict",
                "--allow-scripts=@deepseek-ai/dsh-subprocess-local,koffi,node-pty,@google/genai,protobufjs",
                "@deepseek-ai/dsh@latest",
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

fn runtime_requirement(kind: HarnessKind) -> Result<Option<RuntimeCompatibility>, InstallError> {
    let manifest = bundled_compatibility_manifest()
        .map_err(|error| InstallError::CompatibilityManifest(error.to_string()))?;
    Ok(manifest.entry(kind).and_then(|entry| entry.runtime.clone()))
}

fn runtime_command(
    kind: HarnessKind,
    requirement: &RuntimeCompatibility,
) -> Result<(String, Vec<String>), InstallError> {
    let mut parts = requirement.command.split_ascii_whitespace();
    let Some(program) = parts.next() else {
        return Err(InstallError::InvalidRuntimeCommand {
            harness: kind,
            command: requirement.command.clone(),
        });
    };
    let arguments = parts.map(ToOwned::to_owned).collect::<Vec<_>>();
    if arguments.is_empty() {
        return Err(InstallError::InvalidRuntimeCommand {
            harness: kind,
            command: requirement.command.clone(),
        });
    }
    Ok((program.to_owned(), arguments))
}

fn runtime_hint(kind: HarnessKind, minimum: &Version) -> String {
    format!(
        "\n\nRecommended fix with nvm:\n  nvm install {}\n  nvm use {}\n  node --version\n  nan {}\n\nIf nvm is unavailable, install Node.js {minimum} or newer with fnm, Volta, asdf, or the official Node.js installer.",
        minimum.major,
        minimum.major,
        kind.binary_name()
    )
}

fn first_non_empty_output_line(output: &Output) -> String {
    for stream in [&output.stdout, &output.stderr] {
        if let Some(line) = stream
            .split(|byte| *byte == b'\n' || *byte == b'\r')
            .map(|line| String::from_utf8_lossy(line).trim().to_owned())
            .find(|line| !line.is_empty())
        {
            return line;
        }
    }
    String::new()
}

fn parse_runtime_version(output: &Output) -> String {
    first_non_empty_output_line(output)
}

pub(crate) fn check_required_runtime(kind: HarnessKind) -> Result<(), InstallError> {
    let Some(requirement) = runtime_requirement(kind)? else {
        return Ok(());
    };
    let (program, arguments) = runtime_command(kind, &requirement)?;
    let command = format!("{program} {}", arguments.join(" "));
    let hint = runtime_hint(kind, &requirement.minimum_version);
    let output = Command::new(&program)
        .args(&arguments)
        .output()
        .map_err(|source| InstallError::RuntimeCommandStart {
            harness: kind,
            command: command.clone(),
            minimum: requirement.minimum_version.clone(),
            hint: hint.clone(),
            source,
        })?;
    if !output.status.success() {
        return Err(InstallError::RuntimeCommandFailed {
            harness: kind,
            command,
            minimum: requirement.minimum_version,
            exit_code: output.status.code(),
            hint,
        });
    }

    let detected = parse_runtime_version(&output);
    let parsed = detected
        .strip_prefix('v')
        .and_then(|value| Version::parse(value.trim()).ok());
    match parsed {
        Some(version) if version >= requirement.minimum_version => Ok(()),
        Some(_) => Err(InstallError::RuntimeUnsupported {
            harness: kind,
            detected,
            minimum: requirement.minimum_version,
            hint,
        }),
        None => Err(InstallError::RuntimeUnparseable {
            harness: kind,
            detected,
            minimum: requirement.minimum_version,
            hint,
        }),
    }
}

const DSH_POST_INSTALL_CHECK: &[&str] = &["--profile", "web", "--help"];
const CLINE_POST_INSTALL_CHECK: &[&str] = &["--version"];
const OMP_POST_INSTALL_CHECK: &[&str] = &["--version"];

fn post_install_check_arguments(kind: HarnessKind) -> Option<&'static [&'static str]> {
    match kind {
        HarnessKind::DeepSeekHarness => Some(DSH_POST_INSTALL_CHECK),
        HarnessKind::Cline => Some(CLINE_POST_INSTALL_CHECK),
        HarnessKind::Omp => Some(OMP_POST_INSTALL_CHECK),
        _ => None,
    }
}

fn summarize_output(output: &Output) -> String {
    let mut summary = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if summary.is_empty() {
        summary.push_str(String::from_utf8_lossy(&output.stdout).trim());
    }
    if summary.chars().count() > 2_000 {
        summary = summary.chars().take(2_000).collect();
        summary.push('…');
    }
    summary
}

fn verify_post_install(kind: HarnessKind) -> Result<(), InstallError> {
    let Some(arguments) = post_install_check_arguments(kind) else {
        return Ok(());
    };
    let executable = executable_from_known_locations(kind).map_or_else(
        || kind.binary_name().to_owned(),
        |path| path.to_string_lossy().into_owned(),
    );
    verify_post_install_with_executable(kind, &executable, arguments)
}

fn verify_post_install_with_executable(
    kind: HarnessKind,
    executable: &str,
    arguments: &[&str],
) -> Result<(), InstallError> {
    let command = format!("{} {}", executable, arguments.join(" "));
    let isolated_home = TempDir::new().map_err(|source| InstallError::PostInstallCheckPrepare {
        harness: kind,
        source,
    })?;
    let mut check = Command::new(executable);
    check.args(arguments);
    if kind == HarnessKind::DeepSeekHarness {
        check
            .env("HOME", isolated_home.path())
            .env("USERPROFILE", isolated_home.path());
    }
    let output = check
        .output()
        .map_err(|source| InstallError::PostInstallCheckStart {
            harness: kind,
            command: command.clone(),
            source,
        })?;
    if output.status.success() {
        return Ok(());
    }
    Err(InstallError::PostInstallCheckFailed {
        harness: kind,
        command,
        exit_code: output.status.code(),
        details: summarize_output(&output),
    })
}

pub(crate) fn offer_install(kind: HarnessKind) -> Result<InstallDecision, InstallError> {
    let spec = install_spec(kind).ok_or(InstallError::UnsupportedHarness(kind))?;
    if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        return Ok(InstallDecision::NotInteractive);
    }
    check_required_runtime(kind)?;

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
    verify_post_install(kind)?;
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
        .find(|executable| is_executable_file(executable))
}

fn executable_candidates(kind: HarnessKind, home: &Path) -> Vec<PathBuf> {
    let path_extensions = env::var_os("PATHEXT");
    let app_data = env::var_os("APPDATA").map(PathBuf::from);
    let local_app_data = env::var_os("LOCALAPPDATA").map(PathBuf::from);
    executable_candidates_for_platform(
        kind,
        home,
        cfg!(windows),
        path_extensions.as_deref(),
        app_data.as_deref(),
        local_app_data.as_deref(),
    )
}

fn executable_candidates_for_platform(
    kind: HarnessKind,
    home: &Path,
    windows: bool,
    path_extensions: Option<&OsStr>,
    app_data: Option<&Path>,
    local_app_data: Option<&Path>,
) -> Vec<PathBuf> {
    let mut directories = match kind {
        HarnessKind::ClaudeCode
        | HarnessKind::Hermes
        | HarnessKind::Aider
        | HarnessKind::Goose
        | HarnessKind::Fx
        | HarnessKind::Omp => vec![home.join(".local/bin")],
        HarnessKind::Codex => vec![home.join(".local/bin"), home.join(".codex/bin")],
        HarnessKind::OpenCode => vec![home.join(".opencode/bin"), home.join(".local/bin")],
        HarnessKind::Pi => vec![
            home.join(".local/bin"),
            home.join(".npm-global/bin"),
            home.join(".local/share/pi-node/current/bin"),
        ],
        HarnessKind::PrimeAgent
        | HarnessKind::DeepSeekHarness
        | HarnessKind::QwenCode
        | HarnessKind::Cline => {
            vec![home.join(".local/bin"), home.join(".npm-global/bin")]
        }
        HarnessKind::OpenClaw => vec![
            home.join(".local/bin"),
            home.join(".openclaw/bin"),
            home.join(".npm-global/bin"),
        ],
        HarnessKind::KimiCode => vec![home.join(".kimi-code/bin"), home.join(".local/bin")],
    };
    if windows
        && matches!(
            kind,
            HarnessKind::Codex
                | HarnessKind::OpenCode
                | HarnessKind::Pi
                | HarnessKind::DeepSeekHarness
                | HarnessKind::OpenClaw
                | HarnessKind::Cline
                | HarnessKind::QwenCode
        )
        && let Some(app_data) = app_data
    {
        directories.push(app_data.join("npm"));
    }
    if windows
        && kind == HarnessKind::Omp
        && let Some(local_app_data) = local_app_data
    {
        directories.push(local_app_data.join("omp"));
    }
    let executable_names = executable_names(kind.binary_name(), windows, path_extensions);
    directories
        .into_iter()
        .flat_map(|directory| {
            executable_names
                .iter()
                .map(move |name| directory.join(name))
        })
        .collect()
}

fn executable_names(
    binary_name: &str,
    windows: bool,
    path_extensions: Option<&OsStr>,
) -> Vec<OsString> {
    if !windows {
        return vec![OsString::from(binary_name)];
    }
    let extensions = path_extensions.unwrap_or_else(|| OsStr::new(".COM;.EXE;.BAT;.CMD"));
    extensions
        .to_string_lossy()
        .split(';')
        .filter(|extension| !extension.is_empty())
        .map(|extension| OsString::from(format!("{binary_name}{extension}")))
        .chain(std::iter::once(OsString::from(binary_name)))
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
                if spec.kind == HarnessKind::Cline {
                    refresh_cline_binary_cache()
                } else {
                    Ok(())
                }
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

fn refresh_cline_binary_cache() -> Result<(), InstallError> {
    let root_command = "npm root --global";
    let root_output = Command::new("npm")
        .args(["root", "--global"])
        .output()
        .map_err(|source| InstallError::PostInstallCheckStart {
            harness: HarnessKind::Cline,
            command: root_command.to_owned(),
            source,
        })?;
    if !root_output.status.success() {
        return Err(InstallError::PostInstallCheckFailed {
            harness: HarnessKind::Cline,
            command: root_command.to_owned(),
            exit_code: root_output.status.code(),
            details: summarize_output(&root_output),
        });
    }

    let global_root = PathBuf::from(first_non_empty_output_line(&root_output));
    if !global_root.is_absolute() {
        return Err(InstallError::PostInstallCheckFailed {
            harness: HarnessKind::Cline,
            command: root_command.to_owned(),
            exit_code: None,
            details: "npm returned an invalid global package root".to_owned(),
        });
    }
    let package_root = global_root.join("cline");
    let postinstall = package_root.join("postinstall.mjs");
    let command = format!("node {}", postinstall.display());
    let output = Command::new("node")
        .arg(&postinstall)
        .output()
        .map_err(|source| InstallError::PostInstallCheckStart {
            harness: HarnessKind::Cline,
            command: command.clone(),
            source,
        })?;
    if !output.status.success() {
        return Err(InstallError::PostInstallCheckFailed {
            harness: HarnessKind::Cline,
            command,
            exit_code: output.status.code(),
            details: summarize_output(&output),
        });
    }

    if !cfg!(windows) {
        let cached_binary = package_root.join("bin/.cline");
        if !is_executable_file(&cached_binary) {
            return Err(InstallError::PostInstallCheckFailed {
                harness: HarnessKind::Cline,
                command,
                exit_code: None,
                details: format!(
                    "Cline postinstall did not create an executable cache at {}",
                    cached_binary.display()
                ),
            });
        }
    }
    Ok(())
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
    configure_shell_installer_command(harness, &mut installer);
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

fn configure_shell_installer_command(harness: HarnessKind, command: &mut Command) {
    if harness != HarnessKind::Pi {
        return;
    }
    let homebrew_bin_dir = homebrew_bin_dir();
    configure_shell_installer_path(
        harness,
        command,
        env::var_os("PATH").as_deref(),
        homebrew_bin_dir.as_deref(),
    );
}

fn configure_shell_installer_path(
    harness: HarnessKind,
    command: &mut Command,
    existing_path: Option<&OsStr>,
    homebrew_bin_dir: Option<&Path>,
) {
    if let Some(path) = preferred_installer_path(harness, existing_path, homebrew_bin_dir) {
        command.env("PATH", path);
    }
}

fn homebrew_bin_dir() -> Option<PathBuf> {
    if !cfg!(target_os = "macos") {
        return None;
    }

    let output = Command::new("brew").arg("--prefix").output().ok()?;
    if !output.status.success() {
        return None;
    }

    let prefix = String::from_utf8(output.stdout).ok()?;
    let prefix = PathBuf::from(prefix.trim());
    prefix.is_absolute().then(|| prefix.join("bin"))
}

fn preferred_installer_path(
    harness: HarnessKind,
    existing_path: Option<&OsStr>,
    homebrew_bin_dir: Option<&Path>,
) -> Option<OsString> {
    if harness != HarnessKind::Pi {
        return None;
    }
    let existing_path = existing_path?;
    if existing_path.is_empty() {
        return None;
    }
    let homebrew_bin_dir = homebrew_bin_dir?;
    let current_paths = env::split_paths(existing_path).collect::<Vec<_>>();
    let mut preferred_paths = Vec::with_capacity(current_paths.len() + 1);
    preferred_paths.push(homebrew_bin_dir.to_path_buf());
    preferred_paths.extend(
        current_paths
            .iter()
            .filter(|path| path.as_path() != homebrew_bin_dir)
            .cloned(),
    );
    if preferred_paths == current_paths {
        return None;
    }
    env::join_paths(preferred_paths).ok()
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

#[cfg(test)]
mod tests {
    use super::{
        AIDER_INSTALL_URL, CLAUDE_CODE_INSTALL_URL, CLINE_INSTALL_URL, CODEX_INSTALL_URL,
        DEEPSEEK_HARNESS_INSTALL_URL, FX_INSTALL_URL, GOOSE_INSTALL_URL, HERMES_INSTALL_URL,
        KIMI_CODE_INSTALL_URL, OMP_INSTALL_URL, OPENCLAW_INSTALL_URL, OPENCODE_INSTALL_URL,
        PI_INSTALL_URL, PRIME_AGENT_INSTALL_URL, QWEN_CODE_INSTALL_URL,
        configure_shell_installer_path, executable_candidates, executable_candidates_for_platform,
        find_executable, install_spec, is_affirmative, official_install_command,
        post_install_check_arguments, preferred_installer_path, runtime_hint, runtime_requirement,
        verify_post_install_with_executable,
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
            (HarnessKind::Omp, OMP_INSTALL_URL),
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
    fn pi_installer_prefers_homebrew_over_a_version_manager() {
        let current_path = std::env::join_paths([
            std::path::Path::new("/Users/nan/.nvm/versions/node/v20.19.4/bin"),
            std::path::Path::new("/opt/homebrew/bin"),
            std::path::Path::new("/usr/bin"),
        ])
        .expect("test PATH should be valid");
        let configured_path = preferred_installer_path(
            HarnessKind::Pi,
            Some(&current_path),
            Some(std::path::Path::new("/opt/homebrew/bin")),
        )
        .expect("Pi should receive a Homebrew-preferred PATH");
        let expected_path = std::env::join_paths([
            std::path::Path::new("/opt/homebrew/bin"),
            std::path::Path::new("/Users/nan/.nvm/versions/node/v20.19.4/bin"),
            std::path::Path::new("/usr/bin"),
        ])
        .expect("expected PATH should be valid");

        assert_eq!(configured_path, expected_path);
    }

    #[test]
    fn pi_installer_removes_duplicate_homebrew_entries() {
        let current_path = std::env::join_paths([
            std::path::Path::new("/opt/homebrew/bin"),
            std::path::Path::new("/Users/nan/.nvm/versions/node/v20.19.4/bin"),
            std::path::Path::new("/opt/homebrew/bin"),
        ])
        .expect("test PATH should be valid");
        let configured_path = preferred_installer_path(
            HarnessKind::Pi,
            Some(&current_path),
            Some(std::path::Path::new("/opt/homebrew/bin")),
        )
        .expect("duplicate Homebrew entries should be normalized");
        let expected_path = std::env::join_paths([
            std::path::Path::new("/opt/homebrew/bin"),
            std::path::Path::new("/Users/nan/.nvm/versions/node/v20.19.4/bin"),
        ])
        .expect("expected PATH should be valid");

        assert_eq!(configured_path, expected_path);
    }

    #[test]
    fn pi_installer_leaves_an_already_preferred_path_unchanged() {
        let current_path = std::env::join_paths([
            std::path::Path::new("/opt/homebrew/bin"),
            std::path::Path::new("/usr/bin"),
        ])
        .expect("test PATH should be valid");

        assert_eq!(
            preferred_installer_path(
                HarnessKind::Pi,
                Some(&current_path),
                Some(std::path::Path::new("/opt/homebrew/bin")),
            ),
            None
        );
        assert_eq!(
            preferred_installer_path(
                HarnessKind::Pi,
                Some(std::ffi::OsStr::new("")),
                Some(std::path::Path::new("/opt/homebrew/bin")),
            ),
            None
        );
    }

    #[test]
    fn only_pi_gets_the_homebrew_installer_path() {
        let current_path = std::env::join_paths([std::path::Path::new("/usr/bin")])
            .expect("test PATH should be valid");

        assert_eq!(
            preferred_installer_path(
                HarnessKind::ClaudeCode,
                Some(&current_path),
                Some(std::path::Path::new("/opt/homebrew/bin")),
            ),
            None
        );
        assert_eq!(
            preferred_installer_path(HarnessKind::Pi, Some(&current_path), None),
            None
        );
        assert_eq!(
            preferred_installer_path(
                HarnessKind::Pi,
                None,
                Some(std::path::Path::new("/opt/homebrew/bin")),
            ),
            None
        );
    }

    #[test]
    fn pi_installer_command_receives_the_preferred_path() {
        let current_path = std::env::join_paths([
            std::path::Path::new("/Users/nan/.nvm/versions/node/v20.19.4/bin"),
            std::path::Path::new("/usr/bin"),
        ])
        .expect("test PATH should be valid");
        let expected_path = std::env::join_paths([
            std::path::Path::new("/opt/homebrew/bin"),
            std::path::Path::new("/Users/nan/.nvm/versions/node/v20.19.4/bin"),
            std::path::Path::new("/usr/bin"),
        ])
        .expect("expected PATH should be valid");
        let mut command = std::process::Command::new("sh");

        configure_shell_installer_path(
            HarnessKind::Pi,
            &mut command,
            Some(&current_path),
            Some(std::path::Path::new("/opt/homebrew/bin")),
        );

        let configured_path = command
            .get_envs()
            .find(|(name, _)| *name == std::ffi::OsStr::new("PATH"))
            .and_then(|(_, value)| value);
        assert_eq!(configured_path, Some(expected_path.as_os_str()));
    }

    #[test]
    fn official_install_prompt_contains_the_executable_command() {
        let spec = install_spec(HarnessKind::Cline).expect("Cline should be installable");
        let command = official_install_command(spec).expect("Cline command should be available");

        assert_eq!(
            command,
            "npm install --global --allow-scripts=cline,protobufjs cline@latest"
        );

        let spec = install_spec(HarnessKind::DeepSeekHarness)
            .expect("DeepSeek Harness should be installable");
        let command =
            official_install_command(spec).expect("DeepSeek Harness command should be available");
        assert_eq!(
            command,
            "npm install --global --engine-strict --allow-scripts=@deepseek-ai/dsh-subprocess-local,koffi,node-pty,@google/genai,protobufjs @deepseek-ai/dsh@latest"
        );

        let spec = install_spec(HarnessKind::Omp).expect("OMP should be installable");
        let command = official_install_command(spec).expect("OMP command should be available");
        assert_eq!(
            command,
            "curl -fsSL https://omp.sh/install | sh -s -- --binary"
        );
    }

    #[test]
    fn deepseek_harness_declares_the_node_runtime_requirement() {
        let requirement = runtime_requirement(HarnessKind::DeepSeekHarness)
            .expect("embedded compatibility manifest should be valid")
            .expect("DeepSeek Harness should declare a runtime");

        assert_eq!(requirement.command, "node --version");
        assert_eq!(requirement.minimum_version.to_string(), "22.19.0");
    }

    #[test]
    fn runtime_hint_explains_how_to_recover_and_retry() {
        let hint = runtime_hint(
            HarnessKind::DeepSeekHarness,
            &semver::Version::new(22, 19, 0),
        );

        assert!(hint.contains("nvm install 22"));
        assert!(hint.contains("nvm use 22"));
        assert!(hint.contains("node --version"));
        assert!(hint.contains("nan dsh"));
        assert!(hint.contains("official Node.js installer"));
    }

    #[test]
    fn harnesses_with_fragile_installers_have_startup_checks() {
        assert_eq!(
            post_install_check_arguments(HarnessKind::DeepSeekHarness),
            Some(["--profile", "web", "--help"].as_slice())
        );
        assert_eq!(
            post_install_check_arguments(HarnessKind::Cline),
            Some(["--version"].as_slice())
        );
        assert_eq!(
            post_install_check_arguments(HarnessKind::Omp),
            Some(["--version"].as_slice())
        );
        assert_eq!(post_install_check_arguments(HarnessKind::ClaudeCode), None);
    }

    #[cfg(unix)]
    #[test]
    fn deepseek_post_install_check_uses_an_isolated_home() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().expect("temporary root should exist");
        let executable = root.path().join("dsh");
        let real_home = std::env::var("HOME").expect("test HOME should exist");
        assert!(!real_home.contains(['\"', '\n', '\r']));
        fs::write(
            &executable,
            format!(
                "#!/bin/sh\n[ \"$HOME\" != \"{real_home}\" ] || exit 29\nmkdir -p \"$HOME/.dsh\"\ntouch \"$HOME/.dsh/post-install-check\"\n"
            ),
        )
        .expect("fake DSH should be written");
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))
            .expect("fake DSH should be executable");

        verify_post_install_with_executable(
            HarnessKind::DeepSeekHarness,
            executable.to_string_lossy().as_ref(),
            &["--profile", "web", "--help"],
        )
        .expect("post-install check should use an isolated home");
    }

    #[test]
    fn finds_installed_executables_in_official_user_directories() {
        let directory = tempfile::tempdir().expect("temporary home should exist");
        let executable = executable_candidates(HarnessKind::OpenCode, directory.path())
            .into_iter()
            .next()
            .expect("OpenCode should have an executable candidate");
        fs::create_dir_all(executable.parent().expect("executable parent should exist"))
            .expect("OpenCode bin directory should be created");
        fs::write(&executable, "fake opencode executable")
            .expect("fake executable should be written");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))
                .expect("fake executable should be executable");
        }

        assert_eq!(
            find_executable(HarnessKind::OpenCode, directory.path()),
            Some(executable)
        );
    }

    #[cfg(unix)]
    #[test]
    fn ignores_non_executable_files_in_official_user_directories() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().expect("temporary home should exist");
        let executable = directory.path().join(".opencode/bin/opencode");
        fs::create_dir_all(executable.parent().expect("executable parent should exist"))
            .expect("OpenCode bin directory should be created");
        fs::write(&executable, "not executable").expect("fake executable should be written");
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o600))
            .expect("fake executable should not be executable");

        assert_eq!(
            find_executable(HarnessKind::OpenCode, directory.path()),
            None
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

    #[test]
    fn windows_candidates_include_npm_command_shims() {
        let home = std::path::Path::new("C:/Users/nan");
        let app_data = std::path::Path::new("C:/Users/nan/AppData/Roaming");
        let candidates = executable_candidates_for_platform(
            HarnessKind::Codex,
            home,
            true,
            Some(std::ffi::OsStr::new(".EXE;.CMD")),
            Some(app_data),
            None,
        );

        assert!(candidates.contains(&app_data.join("npm/codex.EXE")));
        assert!(candidates.contains(&app_data.join("npm/codex.CMD")));
    }

    #[test]
    fn windows_omp_candidates_include_the_official_binary_directory() {
        let home = std::path::Path::new("C:/Users/nan");
        let local_app_data = std::path::Path::new("C:/Users/nan/AppData/Local");
        let candidates = executable_candidates_for_platform(
            HarnessKind::Omp,
            home,
            true,
            Some(std::ffi::OsStr::new(".EXE;.CMD")),
            None,
            Some(local_app_data),
        );

        assert!(candidates.contains(&local_app_data.join("omp/omp.EXE")));
        assert!(candidates.contains(&local_app_data.join("omp/omp.CMD")));
    }

    #[test]
    fn deepseek_candidates_include_npm_directories() {
        let home = std::path::Path::new("/Users/nan");
        let candidates = executable_candidates_for_platform(
            HarnessKind::DeepSeekHarness,
            home,
            false,
            None,
            None,
            None,
        );

        assert!(candidates.contains(&home.join(".local/bin/dsh")));
        assert!(candidates.contains(&home.join(".npm-global/bin/dsh")));
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
