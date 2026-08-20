use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::io::{Read as _, Write as _};
use std::net::TcpListener;
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
    let output = run_installer(directory.path(), &home, &install_directory, &base_url);
    assert_success("installer", &output);
    let server_result = server.join().expect("release server should finish");
    server_result.expect("release server should deliver every file");

    let binary = install_directory.join(binary_file_name());
    assert_version(&binary);
    assert_alias(&install_directory);
}

fn run_installer(root: &Path, home: &Path, install_directory: &Path, base_url: &str) -> Output {
    let script = repository_root().join(installer_file_name());
    let mut command = installer_command(&script);
    command
        .current_dir(root)
        .env("HOME", home)
        .env("NAN_INSTALL_BASE_URL", base_url)
        .env("NAN_INSTALL_DIR", install_directory)
        .env("NO_PROXY", "127.0.0.1,localhost");
    command.output().expect("release installer should start")
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
    let alias = install_directory.join("nan-harness");
    assert_eq!(
        fs::read_link(&alias).expect("alias should be a symbolic link"),
        PathBuf::from("nan")
    );
    assert_version(&alias);
}

#[cfg(windows)]
fn assert_alias(install_directory: &Path) {
    let alias = install_directory.join("nan-harness.cmd");
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
        let mut deadline = Instant::now() + Duration::from_secs(60);
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
                    deadline = Instant::now() + Duration::from_secs(60);
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
