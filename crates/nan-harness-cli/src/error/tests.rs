use super::CliError;
use crate::app::{Cli, Command, DirectHarnessRunArgs, HarnessRunArgs, WebSearchArgs};
use crate::commands::credentials::CredentialError;
use crate::commands::install::InstallError;
use crate::usage_evidence::UsageEvidenceError;
use nan_harness_core::{HarnessKind, PlanError};
use nan_harness_runtime::update::UpdateError;
use nan_harness_runtime::{BridgeError, DiscoveryError, RuntimeError};
use nan_harness_telemetry::diagnostic::DiagnosticReason;
use nan_harness_telemetry::event::REOPEN_TERMINAL_GUIDANCE_TEXT;
use semver::Version;
use std::path::PathBuf;

#[test]
fn local_runtime_preconditions_are_not_reportable() {
    let error = CliError::Install(InstallError::RuntimeUnsupported {
        harness: HarnessKind::DeepSeekHarness,
        detected: "v20.19.4".to_owned(),
        minimum: Version::new(22, 19, 0),
        hint: "actionable guidance".to_owned(),
    });

    let message = error.user_message(&dry_run_cli());
    assert_eq!(message.code, None);
    assert!(!message.is_reportable());
}

#[test]
fn installer_failures_remain_reportable() {
    let error = CliError::Install(InstallError::InstallerFailed {
        harness: HarnessKind::DeepSeekHarness,
        interpreter: "npm",
        exit_code: Some(1),
    });

    let message = error.user_message(&dry_run_cli());
    assert_eq!(message.code.as_deref(), Some("NH-INSTALL-001"));
    assert!(message.is_reportable());
}

#[test]
fn credential_guidance_is_not_reportable() {
    let message =
        CliError::Credential(CredentialError::MissingCredential).user_message(&dry_run_cli());

    assert_eq!(message.code, None);
    assert!(!message.is_reportable());
}

#[test]
fn expected_dry_run_validation_errors_are_not_reportable_to_telemetry() {
    let cli = dry_run_cli();
    let discovery = CliError::Discovery(DiscoveryError::InvalidExecutable(PathBuf::from(
        "/tmp/kimi",
    )));
    let plan = CliError::InvalidPlan(PlanError::InvalidField {
        field: "process.arguments",
        message: "argument conflicts with routing".to_owned(),
    });

    assert!(!discovery.should_report_telemetry(&cli));
    assert!(!plan.should_report_telemetry(&cli));
}

#[test]
fn missing_update_channel_is_not_reportable_to_telemetry() {
    let cli = Cli {
        command: Command::Update,
    };
    let error = CliError::Update(UpdateError::UpdateChannelUnavailable);

    assert!(!error.should_report_telemetry(&cli));
}

#[test]
fn private_usage_evidence_failures_are_generic_and_not_reportable() {
    let error = CliError::UsageEvidence(UsageEvidenceError);
    let message = error.user_message(&dry_run_cli()).render_terminal();

    assert!(!error.should_report_telemetry(&dry_run_cli()));
    assert_eq!(
        message,
        "error [NH-CLI-006]: could not write private usage evidence"
    );
    assert!(!message.contains("NAN_HARNESS_INTERNAL_CANARY_USAGE_FILE"));
    assert!(!message.contains("/private"));
}

#[tokio::test]
async fn preflight_task_failures_are_fixed_and_safely_diagnosed() {
    let task = tokio::spawn(std::future::pending::<()>());
    task.abort();
    let source = task.await.expect_err("aborted task should fail to join");
    let error = CliError::PreflightTaskFailed(source);
    let cli = dry_run_cli();
    let message = error.user_message(&cli).render_terminal();
    let context = error.telemetry_context(&cli, false, None);

    assert_eq!(error.code(), "NH-CLI-005");
    assert_eq!(
        message,
        "error [NH-CLI-005]: terminal launch preflight task failed"
    );
    assert_eq!(
        context.diagnostic_reason(),
        DiagnosticReason::InternalInvariant
    );
    assert!(!message.contains("cancelled"));
    assert!(!message.contains("JoinError"));
}

#[test]
fn real_discovery_failures_remain_reportable_during_dry_run() {
    let cli = dry_run_cli();
    let error = CliError::Discovery(DiscoveryError::VersionCommandFailed {
        command: "kimi --version".to_owned(),
        exit_code: Some(1),
    });

    assert!(error.should_report_telemetry(&cli));
}

#[test]
fn current_directory_failures_show_recovery_without_an_error_code() {
    let error =
        CliError::CurrentDirectory(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
    let message = error.user_message(&dry_run_cli());

    assert!(message.is_reportable());
    assert_eq!(message.code, None);
    assert_eq!(
        message.render_terminal(),
        "warning: The current terminal session cannot access the project directory. Please close this terminal, open a new terminal in the project directory, and try again."
    );
}

#[test]
fn current_directory_reports_include_the_exact_guidance_and_skip_discovery() {
    let error =
        CliError::CurrentDirectory(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
    let cli = Cli {
        command: Command::Pi(DirectHarnessRunArgs {
            run: HarnessRunArgs {
                model: None,
                executable: None,
                provider_base_url: None,
                allow_unsupported: false,
                allow_untested: false,
                search: WebSearchArgs::default(),
                dry_run: false,
                arguments: Vec::new(),
            },
            no_chat_gateway: false,
        }),
    };
    let context = error.telemetry_context(&cli, true, None);
    let guidance = context
        .user_guidance()
        .expect("current directory failures should include user guidance");

    assert!(guidance.shown());
    assert_eq!(guidance.id(), "reopen-terminal");
    assert_eq!(guidance.text(), REOPEN_TERMINAL_GUIDANCE_TEXT);
    assert_eq!(
        context.diagnostic_reason().as_str(),
        "filesystem-operation-failed"
    );
}

#[test]
fn unavailable_models_offer_harness_specific_recovery() {
    for (harness, command) in [
        ("claude", "nanh claude --model qwen3.6"),
        ("codex", "nanh codex --model qwen3.6"),
        ("qwen", "nanh qwen --model qwen3.6"),
        ("dsh", "nanh dsh --model qwen3.6"),
        ("fx", "nanh fx --model qwen3.6"),
    ] {
        let cli =
            Cli::try_parse_checked_from(["nanh", harness]).expect("harness command should parse");
        let error = CliError::Runtime(RuntimeError::Bridge(
            BridgeError::SelectedModelUnavailable {
                model: "qwen36".to_owned(),
                available: vec!["qwen3.6".to_owned(), "glm5.3-flash".to_owned()],
            },
        ));
        let rendered = error.user_message(&cli).render_terminal();
        assert!(rendered.contains(&format!(
            "Choose a model from your live catalog:\n  nanh doctor\n  {command}"
        )));
    }
}

#[test]
fn empty_model_catalog_recovery_only_runs_doctor() {
    let cli = Cli::try_parse_checked_from(["nanh", "codex"]).expect("Codex command should parse");
    let error = CliError::Runtime(RuntimeError::Bridge(
        BridgeError::SelectedModelUnavailable {
            model: "old-model".to_owned(),
            available: Vec::new(),
        },
    ));
    let rendered = error.user_message(&cli).render_terminal();
    assert!(rendered.ends_with("Choose a model from your live catalog:\n  nanh doctor"));
    assert!(!rendered.contains(" --model "));
}

fn dry_run_cli() -> Cli {
    Cli {
        command: Command::Kimi(DirectHarnessRunArgs {
            run: HarnessRunArgs {
                model: None,
                executable: None,
                provider_base_url: None,
                allow_unsupported: false,
                allow_untested: false,
                search: WebSearchArgs::default(),
                dry_run: true,
                arguments: Vec::new(),
            },
            no_chat_gateway: false,
        }),
    }
}
