//! Loopback release-source fixture: it serves manifests and candidate artifacts to a copied
//! `nan-harness` process, so both release-discovery paths can be observed end to end without a
//! network. Child processes run with an isolated home, configuration, cache and temporary
//! directory, no telemetry endpoints and no inherited continuous-integration marker.

use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::fmt::{self, Write as _};
use std::fs;
use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// Every supported target points at the same fixture artifact, so the copied process finds an
/// entry for whichever target it was built for.
const RELEASE_TARGETS: [&str; 8] = [
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "aarch64-unknown-linux-musl",
    "x86_64-unknown-linux-gnu",
    "x86_64-unknown-linux-musl",
    "aarch64-pc-windows-msvc",
    "x86_64-pc-windows-msvc",
];
const BASE_URL_PLACEHOLDER: &str = "__BASE__";
/// Never inherited by a fixture child: `CI` would disable automatic checks, the others would
/// change the behaviour under test or reach a real endpoint.
pub const UNSET_IN_CHILDREN: [&str; 5] = [
    "CI",
    "NAN_NO_UPDATE_CHECK",
    "NAN_HARNESS_GLITCHTIP_DSN",
    "NAN_HARNESS_UMAMI_URL",
    "NAN_COMPATIBILITY_MANIFEST_URL",
];

pub type Response = (u16, Vec<u8>);
pub type Route = (String, Response);

pub struct Fixture {
    directory: tempfile::TempDir,
    executable: PathBuf,
    base_url: String,
    requests: Arc<Mutex<Vec<String>>>,
    server: Option<Server>,
}

impl Fixture {
    pub fn new(routes: Vec<Route>) -> Self {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let executable = copied_executable(directory.path());
        let server = Server::start(routes);
        let base_url = server.base_url.clone();
        let requests = Arc::clone(&server.requests);
        Self {
            directory,
            executable,
            base_url,
            requests,
            server: Some(server),
        }
    }

    pub fn run_update(&self) -> Output {
        let mut command = Command::new(&self.executable);
        command.arg("update");
        self.isolate(&mut command);
        Output::capture(&mut command)
    }

    pub fn installed_version(&self) -> String {
        let output = Command::new(&self.executable)
            .arg("--version")
            .output()
            .expect("installed nan-harness should start");
        String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .last()
            .unwrap_or_default()
            .to_owned()
    }

    pub fn state_path(&self) -> PathBuf {
        self.config_path().join("update.json")
    }

    pub fn config_path(&self) -> PathBuf {
        self.directory.path().join("config")
    }

    pub fn home_path(&self) -> PathBuf {
        self.directory.path().join("home")
    }

    #[cfg(unix)]
    pub fn executable_path(&self) -> &Path {
        &self.executable
    }

    #[cfg(unix)]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// The paths the fixture server has been asked for, in order.
    pub fn requests(&self) -> Vec<String> {
        self.requests
            .lock()
            .expect("request log should not be poisoned")
            .clone()
    }

    /// Applies the isolated child environment and the fixture release sources.
    pub fn isolate(&self, command: &mut Command) {
        for (name, value) in self.environment() {
            command.env(name, value);
        }
        for (name, value) in self.sources() {
            command.env(name, value);
        }
        command.env("NO_PROXY", "127.0.0.1,localhost");
        for name in UNSET_IN_CHILDREN {
            command.env_remove(name);
        }
    }

    /// The isolated environment as name/value pairs, for callers that build their own process.
    pub fn environment(&self) -> BTreeMap<String, PathBuf> {
        let root = self.directory.path();
        let home = self.home_path();
        let temporary = root.join("tmp");
        let entries = BTreeMap::from([
            ("HOME".to_owned(), home.clone()),
            ("USERPROFILE".to_owned(), home.clone()),
            ("APPDATA".to_owned(), home.join("AppData/Roaming")),
            ("LOCALAPPDATA".to_owned(), home.join("AppData/Local")),
            ("XDG_CONFIG_HOME".to_owned(), home.join(".config")),
            ("XDG_DATA_HOME".to_owned(), home.join(".local/share")),
            ("XDG_CACHE_HOME".to_owned(), home.join(".cache")),
            ("NAN_HARNESS_CONFIG_DIR".to_owned(), self.config_path()),
            ("TMPDIR".to_owned(), temporary.clone()),
            ("TMP".to_owned(), temporary.clone()),
            ("TEMP".to_owned(), temporary),
        ]);
        for directory in entries.values() {
            fs::create_dir_all(directory).expect("isolated user directory should exist");
        }
        entries
    }

    /// The release sources this fixture serves, as the environment the binary reads them from.
    pub fn sources(&self) -> [(String, String); 2] {
        [
            (
                "NAN_UPDATE_MANIFEST_URL".to_owned(),
                format!("{}/recommended.json", self.base_url),
            ),
            (
                "NAN_UPDATE_AVAILABLE_MANIFEST_URL".to_owned(),
                format!("{}/available.json", self.base_url),
            ),
        ]
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(server) = self.server.take() {
            server.stop();
        }
    }
}

pub struct Output {
    success: bool,
    code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    fn capture(command: &mut Command) -> Self {
        let output = command.output().expect("copied nan-harness should start");
        Self {
            success: output.status.success(),
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

    pub const fn succeeded(&self) -> bool {
        self.success
    }
}

impl fmt::Display for Output {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "exit {:?}\nstdout:\n{}\nstderr:\n{}",
            self.code, self.stdout, self.stderr
        )
    }
}

pub fn manifest_response(version: &str, candidate: &[u8]) -> Response {
    let checksum = hex_digest(Sha256::digest(candidate));
    let mut artifacts = String::new();
    for (index, target) in RELEASE_TARGETS.iter().enumerate() {
        let separator = if index == 0 { "" } else { "," };
        write!(
            artifacts,
            "{separator}{{\"target\":\"{target}\",\"url\":\"{BASE_URL_PLACEHOLDER}/nan\",\"sha256\":\"{checksum}\"}}"
        )
        .expect("writing to a String cannot fail");
    }
    let document = format!(
        "{{\"schemaVersion\":1,\"version\":\"{version}\",\
         \"notesUrl\":\"https://example.com/releases/{version}\",\"artifacts\":[{artifacts}]}}"
    );
    (200, document.into_bytes())
}

#[cfg(unix)]
pub const fn missing_response() -> Response {
    (404, Vec::new())
}

pub const fn failing_response() -> Response {
    (500, Vec::new())
}

#[cfg(unix)]
pub fn candidate_script(version: &str) -> Vec<u8> {
    format!("#!/bin/sh\nprintf '%s\\n' 'nan-harness {version}'\n").into_bytes()
}

#[cfg(windows)]
pub fn candidate_script(_version: &str) -> Vec<u8> {
    fs::read(env!("CARGO_BIN_EXE_nan-harness")).expect("candidate binary should be readable")
}

struct Server {
    base_url: String,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    handle: thread::JoinHandle<()>,
}

impl Server {
    fn start(routes: Vec<Route>) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("fixture server should bind");
        listener
            .set_nonblocking(true)
            .expect("fixture server should become nonblocking");
        let address = listener
            .local_addr()
            .expect("fixture server address should exist");
        let base_url = format!("http://{address}");
        let stop = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let handle = {
            let stop = Arc::clone(&stop);
            let requests = Arc::clone(&requests);
            let base_url = base_url.clone();
            thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => answer(stream, &routes, &base_url, &requests),
                        Err(_) => thread::sleep(Duration::from_millis(10)),
                    }
                }
            })
        };
        Self {
            base_url,
            requests,
            stop,
            handle,
        }
    }

    fn stop(self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.handle.join();
    }
}

fn answer(
    mut stream: std::net::TcpStream,
    routes: &[Route],
    base_url: &str,
    requests: &Mutex<Vec<String>>,
) {
    let Ok(()) = stream.set_nonblocking(false) else {
        return;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut request = [0_u8; 2048];
    let Ok(length) = stream.read(&mut request) else {
        return;
    };
    let path = requested_path(&request[..length]);
    if let Ok(mut log) = requests.lock() {
        log.push(path.clone());
    }
    let (status, body) = routes
        .iter()
        .find(|(route, _)| *route == path)
        .map_or_else(|| (404, Vec::new()), |(_, response)| response.clone());
    let body = replace_placeholder(&body, base_url);
    let _ = write!(
        stream,
        "HTTP/1.1 {status} OK\r\nContent-Type: application/octet-stream\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(&body);
    let _ = stream.flush();
}

fn requested_path(request: &[u8]) -> String {
    String::from_utf8_lossy(request)
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or_default()
        .to_owned()
}

fn replace_placeholder(body: &[u8], base_url: &str) -> Vec<u8> {
    match std::str::from_utf8(body) {
        Ok(text) if text.contains(BASE_URL_PLACEHOLDER) => {
            text.replace(BASE_URL_PLACEHOLDER, base_url).into_bytes()
        }
        _ => body.to_vec(),
    }
}

fn copied_executable(directory: &Path) -> PathBuf {
    let source = Path::new(env!("CARGO_BIN_EXE_nan-harness"));
    let file_name = source
        .file_name()
        .expect("release binary should have a file name");
    let target = directory.join(file_name);
    fs::copy(source, &target).expect("release binary should be copied");
    target
}

fn hex_digest(bytes: impl AsRef<[u8]>) -> String {
    let bytes = bytes.as_ref();
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}
