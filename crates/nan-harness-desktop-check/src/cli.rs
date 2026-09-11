use crate::journal::Journal;
use crate::report::{Report, Status};
use clap::{Args, Parser, Subcommand};
use nan_harness_core::DesktopHarnessKind;
use std::io::{self, IsTerminal as _, Write as _};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt as _;

#[derive(Debug, Parser)]
#[command(
    name = "nanh-desktop-check",
    version,
    about = "Run opt-in Desktop compatibility checks"
)]
pub struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    #[command(flatten)]
    run: RunArgs,
}

#[derive(Debug, Subcommand)]
enum Command {
    Run(RunArgs),
    /// Install and identify test applications without opening them or making NaN calls.
    Prepare(RunArgs),
    /// Print bundled native OCR and model license notices.
    Licenses,
    /// Review and submit a sanitized report using your GitHub identity.
    Submit {
        report: PathBuf,
    },
    /// Recover only unchanged resources owned by this run.
    Cleanup {
        run_id: String,
    },
    /// Validate a public report and print its exact SHA-256 digest.
    ValidateReport {
        report: PathBuf,
    },
    /// Validate a closed wave10 occlusion diagnostic and print its digest.
    #[command(hide = true)]
    ValidateOcclusion {
        diagnostic: PathBuf,
    },
    /// Emit candidate feed evidence; publication still requires trusted release validation.
    FeedUpdates {
        report: PathBuf,
    },
    #[command(hide = true)]
    Probe {
        spec: PathBuf,
        output: PathBuf,
    },
}

#[derive(Debug, Default, Args)]
pub struct RunArgs {
    /// Test only these Desktop integrations (repeatable); defaults to all five.
    #[arg(long = "app")]
    pub apps: Vec<DesktopHarnessKind>,
    /// Retain newly installed applications and the downloaded checker on disposable machines.
    #[arg(long)]
    pub ephemeral: bool,
    /// Authorize the displayed installation and test operations, not public submission.
    #[arg(long)]
    pub yes: bool,
    /// Never prompt; operations require --yes and interactive prerequisites become blocked.
    #[arg(long)]
    pub non_interactive: bool,
    #[arg(long, default_value = "qwen3.6")]
    pub model: String,
    /// Write a sanitized report to this path, without overwriting existing files.
    #[arg(long)]
    pub output: Option<PathBuf>,
    /// Test this exact nanh executable instead of discovering an installation.
    #[arg(long)]
    pub nan_harness: Option<PathBuf>,
    /// Use the exact installations recorded by prepare; never download or install.
    #[arg(long)]
    pub prepared: Option<PathBuf>,
    /// Select deterministic checks, required live checks, or both when a key is present.
    #[arg(long, value_enum, default_value_t = ExecutionMode::Auto)]
    pub mode: ExecutionMode,
    /// Declare a fresh GitHub-hosted VM with no personal data; never use on a personal session.
    #[arg(long, value_enum, default_value_t = SessionMode::PrivateProfile)]
    pub session: SessionMode,
    /// Temporary Linux startup diagnostic: launch only `chatgpt-desktop`
    /// through this wrapper. Help, version and restoration still run the tested nanh.
    #[arg(
        long,
        hide = true,
        requires_all = ["launch_wrapper_sha256", "launch_wrapper_facts"]
    )]
    pub launch_wrapper: Option<PathBuf>,
    /// The wrapper's own SHA-256; the tested nanh identity stays authoritative.
    #[arg(long, hide = true, requires = "launch_wrapper")]
    pub launch_wrapper_sha256: Option<String>,
    /// Existing owner-only directory for the wrapper's closed per-probe facts.
    #[arg(long, hide = true, requires = "launch_wrapper")]
    pub launch_wrapper_facts: Option<PathBuf>,
}

#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    clap::ValueEnum,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum SessionMode {
    #[default]
    PrivateProfile,
    GithubHosted,
}

impl SessionMode {
    pub(crate) fn available(self) -> bool {
        self.accepts_environment(
            std::env::var("GITHUB_ACTIONS").ok().as_deref(),
            std::env::var("RUNNER_ENVIRONMENT").ok().as_deref(),
        )
    }

    fn accepts_environment(self, actions: Option<&str>, environment: Option<&str>) -> bool {
        self == Self::PrivateProfile
            || (actions == Some("true") && environment == Some("github-hosted"))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum ExecutionMode {
    #[default]
    Auto,
    Deterministic,
    Live,
}

/// Execute one command, printing only closed errors at the outer boundary.
///
/// # Errors
/// Returns a safe error description when an operation cannot finish.
pub async fn execute() -> Result<i32, String> {
    let cli = Cli::parse();
    match cli.command {
        None => crate::runner::run(cli.run).await,
        Some(Command::Run(args)) => crate::runner::run(args).await,
        Some(Command::Prepare(args)) => crate::runner::prepare(args).await,
        Some(Command::Licenses) => {
            print!("{}", include_str!("../native/THIRD_PARTY_NOTICES.txt"));
            Ok(0)
        }
        Some(Command::ValidateReport { report }) => {
            let (_, digest) = Report::read(&report).map_err(|error| error.to_string())?;
            println!("{digest}");
            Ok(0)
        }
        Some(Command::ValidateOcclusion { diagnostic }) => {
            let (_, digest) = crate::occlusion::OcclusionDiagnostic::read(&diagnostic).map_err(
                |_| "occlusion diagnostic cannot be read or failed allowlist/schema validation",
            )?;
            println!("{digest}");
            Ok(0)
        }
        Some(Command::FeedUpdates { report }) => {
            let (report, _) = Report::read(&report).map_err(|error| error.to_string())?;
            let updates = feed_updates(&report)?;
            println!(
                "{}",
                serde_json::to_string(&updates).map_err(|_| "cannot encode evidence")?
            );
            Ok(0)
        }
        Some(Command::Cleanup { run_id }) => {
            let mut journal =
                Journal::open(&state_directory()?, &run_id).map_err(|error| error.to_string())?;
            crate::probe::recover_pending(&mut journal).await?;
            journal.cleanup(false).map_err(|error| error.to_string())?;
            println!("Owned installations cleaned up. The recovery receipt is retained.");
            Ok(0)
        }
        Some(Command::Submit { report }) => submit(&report).await,
        Some(Command::Probe { spec, output }) => crate::probe::run_worker(&spec, &output).await,
    }
}

pub(crate) fn state_directory() -> Result<PathBuf, String> {
    let variable = if cfg!(windows) {
        "LOCALAPPDATA"
    } else {
        "XDG_STATE_HOME"
    };
    if let Some(path) = std::env::var_os(variable) {
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            return Err("the state directory must be absolute".into());
        }
        return Ok(path.join("nanh-desktop-check"));
    }
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or("a usable user directory is required")?;
    Ok(home.join(".local/state/nanh-desktop-check"))
}

pub(crate) fn confirm(message: &str) -> Result<bool, String> {
    if !io::stdin().is_terminal() {
        return Ok(false);
    }
    eprint!("{message} [y/N] ");
    io::stderr()
        .flush()
        .map_err(|_| "cannot display confirmation")?;
    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .map_err(|_| "cannot read confirmation")?;
    Ok(answer.trim().eq_ignore_ascii_case("y") || answer.trim().eq_ignore_ascii_case("yes"))
}

fn feed_updates(report: &Report) -> Result<serde_json::Value, String> {
    let binary = report
        .nan_harness
        .as_ref()
        .ok_or("report has no tested nanh identity")?;
    let mut updates = Vec::new();
    for app in &report.results {
        let Some(version) = &app.app_version else {
            continue;
        };
        if app.cleanup != Status::Passed || report.cleanup != Status::Passed {
            continue;
        }
        let deterministic = app
            .deterministic
            .iter()
            .all(|probe| probe.status == Status::Passed);
        let live = app.live.status == Status::Passed;
        if !deterministic && !live {
            continue;
        }
        let mut update = serde_json::json!({
            "id": app.app, "platform": report.platform, "architecture": report.architecture,
            "appVersion": version,
        });
        if deterministic {
            update["deterministicAt"] = serde_json::json!(report.started_at);
        }
        if let Some(runtime) = &app.runtime_version {
            update["runtimeVersion"] = serde_json::json!(runtime);
        }
        if live {
            update["liveVerifiedAt"] = serde_json::json!(report.started_at);
        }
        updates.push(update);
    }
    Ok(
        serde_json::json!({"nanHarnessVersion":binary.version,"verifications":[],"desktopChecks":updates}),
    )
}

async fn submit(path: &Path) -> Result<i32, String> {
    let (report, digest) = Report::read(path).map_err(|error| error.to_string())?;
    // Re-encode only validated fields; bind the issue digest to exactly that encoding.
    let bytes = serde_json::to_vec_pretty(&report).map_err(|_| "cannot encode public report")?;
    let public_digest = crate::report::digest(&bytes);
    let body = format!(
        "Desktop compatibility report\n\nReport SHA-256: `{public_digest}`\n\n```nanh-desktop-report\n{}\n```\n",
        String::from_utf8(bytes).map_err(|_| "invalid public report")?
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&report).map_err(|_| "cannot display report")?
    );
    eprintln!("Original report SHA-256: {digest}");
    if !confirm("Publish this report in a public GitHub issue?")? {
        return Ok(0);
    }
    let title = format!("Desktop compatibility: {}", report.run_id);
    let Ok(mut child) = tokio::process::Command::new("gh")
        .env_remove("NAN_API_KEY")
        .args([
            "issue",
            "create",
            "--repo",
            "DavidLMS/nan-harness",
            "--title",
            &title,
            "--body-file",
            "-",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
    else {
        return browser_submission(&body, &title);
    };
    child
        .stdin
        .take()
        .ok_or("cannot send report")?
        .write_all(body.as_bytes())
        .await
        .map_err(|_| "cannot send report")?;
    let status = tokio::time::timeout(std::time::Duration::from_secs(45), child.wait())
        .await
        .map_err(|_| "GitHub submission timed out; check existing issues before retrying")?
        .map_err(|_| "GitHub submission failed")?;
    if !status.success() {
        return Err(
            "GitHub submission failed; authenticate gh or submit the report in your browser".into(),
        );
    }
    Ok(0)
}

fn browser_submission(body: &str, title: &str) -> Result<i32, String> {
    let mut url = url::Url::parse("https://github.com/DavidLMS/nan-harness/issues/new")
        .map_err(|_| "cannot create submission link")?;
    if body.len() <= 6000 {
        url.query_pairs_mut()
            .append_pair("title", title)
            .append_pair("body", body);
    } else {
        eprintln!(
            "The report is too large for a browser link. Paste the following block into the issue:"
        );
        println!("{body}");
    }
    let mut command = if cfg!(target_os = "macos") {
        std::process::Command::new("open")
    } else if cfg!(windows) {
        let mut command = std::process::Command::new("rundll32.exe");
        command.arg("url.dll,FileProtocolHandler");
        command
    } else {
        std::process::Command::new("xdg-open")
    };
    command
        .env_remove("NAN_API_KEY")
        .arg(url.as_str())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|_| "open github.com/DavidLMS/nan-harness/issues/new to submit your report")?;
    eprintln!(
        "Review and submit the issue in your browser. It has not been published by the checker."
    );
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory as _;

    #[test]
    fn hosted_sessions_require_explicit_selection_and_hosted_runner_metadata() {
        let cli = Cli::try_parse_from(["nanh-desktop-check", "--yes", "--ephemeral"]).unwrap();
        assert_eq!(cli.run.session, SessionMode::PrivateProfile);
        let hosted = SessionMode::GithubHosted;
        assert!(hosted.accepts_environment(Some("true"), Some("github-hosted")));
        for (actions, environment) in [
            (None, None),
            (Some("true"), None),
            (None, Some("github-hosted")),
            (Some("true"), Some("self-hosted")),
            (Some("false"), Some("github-hosted")),
        ] {
            assert!(!hosted.accepts_environment(actions, environment));
        }
        assert!(SessionMode::PrivateProfile.accepts_environment(None, None));
        assert!(Cli::try_parse_from(["nanh-desktop-check", "--session", "personal"]).is_err());
    }

    #[tokio::test]
    async fn a_hosted_session_needs_separate_operation_authorization_before_inventory() {
        let args = RunArgs {
            session: SessionMode::GithubHosted,
            ..RunArgs::default()
        };
        assert!(
            crate::runner::run(args)
                .await
                .unwrap_err()
                .contains("requires --yes")
        );
    }

    #[test]
    fn cli_has_no_key_argument_and_retention_does_not_authorize_operations() {
        Cli::command().debug_assert();
        let cli = Cli::try_parse_from(["nanh-desktop-check", "--ephemeral"]).unwrap();
        assert!(cli.run.ephemeral);
        assert!(!cli.run.yes);
        assert!(Cli::try_parse_from(["nanh-desktop-check", "--nan-api-key", "secret"]).is_err());
        assert!(Cli::try_parse_from(["nanh-desktop-check", "run", "--app", "zed"]).is_ok());
    }

    #[test]
    fn the_launch_wrapper_is_hidden_and_bound_as_a_complete_triple() {
        let digest = "a".repeat(64);
        for partial in [
            vec!["--launch-wrapper", "/wrapper"],
            vec![
                "--launch-wrapper",
                "/wrapper",
                "--launch-wrapper-sha256",
                &digest,
            ],
            vec![
                "--launch-wrapper",
                "/wrapper",
                "--launch-wrapper-facts",
                "/facts",
            ],
            vec!["--launch-wrapper-sha256", &digest],
            vec!["--launch-wrapper-facts", "/facts"],
        ] {
            let argv = std::iter::once("nanh-desktop-check").chain(partial);
            assert!(Cli::try_parse_from(argv).is_err());
        }
        let cli = Cli::try_parse_from([
            "nanh-desktop-check",
            "run",
            "--launch-wrapper",
            "/wrapper",
            "--launch-wrapper-sha256",
            &digest,
            "--launch-wrapper-facts",
            "/facts",
        ])
        .unwrap();
        let Some(Command::Run(args)) = cli.command else {
            panic!("the run subcommand must accept the diagnostic binding");
        };
        assert_eq!(args.launch_wrapper.as_deref(), Some(Path::new("/wrapper")));
        assert!(
            !Cli::command()
                .render_long_help()
                .to_string()
                .contains("launch-wrapper")
        );
        let normal = Cli::try_parse_from(["nanh-desktop-check", "--yes"]).unwrap();
        assert!(normal.run.launch_wrapper.is_none());
    }

    fn passing_report() -> Report {
        use crate::report::{
            AppResult, Architecture, BinaryIdentity, CheckStep, InputMode, Platform, ProbeResult,
        };
        let probe = ProbeResult {
            status: Status::Passed,
            reason: None,
            steps: vec![
                CheckStep::Launched,
                CheckStep::InputSubmitted,
                CheckStep::ResponseVerified,
                CheckStep::ToolVerified,
                CheckStep::ErrorRecovered,
            ],
            input_mode: Some(InputMode::Accessibility),
            gui_stage: None,
            response_verification: None,
            duration_milliseconds: 1,
        };
        Report {
            schema_version: 1,
            checker_version: "0.1.0".parse().unwrap(),
            run_id: "a".repeat(32),
            started_at: "2026-09-08T00:00:00Z".into(),
            platform: Platform::Linux,
            architecture: Architecture::X86_64,
            nan_harness: Some(BinaryIdentity {
                version: "0.1.2".parse().unwrap(),
                sha256: "b".repeat(64),
            }),
            results: vec![AppResult {
                app: DesktopHarnessKind::Zed,
                app_version: Some("1.19.0".parse().unwrap()),
                runtime_version: None,
                deterministic: std::array::from_fn(|_| probe.clone()),
                live: probe,
                cleanup: Status::Passed,
            }],
            cleanup: Status::Passed,
        }
    }

    #[test]
    fn conversion_keeps_live_and_deterministic_success_independent() {
        use crate::report::{ProbeResult, Reason};
        let mut report = passing_report();
        report.results[0].deterministic[1] = ProbeResult::blocked(Reason::ProviderFailed);
        report.validate().unwrap();
        let update = feed_updates(&report).unwrap();
        assert!(update["desktopChecks"][0].get("deterministicAt").is_none());
        assert_eq!(
            update["desktopChecks"][0]["liveVerifiedAt"],
            report.started_at
        );
        assert_eq!(update["nanHarnessVersion"], "0.1.2");

        let mut report = passing_report();
        report.results[0].live = ProbeResult::not_run(Reason::MissingKey);
        report.validate().unwrap();
        let update = feed_updates(&report).unwrap();
        assert_eq!(
            update["desktopChecks"][0]["deterministicAt"],
            report.started_at
        );
        assert!(update["desktopChecks"][0].get("liveVerifiedAt").is_none());
    }

    #[test]
    fn conversion_requires_cleanup_and_identified_app_for_either_track() {
        for (app_cleanup, run_cleanup, version) in [
            (Status::Failed, Status::Passed, Some("1.19.0")),
            (Status::Passed, Status::Failed, Some("1.19.0")),
            (Status::Passed, Status::Passed, None),
        ] {
            let mut report = passing_report();
            report.results[0].cleanup = app_cleanup;
            report.cleanup = run_cleanup;
            report.results[0].app_version = version.map(|value| value.parse().unwrap());
            assert_eq!(
                feed_updates(&report).unwrap()["desktopChecks"],
                serde_json::json!([])
            );
        }
    }
}
