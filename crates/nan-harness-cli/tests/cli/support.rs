use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Output};
use std::thread;
use std::time::Duration;

pub(crate) fn run(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nan-harness"))
        .args(arguments)
        .output()
        .expect("nan-harness should start")
}

pub(crate) fn run_alias(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nan"))
        .args(arguments)
        .output()
        .expect("nan alias should start")
}

pub(crate) fn run_with_embedded_compatibility(arguments: &[&str]) -> Output {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    Command::new(env!("CARGO_BIN_EXE_nan-harness"))
        .args(arguments)
        .env("NAN_HARNESS_CONFIG_DIR", directory.path())
        .env("NAN_NO_COMPATIBILITY_CHECK", "1")
        .output()
        .expect("nan-harness should start")
}

#[cfg(unix)]
pub(crate) fn run_from_removed_cwd(
    cwd: &std::path::Path,
    state: &std::path::Path,
    arguments: &[&str],
) -> Output {
    let mut command = Command::new("sh");
    command
        .args([
            "-c",
            "cd \"$1\" && rmdir \"$1\" && shift && exec \"$@\"",
            "sh",
            cwd.to_str().expect("cwd should be UTF-8"),
            env!("CARGO_BIN_EXE_nan-harness"),
        ])
        .args(arguments)
        .env("HOME", state.join("home"))
        .env("NAN_HARNESS_CONFIG_DIR", state)
        .env("NAN_NO_COMPATIBILITY_CHECK", "1")
        .env("NAN_HARNESS_CREDENTIAL_BACKEND", "file")
        .env_remove("NAN_API_KEY")
        .output()
        .expect("nan-harness should start from the removed cwd")
}

pub(crate) fn config_command(
    home: &std::path::Path,
    state: &std::path::Path,
    base_url: &str,
) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_nan"));
    command
        .env("HOME", home)
        .env("NAN_HARNESS_CONFIG_DIR", state)
        .env("NAN_HARNESS_CREDENTIAL_BACKEND", "file")
        .env("NAN_NO_COMPATIBILITY_CHECK", "1")
        .env("NAN_BASE_URL", base_url)
        .env_remove("NAN_API_KEY");
    command
}

pub(crate) fn write_private_credential_fixture(state: &std::path::Path, api_key: &str) {
    let key_path = state.join("nan-api-key");
    let receipt_path = state.join("credential.json");
    std::fs::write(&key_path, api_key).expect("credential should be written");
    std::fs::write(
        &receipt_path,
        r#"{"schemaVersion":1,"backend":"private-file"}"#,
    )
    .expect("credential receipt should be written");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        for path in [&key_path, &receipt_path] {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                .expect("credential fixture should be private");
        }
    }
}

#[cfg(unix)]
pub(crate) fn run_direct_model_launch(
    explicit_model: Option<&str>,
    disable_gateway: bool,
    preferences: Option<&str>,
) -> (tempfile::TempDir, Output, String) {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let home = directory.path().join("home");
    let state = directory.path().join("state");
    std::fs::create_dir_all(&home).expect("home directory should be created");
    std::fs::create_dir_all(&state).expect("state directory should be created");
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700))
        .expect("state directory should be private");
    write_private_credential_fixture(&state, "local-test-key");
    if let Some(preferences) = preferences {
        let path = state.join("preferences.json");
        std::fs::write(&path, preferences).expect("preferences should be written");
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .expect("preferences should be private");
    }

    let response = r#"{"data":[{"id":"qwen3.6"}]}"#;
    let (endpoint, request) = capture_one_http_request_with_response(response);
    let executable = fake_harness(directory.path(), "0.84.2");
    let mut command = Command::new(env!("CARGO_BIN_EXE_nan"));
    command.args([
        "pi",
        "--executable",
        executable.to_str().expect("path should be UTF-8"),
        "--provider-base-url",
        &format!("{endpoint}/v1"),
        "--no-search",
    ]);
    if let Some(model) = explicit_model {
        command.args(["--model", model]);
    }
    if disable_gateway {
        command.arg("--no-chat-gateway");
    }
    let output = command
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("NAN_HARNESS_CONFIG_DIR", &state)
        .env("NAN_HARNESS_CREDENTIAL_BACKEND", "file")
        .env("NAN_NO_COMPATIBILITY_CHECK", "1")
        .env_remove("NAN_API_KEY")
        .env_remove("NAN_UPDATE_MANIFEST_URL")
        .env_remove("NAN_HARNESS_GLITCHTIP_DSN")
        .output()
        .expect("nan should start");
    let request = request.join().expect("model request should finish");
    (directory, output, request)
}

pub(crate) fn capture_one_http_request() -> (String, thread::JoinHandle<String>) {
    capture_one_http_request_with_response("{}")
}

pub(crate) fn capture_one_http_request_with_response(
    response_body: &'static str,
) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("capture listener should bind");
    let address = listener
        .local_addr()
        .expect("listener address should exist");
    let request = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("request should connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .expect("read timeout should configure");
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        let expected_length = loop {
            let read = stream
                .read(&mut buffer)
                .expect("request should be readable");
            assert_ne!(read, 0, "request ended before its headers");
            request.extend_from_slice(&buffer[..read]);
            if let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..header_end]);
                let content_length = headers.lines().find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                });
                break header_end + 4 + content_length.unwrap_or(0);
            }
        };
        while request.len() < expected_length {
            let read = stream.read(&mut buffer).expect("body should be readable");
            assert_ne!(read, 0, "request ended before its body");
            request.extend_from_slice(&buffer[..read]);
        }
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response_body}",
            response_body.len()
        );
        stream
            .write_all(response.as_bytes())
            .expect("response should be writable");
        String::from_utf8(request).expect("request should be UTF-8")
    });
    (format!("http://{address}"), request)
}

#[cfg(unix)]
pub(crate) fn fake_claude(directory: &std::path::Path) -> std::path::PathBuf {
    fake_claude_with_version(directory, "2.1.233 (Claude Code)")
}

#[cfg(unix)]
pub(crate) fn fake_claude_with_version(
    directory: &std::path::Path,
    version: &str,
) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let executable = directory.join("claude");
    std::fs::write(
        &executable,
        format!("#!/bin/sh\nprintf '%s\\n' '{version}'\n"),
    )
    .expect("fake executable should be written");
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
        .expect("fake executable should be executable");
    executable
}

#[cfg(unix)]
pub(crate) fn fake_harness(directory: &std::path::Path, version: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let executable = directory.join("fake-harness");
    std::fs::write(
        &executable,
        format!("#!/bin/sh\nprintf '%s\\n' '{version}'\n"),
    )
    .expect("fake executable should be written");
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
        .expect("fake executable should be executable");
    executable
}
