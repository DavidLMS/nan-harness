use crate::app::ClaudeDesktopArgs;
use crate::commands::credentials;
use crate::commands::persistence::PersistenceManager;
use crate::error::CliError;
use nan_harness_core::{DesktopHarnessKind, DesktopLaunchPlan, DesktopTransport, WebSearchPolicy};
use nan_harness_private_fs::open_private_new;
use nan_harness_runtime::{
    BridgeActivity, BridgeDiagnostic, ClaudeAutoModeReviewStage, DesktopCompatibilityEvidence,
    DesktopCompatibilityStatus, RunningClaudeDesktopBridge, classify_desktop_version,
    desktop_compatibility, start_claude_desktop_bridge,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest as _, Sha256};
use std::fs::{self, File, OpenOptions, Permissions, TryLockError};
use std::future::Future;
use std::io::{ErrorKind, Write as _};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::process::Stdio;
use std::time::Duration;
use tempfile::Builder as TempFileBuilder;
use thiserror::Error;

const PROFILE_ID: &str = "6e616e68-6172-4e65-8000-000000000001";
const PROFILE_NAME: &str = "NaN Harness";
const RECEIPT_SCHEMA: u8 = 2;
const DOCUMENT_IDS: [&str; 4] = [
    "normal-config",
    "third-party-config",
    "profile-meta",
    "profile",
];

mod configuration;
mod orchestration;
mod paths;
mod process;
mod session;

#[allow(clippy::wildcard_imports)]
use configuration::*;
#[allow(clippy::wildcard_imports)]
use orchestration::*;
#[allow(clippy::wildcard_imports)]
use paths::*;
#[allow(clippy::wildcard_imports)]
use process::*;
#[allow(clippy::wildcard_imports)]
use session::*;

pub(crate) async fn run(
    arguments: &ClaudeDesktopArgs,
    interactive: bool,
    bridge_diagnostics: &mut Vec<BridgeDiagnostic>,
) -> Result<i32, CliError> {
    if arguments.dry_run {
        return print_dry_run(arguments);
    }
    let compatibility =
        desktop_compatibility(DesktopHarnessKind::Claude).map_err(ClaudeDesktopError::from)?;
    match classify_desktop_version(&compatibility, None) {
        DesktopCompatibilityStatus::ContractOnly => eprintln!(
            "warning: Claude Desktop compatibility on this platform is based on deterministic contracts, not a live verification"
        ),
        DesktopCompatibilityStatus::Unavailable => {
            return Err(ClaudeDesktopError::UnsupportedPlatform.into());
        }
        DesktopCompatibilityStatus::Tested
        | DesktopCompatibilityStatus::NewerUntested
        | DesktopCompatibilityStatus::OlderUnsupported => {}
    }
    debug_assert_ne!(
        compatibility.evidence,
        DesktopCompatibilityEvidence::Unavailable
    );
    let manager = PersistenceManager::from_environment()?;
    let remembered_model = if arguments.model.is_none() {
        manager
            .last_desktop_selection(DesktopHarnessKind::Claude)?
            .map(|selection| selection.model)
    } else {
        None
    };
    let requested_model = arguments.model.as_deref().or(remembered_model.as_deref());
    let platform = DesktopPlatform::current()?;
    let paths = DesktopPaths::from_environment(platform)?;
    let process = SystemDesktopProcess::new(platform, arguments.executable.clone());
    if arguments.restore {
        return restore_command(&paths, &process);
    }
    let _lock = prepare_session_lock(&paths, &process)?;
    ensure_no_pending_recovery(&paths)?;
    if process.is_running()? {
        return Err(ClaudeDesktopError::AlreadyRunning.into());
    }
    let mut config =
        credentials::resolve_or_onboard(arguments.provider_base_url.clone(), interactive).await?;
    let discovered_models = config.model_catalog.take();
    let bridge = start_claude_desktop_bridge(
        &config.config,
        discovered_models,
        requested_model,
        arguments.show_auto,
        !arguments.search.no_search,
    )
    .await
    .map_err(ClaudeDesktopError::from)?;
    let selected_model = bridge.selected_model().to_owned();
    let result = run_ready_session(&paths, &process, &bridge, arguments.show_auto).await;
    let shutdown = bridge.shutdown_with_usage().await;
    match (result, shutdown) {
        (Err(error), _) => Err(error.into()),
        (Ok(code), Ok((diagnostics, usage))) => {
            append_diagnostics(bridge_diagnostics, diagnostics);
            if let Err(error) =
                manager.save_last_desktop_selection(DesktopHarnessKind::Claude, &selected_model)
            {
                eprintln!("warning: could not save the last Desktop model: {error}");
            }
            let outcome = if code == 0 {
                nan_harness_runtime::ExecutionOutcome::Succeeded
            } else {
                nan_harness_runtime::ExecutionOutcome::Failed
            };
            if let Some(summary) = crate::usage_summary::render_snapshot(&usage, outcome) {
                eprintln!("{summary}");
            }
            Ok(code)
        }
        (Ok(_), Err(error)) => Err(ClaudeDesktopError::Bridge(error).into()),
    }
}

#[derive(Debug, Error)]
pub(crate) enum ClaudeDesktopError {
    #[error("Claude Desktop integration is available only on macOS, Linux, and Windows")]
    UnsupportedPlatform,
    #[error(transparent)]
    Compatibility(#[from] nan_harness_runtime::DesktopCompatibilityError),
    #[error(
        "Claude Desktop is already running; quit it completely, then re-run `nanh claude-desktop`"
    )]
    AlreadyRunning,
    #[error("another `nanh claude-desktop` session is active")]
    ConcurrentSession,
    #[error(
        "an interrupted Claude Desktop session needs recovery; run `nanh claude-desktop --restore`"
    )]
    OrphanReceipt,
    #[error("no interrupted Claude Desktop configuration receipt was found")]
    NoReceipt,
    #[error("Claude Desktop did not start; its original configuration has been restored")]
    DidNotStart,
    #[error(
        "Claude Desktop did not quit, so its configuration was not restored; quit it completely, then run `nanh claude-desktop --restore`"
    )]
    DidNotTerminate,
    #[error(
        "Claude Desktop was not found for {platform}; install the official app from https://support.claude.com/es/articles/10065433-instalar-claude-desktop"
    )]
    AppNotFound { platform: &'static str },
    #[error(transparent)]
    Bridge(#[from] nan_harness_runtime::ClaudeDesktopBridgeError),
    #[error("could not determine the current user's home directory")]
    MissingHome,
    #[error("could not resolve the current user's {0} directory")]
    MissingPlatformDirectory(&'static str),
    #[error("Claude Desktop state path is invalid")]
    InvalidStatePath,
    #[error("Claude Desktop managed state contains an unsafe symbolic link")]
    UnsafeSymlink,
    #[error("could not create a configuration directory: {0}")]
    CreateDirectory(std::io::Error),
    #[error("could not protect private Claude Desktop state: {0}")]
    Permissions(std::io::Error),
    #[error("could not lock the Claude Desktop integration: {0}")]
    Lock(std::io::Error),
    #[error("could not check whether Claude Desktop is running: {0}")]
    ProcessCheck(std::io::Error),
    #[error("the Claude Desktop process check failed with exit code {0:?}")]
    ProcessCheckFailed(Option<i32>),
    #[error("could not launch Claude Desktop: {0}")]
    Launch(std::io::Error),
    #[error("Claude Desktop launcher failed with exit code {0:?}")]
    LaunchFailed(Option<i32>),
    #[error(
        "could not terminate Claude Desktop, so its configuration was not restored; quit it completely, then run `nanh claude-desktop --restore`: {0}"
    )]
    Terminate(std::io::Error),
    #[error(
        "Claude Desktop termination failed with exit code {0:?}, so its configuration was not restored; quit it completely, then run `nanh claude-desktop --restore`"
    )]
    TerminateFailed(Option<i32>),
    #[error("could not read Claude Desktop configuration: {0}")]
    ReadConfig(std::io::Error),
    #[error("Claude Desktop configuration is not valid JSON: {0}")]
    ParseConfig(serde_json::Error),
    #[error("Claude Desktop configuration root must be an object")]
    ConfigRoot,
    #[error("could not serialize Claude Desktop configuration: {0}")]
    SerializeConfig(serde_json::Error),
    #[error("could not write Claude Desktop configuration: {0}")]
    Write(std::io::Error),
    #[error("could not restore Claude Desktop configuration: {0}")]
    Restore(std::io::Error),
    #[error(
        "an orphaned Claude Desktop backup exists; inspect the private state directory before retrying"
    )]
    OrphanBackup,
    #[error("could not create the private Claude Desktop backup directory: {0}")]
    CreateBackupDirectory(std::io::Error),
    #[error("could not write a private Claude Desktop backup: {0}")]
    WriteBackup(std::io::Error),
    #[error("could not read a private Claude Desktop backup: {0}")]
    ReadBackup(std::io::Error),
    #[error("a private Claude Desktop backup does not match its receipt hash")]
    BackupHashMismatch,
    #[error("could not remove private Claude Desktop backups: {0}")]
    RemoveBackup(std::io::Error),
    #[error("could not serialize the private Claude Desktop receipt: {0}")]
    SerializeReceipt(serde_json::Error),
    #[error("could not read the private Claude Desktop receipt: {0}")]
    ReadReceipt(std::io::Error),
    #[error("the private Claude Desktop receipt is invalid: {0}")]
    ParseReceipt(serde_json::Error),
    #[error("the private Claude Desktop receipt schema is not supported")]
    UnsupportedReceipt,
    #[error("could not remove the restored Claude Desktop receipt: {0}")]
    RemoveReceipt(std::io::Error),
}

impl ClaudeDesktopError {
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::Bridge(error) => error.code(),
            Self::AlreadyRunning
            | Self::ConcurrentSession
            | Self::OrphanReceipt
            | Self::OrphanBackup
            | Self::UnsafeSymlink => "NH-DESKTOP-002",
            Self::UnsupportedPlatform
            | Self::AppNotFound { .. }
            | Self::Compatibility(
                nan_harness_runtime::DesktopCompatibilityError::Unavailable
                | nan_harness_runtime::DesktopCompatibilityError::MissingPlatform,
            ) => "NH-DESKTOP-003",
            _ => "NH-DESKTOP-001",
        }
    }
}

// Keep lifecycle tests in the parent module so they continue exercising the
// same private surface while this responsibility-only split stays focused.
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    fn paths() -> (tempfile::TempDir, DesktopPaths) {
        let root = tempfile::tempdir().expect("temp root");
        let paths = DesktopPaths::new(
            &root.path().join("Claude"),
            &root.path().join("Claude-3p"),
            &root.path().join("state"),
        );
        (root, paths)
    }

    #[test]
    fn dry_run_plan_preserves_model_executable_diagnostics_and_search_policy() {
        let arguments = ClaudeDesktopArgs {
            model: Some("qwen3.6".to_owned()),
            provider_base_url: None,
            executable: Some(PathBuf::from("/tmp/claude")),
            allow_unsupported: false,
            allow_untested: false,
            search: crate::app::WebSearchArgs {
                no_search: false,
                force_search: true,
            },
            dry_run: true,
            show_auto: true,
            restore: false,
        };

        let plan = dry_run_plan(&arguments);

        assert_eq!(plan.harness, DesktopHarnessKind::Claude);
        assert_eq!(plan.transport, DesktopTransport::AnthropicBridge);
        assert_eq!(plan.executable, arguments.executable);
        assert_eq!(plan.selected_model, arguments.model);
        assert_eq!(plan.web_search_policy, WebSearchPolicy::Force);
        assert!(plan.private_diagnostics);
    }

    #[test]
    fn macos_paths_use_application_support_and_accept_a_nan_override() {
        let environment = DesktopEnvironment {
            home: Some(PathBuf::from("/Users/tester")),
            nan_config: Some(PathBuf::from("/private/nan")),
            ..DesktopEnvironment::default()
        };

        let paths = DesktopPaths::from_platform_environment(DesktopPlatform::Macos, &environment)
            .expect("macOS paths");

        assert_eq!(
            paths.normal_config,
            PathBuf::from(
                "/Users/tester/Library/Application Support/Claude/claude_desktop_config.json"
            )
        );
        assert_eq!(
            paths.profile,
            PathBuf::from(
                "/Users/tester/Library/Application Support/Claude-3p/configLibrary/6e616e68-6172-4e65-8000-000000000001.json"
            )
        );
        assert_eq!(
            paths.receipt,
            PathBuf::from("/private/nan/claude-desktop-receipt.json")
        );
    }

    #[test]
    fn linux_paths_follow_xdg_config_home() {
        let environment = DesktopEnvironment {
            home: Some(PathBuf::from("/home/tester")),
            xdg_config: Some(PathBuf::from("/var/lib/tester/config")),
            ..DesktopEnvironment::default()
        };

        let paths = DesktopPaths::from_platform_environment(DesktopPlatform::Linux, &environment)
            .expect("Linux XDG paths");

        assert_eq!(
            paths.normal_config,
            PathBuf::from("/var/lib/tester/config/Claude/claude_desktop_config.json")
        );
        assert_eq!(
            paths.third_party_config,
            PathBuf::from("/var/lib/tester/config/Claude-3p/claude_desktop_config.json")
        );
        assert_eq!(
            paths.lock,
            PathBuf::from("/var/lib/tester/config/nan-harness/claude-desktop.lock")
        );
    }

    #[test]
    fn linux_paths_fall_back_to_the_home_config_directory() {
        let environment = DesktopEnvironment {
            home: Some(PathBuf::from("/home/tester")),
            ..DesktopEnvironment::default()
        };

        let paths = DesktopPaths::from_platform_environment(DesktopPlatform::Linux, &environment)
            .expect("Linux home paths");

        assert_eq!(
            paths.normal_config,
            PathBuf::from("/home/tester/.config/Claude/claude_desktop_config.json")
        );
        assert_eq!(
            paths.profile,
            PathBuf::from(
                "/home/tester/.config/Claude-3p/configLibrary/6e616e68-6172-4e65-8000-000000000001.json"
            )
        );
    }

    #[test]
    fn windows_paths_separate_roaming_standard_state_from_local_third_party_state() {
        let environment = DesktopEnvironment {
            app_data: Some(PathBuf::from("roaming")),
            local_app_data: Some(PathBuf::from("local")),
            ..DesktopEnvironment::default()
        };

        let paths = DesktopPaths::from_platform_environment(DesktopPlatform::Windows, &environment)
            .expect("Windows paths");

        assert_eq!(
            paths.normal_config,
            PathBuf::from("roaming/Claude/claude_desktop_config.json")
        );
        assert_eq!(
            paths.third_party_config,
            PathBuf::from("local/Claude-3p/claude_desktop_config.json")
        );
        assert_eq!(
            paths.receipt,
            PathBuf::from("roaming/nan-harness/claude-desktop-receipt.json")
        );
    }

    #[test]
    fn windows_tasklist_detection_ignores_localized_empty_output() {
        assert!(!tasklist_reports_desktop(
            b"INFO: No tasks are running which match the specified criteria.\r\n"
        ));
        assert!(tasklist_reports_desktop(
            b"\"Claude.exe\",\"2312\",\"Console\",\"1\",\"100,000 K\"\r\n"
        ));
        assert!(tasklist_reports_desktop(
            b"\"claude.exe\",\"2312\",\"Console\",\"1\",\"100,000 K\"\r\n"
        ));
    }

    #[test]
    fn auto_mode_activity_renders_the_provider_request() {
        let message = render_bridge_activity(&BridgeActivity::ClaudeAutoModeReview {
            review_id: 7,
            stage: ClaudeAutoModeReviewStage::Initial,
            model_id: "qwen3.6".to_owned(),
            request: nan_harness_runtime::ClaudeAutoModeTracePayload::new(
                r#"{"model":"qwen3.6","temperature":0}"#,
            ),
        });

        assert_eq!(
            message,
            concat!(
                "[Auto #7] Claude requested a permission review (stage 1, classifier qwen3.6).\n",
                "[Auto #7] NaN request:\n",
                "{\n  \"model\": \"qwen3.6\",\n  \"temperature\": 0\n}"
            )
        );
    }

    #[test]
    fn auto_mode_response_pretty_prints_json_and_preserves_non_json_bodies() {
        let response = render_bridge_activity(&BridgeActivity::ClaudeAutoModeReviewResponse {
            review_id: 7,
            status: 200,
            response: nan_harness_runtime::ClaudeAutoModeTracePayload::new(
                r#"{"choices":[{"message":{"content":"reviewed"}}]}"#,
            ),
        });
        assert!(response.contains("[Auto #7] NaN response (HTTP 200):"));
        assert!(response.contains("\"content\": \"reviewed\""));

        let plain_text = "provider response body\n";
        let response = render_bridge_activity(&BridgeActivity::ClaudeAutoModeReviewResponse {
            review_id: 8,
            status: 200,
            response: nan_harness_runtime::ClaudeAutoModeTracePayload::new(plain_text),
        });
        assert!(response.ends_with(plain_text));
    }

    #[test]
    fn auto_mode_failure_is_correlated_without_transport_details() {
        let message = render_bridge_activity(&BridgeActivity::ClaudeAutoModeReviewFailed {
            review_id: 9,
            error_code: "NH-BRIDGE-103",
        });

        assert_eq!(
            message,
            "[Auto #9] NaN request failed before a response was received (NH-BRIDGE-103)."
        );
    }

    #[test]
    fn launch_message_mentions_auto_only_when_tracing_is_enabled() {
        assert_eq!(
            launch_message(false),
            "Claude Desktop launched through NaN."
        );
        assert!(!launch_message(false).contains("Auto"));
        assert!(launch_message(true).contains("Auto traces will appear here"));
        assert!(launch_message(true).contains("private data"));
    }

    #[test]
    fn apply_preserves_unknown_fields_and_restore_is_exact() {
        let (_root, paths) = paths();
        fs::create_dir_all(paths.normal_config.parent().expect("parent")).expect("dir");
        fs::write(
            &paths.normal_config,
            b"{\"unknown\":{\"kept\":true},\"deploymentMode\":\"1p\"}\n",
        )
        .expect("original");
        let original = fs::read(&paths.normal_config).expect("read original");
        let receipt = Receipt::capture(&paths).expect("capture");
        receipt.write(&paths.receipt).expect("receipt");
        apply_gateway(&paths, "http://127.0.0.1:1234", "session-only").expect("apply");
        let active: Value =
            serde_json::from_slice(&fs::read(&paths.normal_config).expect("read active"))
                .expect("json");
        assert_eq!(active["unknown"]["kept"], true);
        let active_profile: Value =
            serde_json::from_slice(&fs::read(&paths.profile).expect("read active profile"))
                .expect("profile json");
        assert_eq!(active_profile["modelDiscoveryEnabled"], true);
        assert_eq!(active_profile["autoModeEnabled"], true);
        restore_receipt(&paths).expect("restore");
        assert_eq!(
            fs::read(&paths.normal_config).expect("read restored"),
            original
        );
        assert!(!paths.profile.exists());
    }

    #[test]
    fn receipt_json_never_contains_backed_up_config_or_provider_key() {
        let (_root, paths) = paths();
        let provider_key = "real-provider-secret";
        fs::create_dir_all(paths.profile.parent().expect("parent")).expect("profile directory");
        fs::write(
            &paths.profile,
            format!(r#"{{"inferenceGatewayApiKey":"{provider_key}","unknown":true}}"#),
        )
        .expect("original profile");
        let receipt = Receipt::capture(&paths).expect("capture");
        receipt.write(&paths.receipt).expect("receipt");
        apply_gateway(&paths, "http://127.0.0.1:1234", "session-token").expect("apply");
        let receipt_text = fs::read_to_string(&paths.receipt).expect("receipt text");
        assert!(
            !receipt_text.contains(provider_key),
            "receipt metadata copied original configuration contents"
        );
        assert!(!receipt_text.contains("inferenceGatewayApiKey"));
        assert!(!receipt_text.contains("session-token"));
        assert!(
            !fs::read_to_string(&paths.profile)
                .expect("profile text")
                .contains(provider_key)
        );
        assert!(
            fs::read_to_string(&paths.profile)
                .expect("profile text")
                .contains("session-token")
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let backup = paths.backup_directory.join("document-3.backup");
            assert_eq!(
                fs::metadata(backup)
                    .expect("backup metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn stale_receipt_recovers_all_documents() {
        let (_root, paths) = paths();
        fs::create_dir_all(paths.meta.parent().expect("parent")).expect("dir");
        fs::write(&paths.meta, b"{\"before\":1}").expect("original");
        Receipt::capture(&paths)
            .expect("capture")
            .write(&paths.receipt)
            .expect("receipt");
        fs::write(&paths.meta, b"{\"after\":2}").expect("changed");
        restore_receipt(&paths).expect("restore");
        assert_eq!(fs::read(&paths.meta).expect("restored"), b"{\"before\":1}");
        assert!(!paths.receipt.exists());
    }

    #[test]
    fn normal_start_rejects_orphan_backup_without_deleting_it() {
        let (_root, paths) = paths();
        fs::create_dir_all(&paths.backup_directory).expect("backup directory");
        let sentinel = paths.backup_directory.join("inspect-me.backup");
        fs::write(&sentinel, b"recoverable configuration").expect("orphan backup");

        let error = ensure_no_pending_recovery(&paths).expect_err("orphan should block startup");

        assert!(matches!(error, ClaudeDesktopError::OrphanBackup));
        assert_eq!(
            fs::read(sentinel).expect("orphan backup should remain"),
            b"recoverable configuration"
        );
    }

    #[test]
    fn restore_reports_orphan_backup_when_receipt_is_missing() {
        let (_root, paths) = paths();
        fs::create_dir_all(&paths.backup_directory).expect("backup directory");
        let sentinel = paths.backup_directory.join("inspect-me.backup");
        fs::write(&sentinel, b"recoverable configuration").expect("orphan backup");

        let error = restore_receipt(&paths).expect_err("orphan should require inspection");

        assert!(matches!(error, ClaudeDesktopError::OrphanBackup));
        assert!(sentinel.exists(), "orphan backup should remain recoverable");
    }

    #[test]
    fn session_lock_rejects_concurrency() {
        let (_root, paths) = paths();
        let _first = SessionLock::acquire(&paths.lock).expect("first lock");
        assert!(matches!(
            SessionLock::acquire(&paths.lock),
            Err(ClaudeDesktopError::ConcurrentSession)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn configuration_symlinks_are_rejected_without_touching_the_target() {
        use std::os::unix::fs::symlink;

        let (_root, paths) = paths();
        let target = paths
            .normal_config
            .parent()
            .expect("normal parent")
            .join("user-owned.json");
        fs::create_dir_all(target.parent().expect("target parent")).expect("target directory");
        fs::write(&target, b"{\"private\":true}").expect("target contents");
        symlink(&target, &paths.normal_config).expect("configuration symlink");

        let error = Receipt::capture(&paths).expect_err("symlink must be rejected");

        assert!(matches!(error, ClaudeDesktopError::UnsafeSymlink));
        assert_eq!(
            fs::read(&target).expect("target should remain readable"),
            b"{\"private\":true}"
        );
        assert!(
            fs::symlink_metadata(&paths.normal_config)
                .expect("symlink should remain")
                .file_type()
                .is_symlink()
        );
        assert!(!paths.backup_directory.exists());
    }

    struct FakeProcess {
        profile: PathBuf,
        available: AtomicBool,
        running: AtomicBool,
        terminated: AtomicBool,
        force_terminated: AtomicBool,
        terminated_while_gateway_active: AtomicBool,
        fail_checks: AtomicBool,
        transient_check_failures: AtomicUsize,
        fail_terminate: AtomicBool,
        fail_force_terminate: AtomicBool,
    }

    impl FakeProcess {
        fn running(profile: PathBuf) -> Self {
            Self {
                profile,
                available: AtomicBool::new(true),
                running: AtomicBool::new(true),
                terminated: AtomicBool::new(false),
                force_terminated: AtomicBool::new(false),
                terminated_while_gateway_active: AtomicBool::new(false),
                fail_checks: AtomicBool::new(false),
                transient_check_failures: AtomicUsize::new(0),
                fail_terminate: AtomicBool::new(false),
                fail_force_terminate: AtomicBool::new(false),
            }
        }
    }

    impl DesktopProcess for FakeProcess {
        fn ensure_available(&self) -> Result<(), ClaudeDesktopError> {
            if self.available.load(Ordering::SeqCst) {
                Ok(())
            } else {
                Err(ClaudeDesktopError::AppNotFound { platform: "test" })
            }
        }

        fn is_running(&self) -> Result<bool, ClaudeDesktopError> {
            let transient_failure = self
                .transient_check_failures
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                    remaining.checked_sub(1)
                })
                .is_ok();
            if self.fail_checks.load(Ordering::SeqCst) || transient_failure {
                return Err(ClaudeDesktopError::ProcessCheck(std::io::Error::other(
                    "synthetic process check failure",
                )));
            }
            Ok(self.running.load(Ordering::SeqCst))
        }

        fn launch(&self) -> Result<(), ClaudeDesktopError> {
            self.running.store(true, Ordering::SeqCst);
            Ok(())
        }

        fn terminate(&self) -> Result<(), ClaudeDesktopError> {
            if self.fail_terminate.load(Ordering::SeqCst) {
                return Err(ClaudeDesktopError::Terminate(std::io::Error::other(
                    "synthetic termination failure",
                )));
            }
            let gateway_active = read_json_object(&self.profile).is_ok_and(|profile| {
                profile.get("inferenceProvider").and_then(Value::as_str) == Some("gateway")
            });
            self.terminated_while_gateway_active
                .store(gateway_active, Ordering::SeqCst);
            self.terminated.store(true, Ordering::SeqCst);
            self.running.store(false, Ordering::SeqCst);
            Ok(())
        }

        fn force_terminate(&self) -> Result<(), ClaudeDesktopError> {
            self.force_terminated.store(true, Ordering::SeqCst);
            if self.fail_force_terminate.load(Ordering::SeqCst) {
                return Err(ClaudeDesktopError::Terminate(std::io::Error::other(
                    "synthetic forced termination failure",
                )));
            }
            self.terminated.store(true, Ordering::SeqCst);
            self.running.store(false, Ordering::SeqCst);
            Ok(())
        }
    }

    #[test]
    fn missing_desktop_is_rejected_before_session_state_setup() {
        let (_root, paths) = paths();
        let process = FakeProcess::running(paths.profile.clone());
        process.available.store(false, Ordering::SeqCst);

        assert!(matches!(
            prepare_session_lock(&paths, &process),
            Err(ClaudeDesktopError::AppNotFound { .. })
        ));
        assert!(!paths.lock.exists());
        assert!(!paths.receipt.exists());
        assert!(!paths.backup_directory.exists());
    }

    #[tokio::test]
    async fn signal_terminates_desktop_before_exact_restore() {
        let (_root, paths) = paths();
        fs::create_dir_all(paths.profile.parent().expect("parent")).expect("profile directory");
        let original = b"{\"userField\":\"original\"}\n";
        fs::write(&paths.profile, original).expect("original profile");
        Receipt::capture(&paths)
            .expect("capture")
            .write(&paths.receipt)
            .expect("receipt");
        apply_gateway(&paths, "http://127.0.0.1:1234", "session-token").expect("apply");
        let process = FakeProcess::running(paths.profile.clone());

        let exit_code = complete_and_restore(&paths, &process, Ok(WaitOutcome::Signaled(130)))
            .await
            .expect("signal cleanup");

        assert_eq!(exit_code, 130);
        assert!(process.terminated.load(Ordering::SeqCst));
        assert!(
            process
                .terminated_while_gateway_active
                .load(Ordering::SeqCst),
            "profile was restored before Claude Desktop was terminated"
        );
        assert_eq!(
            fs::read(&paths.profile).expect("restored profile"),
            original
        );
        assert!(!paths.receipt.exists());
        assert!(!paths.backup_directory.exists());
    }

    #[tokio::test]
    async fn process_wait_error_still_restores_exact_configuration() {
        let (_root, paths) = paths();
        fs::create_dir_all(paths.normal_config.parent().expect("parent"))
            .expect("config directory");
        let original = b"{\"deploymentMode\":\"1p\",\"kept\":7}\n";
        fs::write(&paths.normal_config, original).expect("original config");
        Receipt::capture(&paths)
            .expect("capture")
            .write(&paths.receipt)
            .expect("receipt");
        apply_gateway(&paths, "http://127.0.0.1:1234", "session-token").expect("apply");
        let process = FakeProcess::running(paths.profile.clone());
        process.transient_check_failures.store(1, Ordering::SeqCst);
        let wait_error = wait_for_exit_or_signal(&process).await;

        let error = complete_and_restore(&paths, &process, wait_error)
            .await
            .expect_err("process error should propagate");

        assert!(matches!(error, ClaudeDesktopError::ProcessCheck(_)));
        assert!(process.terminated.load(Ordering::SeqCst));
        assert_eq!(
            fs::read(&paths.normal_config).expect("restored config"),
            original
        );
        assert!(!paths.receipt.exists());
        assert!(!paths.backup_directory.exists());
    }

    #[test]
    fn apply_error_restores_before_launch() {
        let (_root, paths) = paths();
        fs::create_dir_all(paths.normal_config.parent().expect("parent"))
            .expect("config directory");
        let original = b"{\"deploymentMode\":\"1p\",\"kept\":8}\n";
        fs::write(&paths.normal_config, original).expect("original config");
        Receipt::capture(&paths)
            .expect("capture")
            .write(&paths.receipt)
            .expect("receipt");
        apply_gateway(&paths, "http://127.0.0.1:1234", "session-token").expect("apply");

        let error = restore_after(&paths, Err(ClaudeDesktopError::ConfigRoot))
            .expect_err("apply error should propagate");

        assert!(matches!(error, ClaudeDesktopError::ConfigRoot));
        assert_eq!(
            fs::read(&paths.normal_config).expect("restored config"),
            original
        );
        assert!(!paths.receipt.exists());
        assert!(!paths.backup_directory.exists());
    }

    #[tokio::test]
    async fn launch_error_terminates_partial_launch_before_restore() {
        let (_root, paths) = paths();
        fs::create_dir_all(paths.profile.parent().expect("parent")).expect("profile directory");
        let original = b"{\"userField\":\"before-launch\"}\n";
        fs::write(&paths.profile, original).expect("original profile");
        Receipt::capture(&paths)
            .expect("capture")
            .write(&paths.receipt)
            .expect("receipt");
        apply_gateway(&paths, "http://127.0.0.1:1234", "session-token").expect("apply");
        let process = FakeProcess::running(paths.profile.clone());

        let error = complete_and_restore(
            &paths,
            &process,
            Err(ClaudeDesktopError::LaunchFailed(Some(1))),
        )
        .await
        .expect_err("launch error should propagate");

        assert!(matches!(error, ClaudeDesktopError::LaunchFailed(Some(1))));
        assert!(process.terminated.load(Ordering::SeqCst));
        assert!(
            process
                .terminated_while_gateway_active
                .load(Ordering::SeqCst)
        );
        assert_eq!(
            fs::read(&paths.profile).expect("restored profile"),
            original
        );
        assert!(!paths.receipt.exists());
        assert!(!paths.backup_directory.exists());
    }

    #[tokio::test]
    async fn termination_failure_leaves_active_config_and_recovery_state() {
        let (_root, paths) = paths();
        fs::create_dir_all(paths.profile.parent().expect("parent")).expect("profile directory");
        let original = b"{\"userField\":\"original\"}\n";
        fs::write(&paths.profile, original).expect("original profile");
        Receipt::capture(&paths)
            .expect("capture")
            .write(&paths.receipt)
            .expect("receipt");
        apply_gateway(&paths, "http://127.0.0.1:1234", "session-token").expect("apply");
        let active = fs::read(&paths.profile).expect("active profile");
        let process = FakeProcess::running(paths.profile.clone());
        process.fail_terminate.store(true, Ordering::SeqCst);
        process.fail_force_terminate.store(true, Ordering::SeqCst);

        let error = complete_and_restore(&paths, &process, Ok(WaitOutcome::Signaled(143)))
            .await
            .expect_err("unsafe cleanup should fail");

        assert!(matches!(error, ClaudeDesktopError::Terminate(_)));
        assert!(process.force_terminated.load(Ordering::SeqCst));
        assert_eq!(
            fs::read(&paths.profile).expect("profile should remain active"),
            active
        );
        assert!(paths.receipt.exists(), "receipt should remain recoverable");
        assert!(
            paths.backup_directory.exists(),
            "backup should remain recoverable"
        );
    }

    #[tokio::test]
    async fn persistent_process_check_error_does_not_restore_without_confirmation() {
        let (_root, paths) = paths();
        fs::create_dir_all(paths.profile.parent().expect("parent")).expect("profile directory");
        fs::write(&paths.profile, b"{\"userField\":\"original\"}\n").expect("original profile");
        Receipt::capture(&paths)
            .expect("capture")
            .write(&paths.receipt)
            .expect("receipt");
        apply_gateway(&paths, "http://127.0.0.1:1234", "session-token").expect("apply");
        let active = fs::read(&paths.profile).expect("active profile");
        let process = FakeProcess::running(paths.profile.clone());
        process.fail_checks.store(true, Ordering::SeqCst);

        let error = complete_and_restore(
            &paths,
            &process,
            Err(ClaudeDesktopError::ProcessCheck(std::io::Error::other(
                "synthetic wait failure",
            ))),
        )
        .await
        .expect_err("unconfirmed termination should fail");

        assert!(matches!(error, ClaudeDesktopError::ProcessCheck(_)));
        assert!(process.terminated.load(Ordering::SeqCst));
        assert!(process.force_terminated.load(Ordering::SeqCst));
        assert_eq!(
            fs::read(&paths.profile).expect("profile should remain active"),
            active
        );
        assert!(paths.receipt.exists(), "receipt should remain recoverable");
        assert!(
            paths.backup_directory.exists(),
            "backup should remain recoverable"
        );
    }
}
