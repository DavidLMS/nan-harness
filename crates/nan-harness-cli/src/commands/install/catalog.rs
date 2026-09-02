use super::error::InstallError;
use nan_harness_core::HarnessKind;

pub(super) const CLAUDE_CODE_INSTALL_URL: &str = "https://claude.ai/install.sh";
pub(super) const CODEX_INSTALL_URL: &str = "https://chatgpt.com/codex/install.sh";
pub(super) const OPENCODE_INSTALL_URL: &str = "https://opencode.ai/install";
pub(super) const HERMES_INSTALL_URL: &str = "https://hermes-agent.nousresearch.com/install.sh";
pub(super) const PI_INSTALL_URL: &str = "https://pi.dev/install.sh";
pub(super) const OMP_INSTALL_URL: &str = "https://omp.sh/install";
pub(super) const PRIME_AGENT_INSTALL_URL: &str =
    "https://app.primeintellect.ai/prime-agent/install.sh";
pub(super) const OPENCLAW_INSTALL_URL: &str = "https://openclaw.ai/install.sh";
pub(super) const CLINE_INSTALL_URL: &str =
    "https://docs.cline.bot/getting-started/installing-cline";
pub(super) const QWEN_CODE_INSTALL_URL: &str =
    "https://qwen-code-assets.oss-cn-hangzhou.aliyuncs.com/installation/install-qwen.sh";
pub(super) const KIMI_CODE_INSTALL_URL: &str = "https://code.kimi.com/kimi-code/install.sh";
pub(super) const AIDER_INSTALL_URL: &str = "https://aider.chat/install.sh";
pub(super) const GOOSE_INSTALL_URL: &str =
    "https://github.com/aaif-goose/goose/releases/download/stable/download_cli.sh";
pub(super) const DEEPSEEK_HARNESS_INSTALL_URL: &str =
    "https://github.com/deepseek-ai/deepseek-harness";
pub(super) const FX_INSTALL_URL: &str = "https://fx.sh/setup.sh";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InstallMethod {
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
    pub(super) kind: HarnessKind,
    pub(super) display_name: &'static str,
    pub(super) official_url: &'static str,
    pub(super) unix: InstallMethod,
    pub(super) windows: Option<InstallMethod>,
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

pub(super) const fn command(
    program: &'static str,
    arguments: &'static [&'static str],
) -> InstallMethod {
    InstallMethod::Command { program, arguments }
}

pub(crate) fn install_spec(kind: HarnessKind) -> Option<&'static InstallSpec> {
    INSTALL_SPECS.iter().find(|spec| spec.kind == kind)
}

impl InstallSpec {
    pub(super) const fn kind(self) -> HarnessKind {
        self.kind
    }

    pub(super) const fn display_name(self) -> &'static str {
        self.display_name
    }

    pub(super) fn method(self) -> Result<InstallMethod, InstallError> {
        if cfg!(windows) {
            self.windows
                .ok_or(InstallError::UnsupportedPlatform(self.kind))
        } else {
            Ok(self.unix)
        }
    }
}

pub(super) fn official_install_command(spec: &InstallSpec) -> Result<String, InstallError> {
    Ok(match spec.method()? {
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

#[cfg(test)]
mod tests {
    use super::{
        AIDER_INSTALL_URL, CLAUDE_CODE_INSTALL_URL, CLINE_INSTALL_URL, CODEX_INSTALL_URL,
        DEEPSEEK_HARNESS_INSTALL_URL, FX_INSTALL_URL, GOOSE_INSTALL_URL, HERMES_INSTALL_URL,
        KIMI_CODE_INSTALL_URL, OMP_INSTALL_URL, OPENCLAW_INSTALL_URL, OPENCODE_INSTALL_URL,
        PI_INSTALL_URL, PRIME_AGENT_INSTALL_URL, QWEN_CODE_INSTALL_URL, install_spec,
        official_install_command,
    };
    use nan_harness_core::HarnessKind;

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
}
