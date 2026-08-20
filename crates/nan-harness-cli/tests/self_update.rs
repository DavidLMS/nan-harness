use nan_harness_runtime::update::{ReleaseArtifact, ReleaseManifest, UpdateManager};
use semver::Version;
use sha2::{Digest as _, Sha256};
use std::fmt::Write as _;
use std::fs;
use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

const CHILD_ENVIRONMENT_VARIABLE: &str = "NAN_SELF_UPDATE_TEST_CHILD";
const ARTIFACT_URL_ENVIRONMENT_VARIABLE: &str = "NAN_SELF_UPDATE_TEST_ARTIFACT_URL";
const ARTIFACT_CHECKSUM_ENVIRONMENT_VARIABLE: &str = "NAN_SELF_UPDATE_TEST_ARTIFACT_SHA256";
const TEST_NAME: &str = "self_update_replaces_a_running_copy";
type ServerError = Box<dyn std::error::Error + Send + Sync>;
type ServerHandle = thread::JoinHandle<Result<(), ServerError>>;

#[test]
fn self_update_replaces_a_running_copy() {
    if std::env::var_os(CHILD_ENVIRONMENT_VARIABLE).is_some() {
        install_candidate_in_child();
        return;
    }

    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let target = copied_test_process(directory.path());
    let candidate_bytes = candidate_bytes();
    let checksum = hex_digest(Sha256::digest(&candidate_bytes));
    let (artifact_url, server) = serve_once(candidate_bytes);

    let output = Command::new(&target)
        .args([TEST_NAME, "--exact"])
        .env(CHILD_ENVIRONMENT_VARIABLE, "1")
        .env(ARTIFACT_URL_ENVIRONMENT_VARIABLE, &artifact_url)
        .env(ARTIFACT_CHECKSUM_ENVIRONMENT_VARIABLE, checksum)
        .env("NAN_UPDATE_MANIFEST_URL", &artifact_url)
        .env("NAN_HARNESS_CONFIG_DIR", directory.path().join("config"))
        .env("NO_PROXY", "127.0.0.1,localhost")
        .output()
        .expect("copied test process should start");
    let server_result = server.join().expect("artifact server should finish");
    server_result.expect("artifact server should deliver the candidate");
    assert!(
        output.status.success(),
        "self-update child failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let version = Command::new(&target)
        .arg("--version")
        .output()
        .expect("updated process should start");
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        format!("nan {}", env!("CARGO_PKG_VERSION"))
    );
}

fn install_candidate_in_child() {
    let artifact_url = std::env::var(ARTIFACT_URL_ENVIRONMENT_VARIABLE)
        .expect("artifact URL should be configured");
    let sha256 = std::env::var(ARTIFACT_CHECKSUM_ENVIRONMENT_VARIABLE)
        .expect("artifact checksum should be configured");
    let release = ReleaseManifest {
        schema_version: 1,
        version: Version::parse(env!("CARGO_PKG_VERSION")).expect("version should be valid"),
        notes_url: "https://example.com/releases/test".to_owned(),
        artifacts: vec![ReleaseArtifact {
            target: current_target().to_owned(),
            url: artifact_url,
            sha256,
        }],
    };
    let manager = UpdateManager::from_environment().expect("update manager should build");
    tokio::runtime::Runtime::new()
        .expect("runtime should build")
        .block_on(manager.install(&release))
        .expect("candidate should replace the copied process");
}

fn copied_test_process(directory: &Path) -> PathBuf {
    let current = std::env::current_exe().expect("test executable path should be available");
    let extension = current.extension().and_then(|value| value.to_str());
    let file_name = extension.map_or_else(
        || "nan-self-update-test".to_owned(),
        |extension| format!("nan-self-update-test.{extension}"),
    );
    let target = directory.join(file_name);
    fs::copy(current, &target).expect("test executable should be copied");
    target
}

#[cfg(unix)]
fn candidate_bytes() -> Vec<u8> {
    format!(
        "#!/bin/sh\nprintf '%s\\n' 'nan {}'\n",
        env!("CARGO_PKG_VERSION")
    )
    .into_bytes()
}

#[cfg(windows)]
fn candidate_bytes() -> Vec<u8> {
    fs::read(env!("CARGO_BIN_EXE_nan")).expect("candidate binary should be readable")
}

fn serve_once(body: Vec<u8>) -> (String, ServerHandle) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("artifact server should bind");
    listener
        .set_nonblocking(true)
        .expect("artifact server should become nonblocking");
    let address = listener
        .local_addr()
        .expect("artifact server address should exist");
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false)?;
                    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
                    let mut request = [0_u8; 2048];
                    let _ = stream.read(&mut request)?;
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )?;
                    stream.write_all(&body)?;
                    stream.flush()?;
                    return Ok(());
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err("artifact server timed out".into());
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => return Err(error.into()),
            }
        }
    });
    (format!("http://{address}/nan"), server)
}

fn hex_digest(bytes: impl AsRef<[u8]>) -> String {
    let bytes = bytes.as_ref();
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}

#[cfg(all(target_arch = "aarch64", target_os = "macos"))]
const fn current_target() -> &'static str {
    "aarch64-apple-darwin"
}

#[cfg(all(target_arch = "x86_64", target_os = "macos"))]
const fn current_target() -> &'static str {
    "x86_64-apple-darwin"
}

#[cfg(all(target_arch = "aarch64", target_env = "musl", target_os = "linux"))]
const fn current_target() -> &'static str {
    "aarch64-unknown-linux-musl"
}

#[cfg(all(target_arch = "x86_64", target_env = "musl", target_os = "linux"))]
const fn current_target() -> &'static str {
    "x86_64-unknown-linux-musl"
}

#[cfg(all(target_arch = "x86_64", target_env = "msvc", target_os = "windows"))]
const fn current_target() -> &'static str {
    "x86_64-pc-windows-msvc"
}

#[cfg(all(target_arch = "x86_64", target_env = "gnu", target_os = "linux"))]
const fn current_target() -> &'static str {
    "x86_64-unknown-linux-gnu"
}
