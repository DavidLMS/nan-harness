use nan_harness_runtime::search_supervisor::LocalSearxngSpec;
use nan_harness_runtime::searxng::SearxngCommand;
use nan_harness_runtime::{SearchSupervisor, SearchSupervisorTimings, SearxngConfig};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::process::{Child, Command};

const CHILD_ENVIRONMENT: &str = "NAN_HARNESS_SEARCH_TEST_CHILD";

#[tokio::test]
async fn owner_exit_preserves_backend_for_borrower_and_last_session_cleans_up() {
    let directory = tempfile::tempdir().expect("coordination directory");
    let port = free_port();
    let owner = spawn_child(directory.path(), port);
    let mut owner = owner;
    wait_ready(&mut owner).await;
    let mut borrower = spawn_child(directory.path(), port);
    wait_ready(&mut borrower).await;

    assert!(
        connect(port).await,
        "synthetic backend should serve both sessions"
    );
    owner
        .stdin
        .as_mut()
        .expect("owner stdin")
        .write_all(b"\n")
        .await
        .expect("release owner session");
    wait_for_exit(&mut owner).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(connect(port).await, "backend survives beyond idle grace");

    borrower
        .stdin
        .as_mut()
        .expect("borrower stdin")
        .write_all(b"\n")
        .await
        .expect("release borrower session");
    wait_for_exit(&mut borrower).await;
    assert_backend_state(port, false).await;
}

#[tokio::test]
async fn child_session() {
    if std::env::var_os(CHILD_ENVIRONMENT).is_none() {
        return;
    }
    let directory = PathBuf::from(std::env::var_os("NAN_HARNESS_SEARCH_TEST_DIRECTORY").unwrap());
    let port: u16 = std::env::var("NAN_HARNESS_SEARCH_TEST_PORT")
        .expect("port")
        .parse()
        .expect("valid port");
    let endpoint_url = format!("http://127.0.0.1:{port}");
    let endpoint = SearxngConfig::local(&endpoint_url).expect("endpoint");
    let command = SearxngCommand {
        program: PathBuf::from(if cfg!(windows) {
            "python.exe"
        } else {
            "python3"
        }),
        arguments: vec![
            "-m".to_owned(),
            "http.server".to_owned(),
            port.to_string(),
            "--bind".to_owned(),
            "127.0.0.1".to_owned(),
        ],
        current_directory: directory.clone(),
        settings_path: None,
    };
    let supervisor = SearchSupervisor::with_timings(
        Some(LocalSearxngSpec::new(directory, endpoint, command).expect("spec")),
        SearchSupervisorTimings {
            readiness_timeout: Duration::from_secs(5),
            readiness_retry: Duration::from_millis(10),
            recovery_backoff: Duration::from_millis(25),
            shutdown_grace: Duration::from_millis(100),
            grace_recheck: Duration::from_millis(10),
            coordination_timeout: Duration::from_secs(5),
        },
    )
    .expect("supervisor")
    .with_host_executable(env!("CARGO_BIN_EXE_nan-harness"));
    let lease = supervisor
        .acquire()
        .await
        .expect("acquire")
        .expect("local backend");
    println!("READY");
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .expect("session release input");
    drop(lease);
}

fn spawn_child(directory: &std::path::Path, port: u16) -> Child {
    let mut command = Command::new(std::env::current_exe().expect("test executable"));
    command
        .args(["--exact", "child_session", "--nocapture"])
        .env(CHILD_ENVIRONMENT, "1")
        .env("TOKIO_WORKER_THREADS", "2")
        .env("NAN_HARNESS_SEARCH_TEST_DIRECTORY", directory)
        .env("NAN_HARNESS_SEARCH_TEST_PORT", port.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    command
        .kill_on_drop(true)
        .spawn()
        .expect("child session should start")
}

async fn wait_ready(child: &mut Child) {
    let stdout = child.stdout.take().expect("child stdout");
    let mut lines = BufReader::new(stdout).lines();
    tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(line) = lines.next_line().await.expect("child output") {
            if line == "READY" {
                tokio::spawn(
                    async move { while lines.next_line().await.ok().flatten().is_some() {} },
                );
                return;
            }
        }
        panic!("child exited before readiness");
    })
    .await
    .expect("child readiness bound");
}

async fn wait_for_exit(child: &mut Child) {
    let status = tokio::time::timeout(Duration::from_secs(10), child.wait())
        .await
        .expect("child exit bound")
        .expect("child wait");
    assert!(status.success(), "client exited unsuccessfully: {status}");
}

async fn assert_backend_state(port: u16, expected_alive: bool) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if connect(port).await == expected_alive {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("backend lifecycle bound");
}

async fn connect(port: u16) -> bool {
    tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .is_ok()
}

fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("free loopback port");
    listener.local_addr().expect("listener address").port()
}

#[tokio::test]
async fn killed_sessions_do_not_leave_a_backend_without_interests() {
    let directory = tempfile::tempdir().expect("coordination directory");
    let port = free_port();
    let mut owner = spawn_child(directory.path(), port);
    let mut borrower = spawn_child(directory.path(), port);
    wait_ready(&mut owner).await;
    wait_ready(&mut borrower).await;
    owner.kill().await.expect("kill first client");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(connect(port).await, "borrower retains the hosted backend");
    borrower.kill().await.expect("kill last client");
    assert_backend_state(port, false).await;
}

#[tokio::test]
async fn abrupt_host_exit_closes_the_backend_lifetime_pipe() {
    let directory = tempfile::tempdir().expect("host directory");
    let port = free_port();
    let _interest =
        nan_harness_runtime::SearchInterest::acquire(directory.path()).expect("interest");
    let request_path = directory.path().join("host-request.json");
    let request = serde_json::json!({
        "directory": directory.path(),
        "endpoint": format!("http://127.0.0.1:{port}/"),
        "command": {
            "program": if cfg!(windows) { "python.exe" } else { "python3" },
            "arguments": ["-m", "http.server", &port.to_string(), "--bind", "127.0.0.1"],
            "currentDirectory": directory.path(),
            "settingsPath": null
        },
        "shutdownGrace": {"secs": 0, "nanos": 100_000_000},
        "graceRecheck": {"secs": 0, "nanos": 10_000_000},
        "recoveryBackoff": {"secs": 0, "nanos": 25_000_000}
    });
    std::fs::write(
        &request_path,
        serde_json::to_vec(&request).expect("request JSON"),
    )
    .expect("request");
    let mut host = Command::new(env!("CARGO_BIN_EXE_nan-harness"))
        .arg("__searxng-host")
        .arg(&request_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .expect("host");
    assert_backend_state(port, true).await;
    host.kill().await.expect("kill owned host handle");
    assert_backend_state(port, false).await;
}

#[tokio::test]
async fn stale_record_never_authorizes_terminating_a_live_unrelated_process() {
    let directory = tempfile::tempdir().expect("coordination directory");
    let port = free_port();
    let record = serde_json::json!({
        "schemaVersion": 1,
        "owner": "nan-harness-searxng-supervisor-v1",
        "endpoint": format!("http://127.0.0.1:{port}/"),
        "pid": std::process::id()
    });
    std::fs::write(
        directory.path().join(".nan-harness-searxng.json"),
        serde_json::to_vec(&record).expect("stale record"),
    )
    .expect("write stale record");
    let mut session = spawn_child(directory.path(), port);
    wait_ready(&mut session).await;
    session
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"\n")
        .await
        .expect("release");
    wait_for_exit(&mut session).await;
    assert_backend_state(port, false).await;
}
