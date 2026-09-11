use crate::support::{fake_harness, write_private_credential_fixture};
use std::io::{Read as _, Write as _};
use std::process::{Command, Output};
use std::sync::{
    Arc,
    atomic::{AtomicU16, Ordering},
};

struct Fixture {
    directory: tempfile::TempDir,
    endpoint: String,
    status: Arc<AtomicU16>,
    server: Option<std::thread::JoinHandle<()>>,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join("state")).unwrap();
        write_private_credential_fixture(&directory.path().join("state"), "test-key");
        fake_harness(directory.path(), "0.84.2");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let status = Arc::new(AtomicU16::new(200));
        let current = Arc::clone(&status);
        let server = std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let status = current.load(Ordering::SeqCst);
                if status == 0 {
                    break;
                }
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 1024];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    let read = stream.read(&mut buffer).unwrap();
                    if read == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..read]);
                }
                let body = if status == 200 {
                    r#"{"data":[{"id":"qwen3.6"}]}"#
                } else {
                    "invalid catalog"
                };
                let code = if status == 201 { 200 } else { status };
                write!(stream, "HTTP/1.1 {code} Response\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        Self {
            directory,
            endpoint,
            status,
            server: Some(server),
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_nanh"))
            .args(args)
            .env("HOME", self.directory.path())
            .env("USERPROFILE", self.directory.path())
            .env(
                "NAN_HARNESS_CONFIG_DIR",
                self.directory.path().join("state"),
            )
            .env("NAN_HARNESS_CREDENTIAL_BACKEND", "file")
            .env("NAN_NO_COMPATIBILITY_CHECK", "1")
            .env("NAN_BASE_URL", format!("{}/v1", self.endpoint))
            .env_remove("NAN_API_KEY")
            .env_remove("NAN_UPDATE_MANIFEST_URL")
            .env_remove("NAN_HARNESS_GLITCHTIP_DSN")
            .output()
            .unwrap()
    }

    fn launch(&self) -> Output {
        let executable = self.directory.path().join("fake-harness");
        self.run(&[
            "pi",
            "--executable",
            executable.to_str().unwrap(),
            "--no-search",
        ])
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.status.store(0, Ordering::SeqCst);
        let _ = std::net::TcpStream::connect(self.endpoint.trim_start_matches("http://"));
        if let Some(server) = self.server.take() {
            server.join().unwrap();
        }
    }
}

fn assert_cached(output: &Output) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert_eq!(stderr.matches("Using cached models").count(), 1, "{stderr}");
    assert!(!stderr.contains("test-key"));
}

#[test]
fn model_cache_supports_launches_with_current_and_missing_verification_receipts() {
    let fixture = Fixture::new();
    let first = fixture.launch();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let receipt_path = fixture
        .directory
        .path()
        .join("state/credential-verification.json");
    let receipt = std::fs::read(&receipt_path).unwrap();
    fixture.status.store(201, Ordering::SeqCst);
    assert_cached(&fixture.launch());
    assert_eq!(std::fs::read(&receipt_path).unwrap(), receipt);
    std::fs::remove_file(&receipt_path).unwrap();
    assert_cached(&fixture.launch());
    assert!(
        !receipt_path.exists(),
        "fallback must not verify credentials"
    );
    fixture.status.store(401, Ordering::SeqCst);
    let rejected = fixture.launch();
    assert!(!rejected.status.success());
    assert!(!String::from_utf8_lossy(&rejected.stderr).contains("Using cached models"));
}

#[test]
fn model_cache_supports_native_setup_but_not_doctor_or_auth_status() {
    let fixture = Fixture::new();
    let initial = fixture.run(&["config", "pi", "--yes"]);
    assert!(
        initial.status.success(),
        "{}",
        String::from_utf8_lossy(&initial.stderr)
    );
    fixture.status.store(201, Ordering::SeqCst);
    assert_cached(&fixture.run(&["config", "pi", "--refresh"]));
    let doctor = fixture.run(&["doctor"]);
    assert!(!String::from_utf8_lossy(&doctor.stderr).contains("Using cached models"));
    assert!(String::from_utf8_lossy(&doctor.stdout).contains("invalid"));
    let auth = fixture.run(&["auth", "status"]);
    assert!(!String::from_utf8_lossy(&auth.stderr).contains("Using cached models"));
    assert!(String::from_utf8_lossy(&auth.stdout).contains("could not be verified"));
}
