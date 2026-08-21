use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
#[cfg(unix)]
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::io::{Read as _, Write as _};
use std::net::TcpListener;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::thread;
use std::time::{Duration, Instant};

type ServerError = Box<dyn std::error::Error + Send + Sync>;
type ServerHandle = thread::JoinHandle<Result<(), ServerError>>;

#[test]
fn release_installer_installs_the_binary_and_alias() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let install_directory = directory.path().join("bin");
    let state_directory = directory.path().join("state");
    fs::create_dir_all(&home).expect("isolated home should exist");

    let candidate = fs::read(env!("CARGO_BIN_EXE_nan")).expect("candidate should be readable");
    let checksum = hex_digest(Sha256::digest(&candidate));
    let artifact = artifact_file_name();
    let responses = BTreeMap::from([
        (format!("/{artifact}"), candidate),
        (
            format!("/{artifact}.sha256"),
            format!("{checksum}  {artifact}\n").into_bytes(),
        ),
        (
            "/release-version.txt".to_owned(),
            format!("{}\n", env!("CARGO_PKG_VERSION")).into_bytes(),
        ),
    ]);
    let (base_url, server) = serve_all(responses);
    let output = run_installer(
        directory.path(),
        &home,
        &install_directory,
        &state_directory,
        &base_url,
    );
    assert_success("installer", &output);
    let server_result = server.join().expect("release server should finish");
    server_result.expect("release server should deliver every file");

    let binary = install_directory.join(binary_file_name());
    assert_version(&binary);
    assert_alias(&install_directory);
    assert_installation_receipt(&state_directory, &binary, &install_directory);

    let output = Command::new(&binary)
        .args(["__record-installation", "--executable"])
        .arg(&binary)
        .arg("--alias")
        .arg(alias_path(&install_directory))
        .env("HOME", &home)
        .env("NAN_HARNESS_CONFIG_DIR", &state_directory)
        .output()
        .expect("installed binary should refresh its receipt");
    assert_success("receipt refresh", &output);

    let output = Command::new(&binary)
        .arg("uninstall")
        .env("HOME", &home)
        .env("NAN_HARNESS_CONFIG_DIR", &state_directory)
        .output()
        .expect("uninstall should enforce confirmation");
    let stderr = String::from_utf8(output.stderr).expect("uninstall error should be UTF-8");
    assert!(!output.status.success());
    assert!(stderr.contains("error [NH-UNINSTALL-001]"));
    assert!(binary.exists());
    assert!(state_directory.exists());

    fs::write(state_directory.join("test-state"), b"managed data")
        .expect("application state should be writable");
    let output = Command::new(&binary)
        .args(["uninstall", "--yes"])
        .env("HOME", &home)
        .env("NAN_HARNESS_CONFIG_DIR", &state_directory)
        .output()
        .expect("installed binary should uninstall itself");
    assert_success("uninstall", &output);
    wait_until_removed(&binary);
    assert!(!binary.exists());
    assert!(!alias_path(&install_directory).exists());
    assert!(!state_directory.exists());
}

#[cfg(unix)]
#[test]
fn release_installer_bounds_download_failures_and_reports_them() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let install_directory = directory.path().join("bin");
    let state_directory = directory.path().join("state");
    let tool_directory = directory.path().join("tools");
    let curl_arguments = directory.path().join("curl-arguments.txt");
    fs::create_dir_all(&home).expect("isolated home should exist");
    fs::create_dir_all(&tool_directory).expect("tool directory should exist");

    let fake_curl = tool_directory.join("curl");
    fs::write(
        &fake_curl,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$NAN_TEST_CURL_ARGUMENTS\"\nexit 28\n",
    )
    .expect("fake curl should be writable");
    let mut permissions = fs::metadata(&fake_curl)
        .expect("fake curl metadata should exist")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_curl, permissions).expect("fake curl should be executable");

    let inherited_path = env::var_os("PATH").unwrap_or_default();
    let path = env::join_paths(
        std::iter::once(tool_directory.clone()).chain(env::split_paths(&inherited_path)),
    )
    .expect("test PATH should be valid");
    let mut command = installer_process(
        directory.path(),
        &home,
        &install_directory,
        &state_directory,
        "https://example.invalid",
    );
    command
        .env("PATH", path)
        .env("NAN_TEST_CURL_ARGUMENTS", &curl_arguments);

    let output = command.output().expect("release installer should start");
    assert!(
        !output.status.success(),
        "installer should fail when the release cannot be downloaded"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("could not download nan-"),
        "unexpected stderr: {stderr}"
    );
    let curl_arguments = fs::read_to_string(curl_arguments)
        .expect("fake curl should record the arguments it received");
    assert_curl_option(&curl_arguments, "--connect-timeout", "10");
    assert_curl_option(&curl_arguments, "--max-time", "120");
    assert_curl_option(&curl_arguments, "--retry-max-time", "10");
    assert!(!install_directory.exists());
}

fn run_installer(
    root: &Path,
    home: &Path,
    install_directory: &Path,
    state_directory: &Path,
    base_url: &str,
) -> Output {
    installer_process(root, home, install_directory, state_directory, base_url)
        .output()
        .expect("release installer should start")
}

fn installer_process(
    root: &Path,
    home: &Path,
    install_directory: &Path,
    state_directory: &Path,
    base_url: &str,
) -> Command {
    let script = repository_root().join(installer_file_name());
    let mut command = installer_command(&script);
    command
        .current_dir(root)
        .env("HOME", home)
        .env("NAN_INSTALL_BASE_URL", base_url)
        .env("NAN_INSTALL_DIR", install_directory)
        .env("NAN_HARNESS_CONFIG_DIR", state_directory)
        .env("NO_PROXY", "127.0.0.1,localhost");
    command
}

#[cfg(unix)]
fn assert_curl_option(arguments: &str, option: &str, value: &str) {
    let arguments = arguments.lines().collect::<Vec<_>>();
    assert!(
        arguments.windows(2).any(|pair| pair == [option, value]),
        "curl arguments should contain {option} {value}: {arguments:?}"
    );
}

fn assert_installation_receipt(state_directory: &Path, binary: &Path, install_directory: &Path) {
    let receipt: serde_json::Value = serde_json::from_slice(
        &fs::read(state_directory.join("installation.json"))
            .expect("installation receipt should exist"),
    )
    .expect("installation receipt should be valid JSON");
    assert_eq!(receipt["schemaVersion"], 1);
    assert_eq!(receipt["executablePath"], binary.to_string_lossy().as_ref());
    assert_eq!(
        receipt["aliasPath"],
        alias_path(install_directory).to_string_lossy().as_ref()
    );
    assert!(receipt["userPathEntryAdded"].is_boolean());
}

fn wait_until_removed(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while path.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(unix)]
fn installer_command(script: &Path) -> Command {
    let mut command = Command::new("sh");
    command.arg(script);
    command
}

#[cfg(windows)]
fn installer_command(script: &Path) -> Command {
    let mut command = Command::new("powershell.exe");
    command.args([
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
    ]);
    command.arg(script);
    command
}

fn assert_version(binary: &Path) {
    let output = Command::new(binary)
        .arg("--version")
        .output()
        .expect("installed binary should start");
    assert_success("installed binary", &output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        format!("nan {}", env!("CARGO_PKG_VERSION"))
    );
}

#[cfg(unix)]
fn assert_alias(install_directory: &Path) {
    let alias = alias_path(install_directory);
    assert_eq!(
        fs::read_link(&alias).expect("alias should be a symbolic link"),
        PathBuf::from("nan")
    );
    assert_version(&alias);
}

#[cfg(windows)]
fn assert_alias(install_directory: &Path) {
    let alias = alias_path(install_directory);
    let contents = fs::read_to_string(&alias).expect("command alias should be readable");
    assert_eq!(contents, "@echo off\r\n\"%~dp0nan.exe\" %*\r\n");
    let output = Command::new("cmd.exe")
        .args(["/D", "/C"])
        .arg(&alias)
        .arg("--version")
        .output()
        .expect("command alias should start");
    assert_success("command alias", &output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        format!("nan {}", env!("CARGO_PKG_VERSION"))
    );
}

#[cfg(unix)]
fn alias_path(install_directory: &Path) -> PathBuf {
    install_directory.join("nan-harness")
}

#[cfg(windows)]
fn alias_path(install_directory: &Path) -> PathBuf {
    install_directory.join("nan-harness.cmd")
}

fn assert_success(label: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{label} failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn serve_all(mut responses: BTreeMap<String, Vec<u8>>) -> (String, ServerHandle) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("release server should bind");
    listener
        .set_nonblocking(true)
        .expect("release server should become nonblocking");
    let address = listener
        .local_addr()
        .expect("release server address should exist");
    let server = thread::spawn(move || {
        let mut deadline = Instant::now() + Duration::from_mins(1);
        while !responses.is_empty() {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false)?;
                    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
                    let mut request = [0_u8; 4096];
                    let length = stream.read(&mut request)?;
                    let request = String::from_utf8_lossy(&request[..length]);
                    let path = request
                        .lines()
                        .next()
                        .and_then(|line| line.split_whitespace().nth(1))
                        .ok_or("release request did not contain a path")?;
                    let body = responses
                        .remove(path)
                        .ok_or_else(|| format!("unexpected release request for {path}"))?;
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )?;
                    stream.write_all(&body)?;
                    stream.flush()?;
                    deadline = Instant::now() + Duration::from_mins(1);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(format!(
                            "release server timed out with {} files pending",
                            responses.len()
                        )
                        .into());
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    });
    (format!("http://{address}"), server)
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn hex_digest(bytes: impl AsRef<[u8]>) -> String {
    let bytes = bytes.as_ref();
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}

#[cfg(windows)]
const fn installer_file_name() -> &'static str {
    "install.ps1"
}

#[cfg(not(windows))]
const fn installer_file_name() -> &'static str {
    "install.sh"
}

#[cfg(windows)]
const fn binary_file_name() -> &'static str {
    "nan.exe"
}

#[cfg(not(windows))]
const fn binary_file_name() -> &'static str {
    "nan"
}

#[cfg(all(target_arch = "aarch64", target_os = "macos"))]
const fn artifact_file_name() -> &'static str {
    "nan-aarch64-apple-darwin"
}

#[cfg(all(target_arch = "x86_64", target_os = "macos"))]
const fn artifact_file_name() -> &'static str {
    "nan-x86_64-apple-darwin"
}

#[cfg(all(target_arch = "aarch64", target_os = "linux"))]
const fn artifact_file_name() -> &'static str {
    "nan-aarch64-unknown-linux-musl"
}

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
const fn artifact_file_name() -> &'static str {
    "nan-x86_64-unknown-linux-musl"
}

#[cfg(all(target_arch = "x86_64", target_os = "windows"))]
const fn artifact_file_name() -> &'static str {
    "nan-x86_64-pc-windows-msvc.exe"
}
