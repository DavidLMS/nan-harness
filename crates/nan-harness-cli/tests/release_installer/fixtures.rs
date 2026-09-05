use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::thread;
use std::time::{Duration, Instant};

type ServerError = Box<dyn std::error::Error + Send + Sync>;
type ServerHandle = thread::JoinHandle<Result<(), ServerError>>;

pub(crate) fn serve_release() -> (String, ServerHandle) {
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

fn serve_all(mut responses: BTreeMap<String, Vec<u8>>) -> (String, ServerHandle) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("release server should bind");
    listener
        .set_nonblocking(true)
        .expect("release server should become nonblocking");
    let address = listener
        .local_addr()
        .expect("release server address should exist");
    let server = thread::spawn(move || {
        let mut deadline = Instant::now() + Duration::from_mins(2);
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
                    deadline = Instant::now() + Duration::from_mins(2);
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

fn hex_digest(bytes: impl AsRef<[u8]>) -> String {
    let bytes = bytes.as_ref();
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
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
