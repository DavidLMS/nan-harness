use crate::common::{send_budget_request, start_servers_with_budget};
use std::{sync::atomic::Ordering, time::Duration};

const SCENARIO: &str = "NAN_TEST_RESPONSES_ACCOUNTING";

#[tokio::test]
async fn recovery_preserves_session_accounting() {
    if std::env::var_os(SCENARIO).is_some() {
        exercise().await;
        return;
    }
    let directory = tempfile::tempdir().expect("isolated configuration");
    let mut command = tokio::process::Command::new(std::env::current_exe().expect("test binary"));
    command
        .args([
            "--exact",
            "accounting::recovery_preserves_session_accounting",
            "--nocapture",
        ])
        .env_clear()
        .env(SCENARIO, "1")
        .env("NAN_HARNESS_CONFIG_DIR", directory.path())
        .env("NAN_HARNESS_INTERNAL_MANAGED_PROCESS", "1")
        .kill_on_drop(true);
    if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
        command.env("LLVM_PROFILE_FILE", profile);
    }
    let result = tokio::time::timeout(Duration::from_secs(30), command.status())
        .await
        .expect("accounting test deadline")
        .expect("child status");
    assert!(result.success());
}

async fn exercise() {
    let daemon = tokio::spawn(nan_harness_coordinator::run_daemon());
    let receipt = nan_harness_coordinator::config_directory()
        .expect("configuration")
        .join("coordinator/v1/receipt.json");
    tokio::time::timeout(Duration::from_secs(5), async {
        while !receipt.exists() {
            assert!(!daemon.is_finished(), "coordinator stopped before startup");
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("coordinator startup");
    for empty in [false, true] {
        let servers = start_servers_with_budget(false, Some(45)).await;
        if empty {
            servers.state.empty_completions.store(1, Ordering::Relaxed);
        } else {
            servers
                .state
                .malformed_patch_completions
                .store(1, Ordering::Relaxed);
        }
        let body = send_budget_request(&servers).await;
        assert!(body.contains("response.completed"), "{body}");
        assert!(!body.contains("response.failed"), "{body}");
        assert_eq!(servers.state.chat_attempts.load(Ordering::Relaxed), 2);

        let body = send_budget_request(&servers).await;
        assert!(body.contains("response.completed"), "{body}");
        let body = send_budget_request(&servers).await;
        assert!(body.contains("NH-BRIDGE-109"), "{body}");
        assert!(!body.contains("NH-BRIDGE-110"), "{body}");
        assert_eq!(servers.state.chat_attempts.load(Ordering::Relaxed), 3);
        servers.shutdown().await;
    }
    let servers = start_servers_with_budget(false, Some(45)).await;
    servers
        .state
        .truncated_completions
        .store(1, Ordering::Relaxed);
    let body = send_budget_request(&servers).await;
    assert!(body.contains("NH-BRIDGE-110"), "{body}");
    let body = send_budget_request(&servers).await;
    assert!(body.contains("NH-BRIDGE-110"), "{body}");
    assert_eq!(servers.state.chat_attempts.load(Ordering::Relaxed), 1);
    servers.shutdown().await;
    daemon.abort();
}
