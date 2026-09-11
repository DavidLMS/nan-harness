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
    let borrower = spawn_child(directory.path(), port);
    let mut owner = owner;
    let mut borrower = borrower;
    wait_ready(&mut owner).await;
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
    assert_backend_state(port, true).await;
    tokio::time::sleep(Duration::from_millis(250)).await;

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
        program: PathBuf::from("/usr/bin/python3"),
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
            readiness_timeout: Duration::from_secs(2),
            readiness_retry: Duration::from_millis(10),
            recovery_backoff: Duration::from_millis(25),
            shutdown_grace: Duration::from_millis(100),
            grace_recheck: Duration::from_millis(10),
            coordination_timeout: Duration::from_secs(2),
        },
    )
    .expect("supervisor");
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
    tokio::time::sleep(Duration::from_millis(250)).await;
}

fn spawn_child(directory: &std::path::Path, port: u16) -> Child {
    let mut command = Command::new(std::env::current_exe().expect("test executable"));
    command
        .args(["--exact", "child_session", "--nocapture"])
        .env(CHILD_ENVIRONMENT, "1")
        .env("NAN_HARNESS_SEARCH_TEST_DIRECTORY", directory)
        .env("NAN_HARNESS_SEARCH_TEST_PORT", port.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    command
        .kill_on_drop(true)
        .spawn()
        .expect("child session should start")
}

async fn wait_ready(child: &mut Child) {
    let stdout = child.stdout.take().expect("child stdout");
    let mut lines = BufReader::new(stdout).lines();
    tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(line) = lines.next_line().await.expect("child output") {
            if line == "READY" {
                return;
            }
        }
        panic!("child exited before readiness");
    })
    .await
    .expect("child readiness bound");
}

async fn wait_for_exit(child: &mut Child) {
    tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("child exit bound")
        .expect("child wait");
}

async fn assert_backend_state(port: u16, expected_alive: bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
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
