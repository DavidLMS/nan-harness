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
    fs::create_dir_all(&install_directory).expect("install directory should exist");
    let unrelated_nan = unrelated_nan_paths(&install_directory);
    for path in &unrelated_nan {
        fs::write(path, b"unrelated nan command")
            .expect("unrelated nan command should be writable");
    }

    let candidate =
        fs::read(env!("CARGO_BIN_EXE_nan-harness")).expect("candidate should be readable");
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

    let mut command = isolated_command(&binary, directory.path(), &home, &state_directory);
    command
        .args(["__record-installation", "--executable"])
        .arg(&binary)
        .arg("--alias")
        .arg(alias_path(&install_directory));
    let output = command
        .output()
        .expect("installed binary should refresh its receipt");
    assert_success("receipt refresh", &output);

    let mut command = isolated_alias_command(
        &install_directory,
        directory.path(),
        &home,
        &state_directory,
    );
    command.arg("uninstall");
    let output = command
        .output()
        .expect("uninstall should enforce confirmation");
    let stderr = String::from_utf8(output.stderr).expect("uninstall error should be UTF-8");
    assert!(!output.status.success());
    assert!(stderr.contains("error [NH-UNINSTALL-001]"));
    assert!(binary.exists());
    assert!(state_directory.exists());

    fs::write(state_directory.join("test-state"), b"managed data")
        .expect("application state should be writable");
    let mut command = isolated_alias_command(
        &install_directory,
        directory.path(),
        &home,
        &state_directory,
    );
    command.args(["uninstall", "--yes"]);
    let output = command
        .output()
        .expect("installed binary should uninstall itself");
    assert_success("uninstall", &output);
    wait_until_removed(&binary);
    assert!(!binary.exists());
    assert!(!alias_path(&install_directory).exists());
    assert!(!state_directory.exists());
    assert!(!home.join(".hermes/profiles/nan").exists());
    for path in unrelated_nan {
        assert_eq!(
            fs::read(path).expect("unrelated nan command should remain readable"),
            b"unrelated nan command"
        );
    }
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
        stderr.contains("could not download nan-harness-"),
        "unexpected stderr: {stderr}"
    );
    let curl_arguments = fs::read_to_string(curl_arguments)
        .expect("fake curl should record the arguments it received");
    assert_curl_option(&curl_arguments, "--connect-timeout", "10");
    assert_curl_option(&curl_arguments, "--max-time", "120");
    assert_curl_option(&curl_arguments, "--retry-max-time", "10");
    assert!(!install_directory.exists());
}

#[test]
fn release_installer_rejects_an_unrelated_nanh_before_replacing_the_binary() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let install_directory = directory.path().join("bin");
    let state_directory = directory.path().join("state");
    fs::create_dir_all(&home).expect("isolated home should exist");
    fs::create_dir_all(&install_directory).expect("install directory should exist");
    let alias = conflicting_nanh_path(&install_directory);
    fs::write(&alias, b"unrelated nanh command")
        .expect("unrelated nanh command should be writable");
    let binary = install_directory.join(binary_file_name());
    fs::write(&binary, b"existing canonical binary")
        .expect("existing canonical binary should be writable");

    let (base_url, server) = serve_release();
    let output = run_installer(
        directory.path(),
        &home,
        &install_directory,
        &state_directory,
        &base_url,
    );
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("exists and is not the nan-harness command alias")
    );
    server
        .join()
        .expect("release server should finish")
        .expect("release server should deliver every file");
    assert_eq!(
        fs::read(&alias).expect("unrelated nanh command should remain readable"),
        b"unrelated nanh command"
    );
    assert_eq!(
        fs::read(&binary).expect("existing canonical binary should remain readable"),
        b"existing canonical binary"
    );
}

#[test]
fn release_installer_preserves_unrelated_nan_command() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let home = directory.path().join("home");
    let install_directory = directory.path().join("bin");
    let state_directory = directory.path().join("state");
    fs::create_dir_all(&home).expect("isolated home should exist");
    fs::create_dir_all(&install_directory).expect("install directory should exist");
    let unrelated = unrelated_nan_paths(&install_directory);
    for path in &unrelated {
        fs::write(path, b"unrelated nan command")
            .expect("unrelated nan command should be writable");
    }

    let (base_url, server) = serve_release();
    let output = run_installer(
        directory.path(),
        &home,
        &install_directory,
        &state_directory,
        &base_url,
    );
    assert_success("installer with unrelated nan", &output);
    server
        .join()
        .expect("release server should finish")
        .expect("release server should deliver every file");
    for path in unrelated {
        assert_eq!(
            fs::read(path).expect("unrelated command should remain readable"),
            b"unrelated nan command"
        );
    }
    assert_version(&install_directory.join(binary_file_name()));
    assert_alias(&install_directory);
}

fn serve_release() -> (String, ServerHandle) {
    let candidate =
        fs::read(env!("CARGO_BIN_EXE_nan-harness")).expect("candidate should be readable");
    let checksum = hex_digest(Sha256::digest(&candidate));
    let artifact = artifact_file_name();
    serve_all(BTreeMap::from([
        (format!("/{artifact}"), candidate),
        (
            format!("/{artifact}.sha256"),
            format!("{checksum}  {artifact}\n").into_bytes(),
        ),
        (
            "/release-version.txt".to_owned(),
            format!("{}\n", env!("CARGO_PKG_VERSION")).into_bytes(),
        ),
    ]))
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
    isolate_user_environment(&mut command, root, home, state_directory);
    command
        .current_dir(root)
        .env("NAN_INSTALL_BASE_URL", base_url)
        .env("NAN_INSTALL_DIR", install_directory)
        .env("NO_PROXY", "127.0.0.1,localhost");
    command
}

fn isolated_command(
    executable: &Path,
    root: &Path,
    home: &Path,
    state_directory: &Path,
) -> Command {
    let mut command = Command::new(executable);
    isolate_user_environment(&mut command, root, home, state_directory);
    command
}

#[cfg(unix)]
fn isolated_alias_command(
    install_directory: &Path,
    root: &Path,
    home: &Path,
    state_directory: &Path,
) -> Command {
    isolated_command(&alias_path(install_directory), root, home, state_directory)
}

#[cfg(windows)]
fn isolated_alias_command(
    install_directory: &Path,
    root: &Path,
    home: &Path,
    state_directory: &Path,
) -> Command {
    let mut command = Command::new("cmd.exe");
    command
        .args(["/D", "/C"])
        .arg(alias_path(install_directory));
    isolate_user_environment(&mut command, root, home, state_directory);
    command
}

fn isolate_user_environment(
    command: &mut Command,
    root: &Path,
    home: &Path,
    state_directory: &Path,
) {
    let temporary_directory = root.join("tmp");
    let app_data = home.join("AppData/Roaming");
    let local_app_data = home.join("AppData/Local");
    let xdg_config = home.join(".config");
    let xdg_data = home.join(".local/share");
    let xdg_cache = home.join(".cache");
    for directory in [
        &temporary_directory,
        &app_data,
        &local_app_data,
        &xdg_config,
        &xdg_data,
        &xdg_cache,
    ] {
        fs::create_dir_all(directory).expect("isolated user directory should exist");
    }
    command
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("HERMES_HOME", home.join(".hermes"))
        .env("APPDATA", app_data)
        .env("LOCALAPPDATA", local_app_data)
        .env("XDG_CONFIG_HOME", xdg_config)
        .env("XDG_DATA_HOME", xdg_data)
        .env("XDG_CACHE_HOME", xdg_cache)
        .env("NAN_HARNESS_CONFIG_DIR", state_directory)
        .env("TMPDIR", &temporary_directory)
        .env("TMP", &temporary_directory)
        .env("TEMP", temporary_directory);
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
        format!("nan-harness {}", env!("CARGO_PKG_VERSION"))
    );
}

#[cfg(unix)]
fn assert_alias(install_directory: &Path) {
    let alias = alias_path(install_directory);
    assert_eq!(
        fs::read_link(&alias).expect("alias should be a symbolic link"),
        PathBuf::from("nan-harness")
    );
    assert_version(&alias);
}

#[cfg(windows)]
fn assert_alias(install_directory: &Path) {
    let alias = alias_path(install_directory);
    let contents = fs::read_to_string(&alias).expect("command alias should be readable");
    assert_eq!(contents, "@echo off\r\n\"%~dp0nan-harness.exe\" %*\r\n");
    let output = Command::new("cmd.exe")
        .args(["/D", "/C"])
        .arg(&alias)
        .arg("--version")
        .output()
        .expect("command alias should start");
    assert_success("command alias", &output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        format!("nan-harness {}", env!("CARGO_PKG_VERSION"))
    );
}

#[cfg(unix)]
fn alias_path(install_directory: &Path) -> PathBuf {
    install_directory.join("nanh")
}

#[cfg(windows)]
fn alias_path(install_directory: &Path) -> PathBuf {
    install_directory.join("nanh.cmd")
}

#[cfg(unix)]
fn conflicting_nanh_path(install_directory: &Path) -> PathBuf {
    alias_path(install_directory)
}

#[cfg(windows)]
fn conflicting_nanh_path(install_directory: &Path) -> PathBuf {
    install_directory.join("nanh.exe")
}

#[cfg(unix)]
fn unrelated_nan_paths(install_directory: &Path) -> Vec<PathBuf> {
    vec![install_directory.join("nan")]
}

#[cfg(windows)]
fn unrelated_nan_paths(install_directory: &Path) -> Vec<PathBuf> {
    vec![
        install_directory.join("nan.exe"),
        install_directory.join("nan.cmd"),
    ]
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
    "nan-harness.exe"
}

#[cfg(not(windows))]
const fn binary_file_name() -> &'static str {
    "nan-harness"
}

#[cfg(all(target_arch = "aarch64", target_os = "macos"))]
const fn artifact_file_name() -> &'static str {
    "nan-harness-aarch64-apple-darwin"
}

#[cfg(all(target_arch = "x86_64", target_os = "macos"))]
const fn artifact_file_name() -> &'static str {
    "nan-harness-x86_64-apple-darwin"
}

#[cfg(all(target_arch = "aarch64", target_os = "linux"))]
const fn artifact_file_name() -> &'static str {
    "nan-harness-aarch64-unknown-linux-musl"
}

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
const fn artifact_file_name() -> &'static str {
    "nan-harness-x86_64-unknown-linux-musl"
}

#[cfg(all(target_arch = "x86_64", target_os = "windows"))]
const fn artifact_file_name() -> &'static str {
    "nan-harness-x86_64-pc-windows-msvc.exe"
}
