use super::lifecycle::{run_bridged_child, run_plain_child};
use super::report::Completion;
use crate::prepared::PreparedLaunch;
use crate::signals::{CancellationToken, SignalKind};
use nan_harness_bridge::{ChatCompletionsBridgeConfig, RunningBridge, spawn_chat_completions};
use nan_harness_core::{LaunchPlan, SecretStore, SecretValue};
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

struct Fixture {
    root: tempfile::TempDir,
    plan: LaunchPlan,
    prepared: PreparedLaunch,
}

impl Fixture {
    async fn bridge(&mut self, check_drain: bool) -> RunningBridge {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        if check_drain {
            let address = listener.local_addr().unwrap();
            self.plan
                .environment
                .public
                .insert("CHECK_BRIDGE".into(), format!("http://{address}"));
        }
        self.prepared = PreparedLaunch::prepare(
            &self.plan,
            "http://127.0.0.1:9/v1",
            None,
            None,
            &SecretStore::new(),
        )
        .unwrap()
        .with_hermes_fixture(self.root.path().to_owned());
        spawn_chat_completions(
            listener,
            ChatCompletionsBridgeConfig {
                launch_id: self.plan.launch_id.to_string(),
                provider_base_url: "http://127.0.0.1:9/v1".into(),
                model_id: self.plan.model.resolved_id.clone(),
                provider_api_key: Arc::new(SecretValue::new("synthetic-provider").unwrap()),
                session_token: Arc::new(SecretValue::new("synthetic-session").unwrap()),
                web_search_enabled: false,
                search_config: None,
                session_max_tokens: None,
            },
        )
        .unwrap()
    }
    fn new(mode: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let interpreter = std::process::Command::new("python3")
            .args(["-c", "import sys; print(sys.executable)"])
            .output()
            .unwrap();
        assert!(interpreter.status.success());
        let python = String::from_utf8(interpreter.stdout).unwrap();
        let executable = root.path().join("hermes");
        fs::write(
            &executable,
            format!(
                "#!{}\n{}",
                python.trim(),
                include_str!("fixtures/hermes.py")
            ),
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(root.path().join("mode"), mode).unwrap();
        let mut plan: LaunchPlan = serde_json::from_str(include_str!(
            "../../../nan-harness-core/tests/fixtures/launch-plan.direct.json"
        ))
        .unwrap();
        plan.harness.executable = executable.to_string_lossy().into();
        plan.environment.public.clear();
        plan.environment.secrets.clear();
        plan.process.arguments.clear();
        plan.process.working_directory = root.path().to_string_lossy().into();
        plan.cleanup.grace_period_ms = 1000;
        let prepared = PreparedLaunch::prepare(
            &plan,
            "http://127.0.0.1:9/v1",
            None,
            None,
            &SecretStore::new(),
        )
        .unwrap();
        let prepared = prepared.with_hermes_fixture(root.path().to_owned());
        Self {
            root,
            plan,
            prepared,
        }
    }

    fn events(&self) -> Vec<String> {
        fs::read_to_string(self.root.path().join("events"))
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn assert_stopped(&self) {
        let events = self.events();
        let exited = events
            .iter()
            .position(|event| event == "owner-exited")
            .unwrap();
        let cleanup = events.iter().position(|event| event == "cleanup").unwrap();
        assert!(exited < cleanup);
        assert!(!self.root.path().join("ephemeral").exists());
    }
}

#[tokio::test]
async fn native_gateway_is_reaped_before_bridge_shutdown_for_success_and_failure() {
    for mode in ["success", "nonzero", "readiness-failure"] {
        let mut fixture = Fixture::new(mode);
        let mut bridge = fixture.bridge(true).await;
        let result = run_bridged_child(
            &fixture.plan,
            &fixture.prepared,
            &SecretStore::new(),
            &CancellationToken::new(),
            &mut bridge,
            None,
        )
        .await;
        match mode {
            "success" => assert!(
                matches!(result.unwrap().completion, Completion::Exited(status) if status.success())
            ),
            "nonzero" => assert!(
                matches!(result.unwrap().completion, Completion::Exited(status) if status.code() == Some(7))
            ),
            _ => assert!(result.is_err(), "{mode}"),
        }
        fixture.assert_stopped();
        assert!(
            fixture
                .events()
                .contains(&"bridge-alive-during-drain".to_owned())
        );
        assert!(bridge.is_finished());
    }
}

#[tokio::test]
async fn bridge_failure_drains_the_gateway_and_does_not_join_the_bridge_twice() {
    let mut fixture = Fixture::new("wait");
    let mut bridge = fixture.bridge(false).await;
    bridge.shutdown();
    let result = run_bridged_child(
        &fixture.plan,
        &fixture.prepared,
        &SecretStore::new(),
        &CancellationToken::new(),
        &mut bridge,
        None,
    )
    .await;
    assert!(matches!(result, Err(super::RuntimeError::BridgeExited)));
    fixture.assert_stopped();
}

#[tokio::test]
async fn native_gateway_is_reaped_on_force_cancel_and_without_bridge() {
    let fixture = Fixture::new("wait");
    let cancellation = CancellationToken::new();
    let trigger = cancellation.clone();
    let ready = fixture.root.path().join("frontend-ready");
    let signal = tokio::spawn(async move {
        tokio::time::timeout(Duration::from_secs(5), async {
            while !ready.exists() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        trigger.cancel(SignalKind::Interrupt);
        tokio::time::sleep(Duration::from_millis(30)).await;
        trigger.cancel(SignalKind::Interrupt);
    });
    let result = run_plain_child(
        &fixture.plan,
        &fixture.prepared,
        &SecretStore::new(),
        &cancellation,
    )
    .await
    .unwrap();
    signal.await.unwrap();
    assert!(matches!(
        result,
        Completion::Cancelled(SignalKind::Interrupt)
    ));
    fixture.assert_stopped();
}

#[tokio::test]
async fn uncertain_cleanup_retains_the_prepared_workspace() {
    for mode in [
        "cleanup-failure",
        "owner-exit",
        "spawn-failure",
        "owner-spawn-failure",
    ] {
        let fixture = Fixture::new(mode);
        let path: PathBuf = fixture.prepared.temporary_root(true).unwrap();
        assert!(
            run_plain_child(
                &fixture.plan,
                &fixture.prepared,
                &SecretStore::new(),
                &CancellationToken::new()
            )
            .await
            .is_err()
        );
        if mode == "owner-spawn-failure" {
            assert!(!fixture.root.path().join("owner.pid").exists());
        } else {
            assert!(fixture.events().contains(&"owner-exited".to_owned()));
        }
        drop(fixture);
        assert!(path.is_dir(), "{mode}");
        fs::remove_dir_all(path).unwrap();
    }
}

#[tokio::test]
async fn excessive_launcher_metadata_is_stopped_without_waiting_for_exit() {
    let fixture = Fixture::new("metadata-flood");
    let path = fixture.prepared.temporary_root(true).unwrap();
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        run_plain_child(
            &fixture.plan,
            &fixture.prepared,
            &SecretStore::new(),
            &CancellationToken::new(),
        ),
    )
    .await
    .unwrap();
    assert!(result.is_err());
    assert!(!fixture.root.path().join("owner.pid").exists());
    drop(fixture);
    assert!(path.is_dir());
    fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn startup_cancellation_preserves_the_signal_after_gateway_cleanup() {
    let fixture = Fixture::new("readiness-wait");
    let cancellation = CancellationToken::new();
    let trigger = cancellation.clone();
    let ready = fixture.root.path().join("readiness-waiting");
    let signal = tokio::spawn(async move {
        tokio::time::timeout(Duration::from_secs(5), async {
            while !ready.exists() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        trigger.cancel(SignalKind::Terminate);
    });
    let result = run_plain_child(
        &fixture.plan,
        &fixture.prepared,
        &SecretStore::new(),
        &cancellation,
    )
    .await
    .unwrap();
    signal.await.unwrap();
    assert!(matches!(
        result,
        Completion::Cancelled(SignalKind::Terminate)
    ));
    fixture.assert_stopped();
}

#[tokio::test]
async fn dropping_execution_retains_files_instead_of_certifying_a_forced_drain() {
    let fixture = Fixture::new("wait");
    let path = fixture.prepared.temporary_root(true).unwrap();
    let secrets = SecretStore::new();
    let cancellation = CancellationToken::new();
    let mut execution = Box::pin(run_plain_child(
        &fixture.plan,
        &fixture.prepared,
        &secrets,
        &cancellation,
    ));
    tokio::select! {
        _result = &mut execution => panic!("fixture must remain running"),
        () = async {
            tokio::time::timeout(Duration::from_secs(5), async {
            while !fixture.root.path().join("frontend-ready").exists() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            }).await.unwrap();
        } => (),
    }
    let pids = ["owner.pid", "frontend.pid"].map(|name| {
        fs::read_to_string(fixture.root.path().join(name))
            .unwrap()
            .parse::<i32>()
            .unwrap()
    });
    drop(execution);
    tokio::time::timeout(Duration::from_secs(2), async {
        while pids
            .iter()
            .any(|pid| nix::sys::signal::kill(nix::unistd::Pid::from_raw(*pid), None).is_ok())
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    drop(fixture);
    assert!(path.is_dir());
    fs::remove_dir_all(path).unwrap();
}
