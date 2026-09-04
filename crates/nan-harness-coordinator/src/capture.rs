use crate::diagnostics::active_capture;
use crate::paths::private_directory;
use base64::Engine as _;
use nan_harness_private_fs::{open_private_new, open_private_truncate};
use serde::Serialize;
use serde_json::Value;
use std::fs::File;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;

const RECORD_QUEUE_CAPACITY: usize = 256;
const RECORD_QUEUE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureLeg {
    HarnessRequest,
    ProviderRequest,
    ProviderResponse,
    HarnessResponse,
    Coordinator,
}

#[derive(Clone)]
pub struct CaptureSink {
    launch_id: Arc<str>,
    writer: Arc<Mutex<WriterSlot>>,
    enabled: bool,
}

type WriterSlot = Option<(String, Weak<Writer>)>;

#[derive(Clone)]
pub struct CaptureRequest {
    request_id: Arc<str>,
    writer: Arc<Writer>,
}

struct Writer {
    sender: mpsc::Sender<Record>,
    incomplete: Arc<AtomicBool>,
    queued_bytes: Arc<AtomicUsize>,
    launch_id: Arc<str>,
}

#[derive(Serialize)]
struct Record {
    schema_version: u8,
    timestamp_unix_millis: u128,
    launch_id: String,
    request_id: String,
    leg: CaptureLeg,
    encoding: &'static str,
    payload: String,
    #[serde(skip)]
    byte_len: usize,
}

impl CaptureSink {
    #[must_use]
    pub fn new(launch_id: impl Into<String>) -> Self {
        Self {
            launch_id: launch_id.into().into(),
            writer: Arc::new(Mutex::new(None)),
            enabled: crate::paths::is_managed_process(),
        }
    }

    #[must_use]
    pub fn begin_request(&self, request_id: impl Into<String>) -> Option<CaptureRequest> {
        if !self.enabled {
            return None;
        }
        let (directory, settings) = active_capture()?;
        let capture_id = settings.capture_id?;
        let writer = self.writer_for(&directory, &capture_id)?;
        Some(CaptureRequest {
            request_id: request_id.into().into(),
            writer,
        })
    }

    fn writer_for(&self, directory: &Path, capture_id: &str) -> Option<Arc<Writer>> {
        let mut current = self.writer.lock().ok()?;
        if let Some((existing_id, writer)) = current.as_ref()
            && existing_id == capture_id
            && let Some(writer) = writer.upgrade()
        {
            return Some(writer);
        }
        let writer = start_writer(directory, capture_id, &self.launch_id)?;
        *current = Some((capture_id.to_owned(), Arc::downgrade(&writer)));
        Some(writer)
    }
}

impl CaptureRequest {
    pub fn record(&self, leg: CaptureLeg, payload: &[u8]) {
        let (encoding, payload) = encode_payload(payload);
        let byte_len = payload.len().saturating_add(256);
        if !reserve_bytes(&self.writer.queued_bytes, byte_len) {
            self.writer.incomplete.store(true, Ordering::Relaxed);
            return;
        }
        let record = Record {
            schema_version: 1,
            timestamp_unix_millis: now_millis(),
            launch_id: self.writer.launch_id.to_string(),
            request_id: self.request_id.to_string(),
            leg,
            encoding,
            payload,
            byte_len,
        };
        if self.writer.sender.try_send(record).is_err() {
            self.writer
                .queued_bytes
                .fetch_sub(byte_len, Ordering::Relaxed);
            self.writer.incomplete.store(true, Ordering::Relaxed);
        }
    }
}

fn start_writer(directory: &Path, capture_id: &str, launch_id: &str) -> Option<Arc<Writer>> {
    let capture_directory = directory.join("captures").join(capture_id);
    private_directory(&capture_directory).ok()?;
    let lock_path = directory.join("capture.lock");
    let lock = open_private_truncate(&lock_path).ok()?;
    lock.try_lock_shared().ok()?;
    let suffix = random_suffix()?;
    let file_path = capture_directory.join(format!(
        "launch-{}-{}-{suffix}.jsonl",
        safe_component(launch_id),
        std::process::id()
    ));
    let file = open_private_new(&file_path).ok()?;
    let incomplete_path = file_path.with_extension("incomplete");
    let (sender, receiver) = mpsc::channel(RECORD_QUEUE_CAPACITY);
    let incomplete = Arc::new(AtomicBool::new(false));
    let queued_bytes = Arc::new(AtomicUsize::new(0));
    tokio::spawn(write_records(
        file,
        lock,
        receiver,
        Arc::clone(&incomplete),
        Arc::clone(&queued_bytes),
        incomplete_path,
    ));
    Some(Arc::new(Writer {
        sender,
        incomplete,
        queued_bytes,
        launch_id: Arc::from(launch_id),
    }))
}

async fn write_records(
    mut file: File,
    lock: File,
    mut receiver: mpsc::Receiver<Record>,
    incomplete: Arc<AtomicBool>,
    queued_bytes: Arc<AtomicUsize>,
    incomplete_path: PathBuf,
) {
    while let Some(record) = receiver.recv().await {
        let byte_len = record.byte_len;
        let result = serde_json::to_writer(&mut file, &record)
            .map_err(std::io::Error::other)
            .and_then(|()| file.write_all(b"\n"));
        queued_bytes.fetch_sub(byte_len, Ordering::Relaxed);
        if result.is_err() {
            incomplete.store(true, Ordering::Relaxed);
            break;
        }
    }
    let _ = file.flush();
    if incomplete.load(Ordering::Relaxed)
        && let Ok(mut marker) = open_private_new(&incomplete_path)
    {
        let _ = marker.write_all(b"capture incomplete\n");
    }
    drop(lock);
}

fn reserve_bytes(queued_bytes: &AtomicUsize, byte_len: usize) -> bool {
    queued_bytes
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            current
                .checked_add(byte_len)
                .filter(|next| *next <= RECORD_QUEUE_BYTES)
        })
        .is_ok()
}

fn encode_payload(payload: &[u8]) -> (&'static str, String) {
    if let Ok(text) = std::str::from_utf8(payload) {
        if let Ok(mut value) = serde_json::from_str::<Value>(text) {
            redact_sensitive_fields(&mut value);
            if let Ok(serialized) = serde_json::to_string(&value) {
                return ("utf8", serialized);
            }
        }
        return ("utf8", text.to_owned());
    }
    (
        "base64",
        base64::engine::general_purpose::STANDARD.encode(payload),
    )
}

fn redact_sensitive_fields(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            for (name, child) in fields {
                if is_sensitive_name(name) {
                    *child = Value::String("[REDACTED]".to_owned());
                } else {
                    redact_sensitive_fields(child);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(redact_sensitive_fields),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn is_sensitive_name(name: &str) -> bool {
    let normalized = name.to_ascii_lowercase().replace(['-', '_'], "");
    matches!(
        normalized.as_str(),
        "authorization" | "apikey" | "accesstoken" | "sessiontoken" | "cookie" | "setcookie"
    )
}

fn safe_component(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .take(96)
        .collect()
}

fn random_suffix() -> Option<u64> {
    let mut bytes = [0_u8; 8];
    getrandom::fill(&mut bytes).ok()?;
    Some(u64::from_le_bytes(bytes))
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::{
        CaptureLeg, CaptureRequest, RECORD_QUEUE_BYTES, encode_payload, reserve_bytes, start_writer,
    };
    use std::fs;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn structured_credentials_are_redacted_without_removing_prompt_text() {
        let (_, encoded) =
            encode_payload(br#"{"api_key":"secret","messages":[{"content":"keep this prompt"}]}"#);
        assert!(!encoded.contains("secret"));
        assert!(encoded.contains("keep this prompt"));
    }

    #[test]
    fn binary_payloads_are_preserved_as_base64() {
        let (encoding, payload) = encode_payload(&[0xff, 0x00, 0x01]);
        assert_eq!(encoding, "base64");
        assert_eq!(payload, "/wAB");
    }

    #[test]
    fn writer_queue_has_a_byte_bound_in_addition_to_its_record_bound() {
        let queued = AtomicUsize::new(0);
        assert!(reserve_bytes(&queued, RECORD_QUEUE_BYTES));
        assert!(!reserve_bytes(&queued, 1));
    }

    #[tokio::test]
    async fn writer_persists_redacted_records_and_releases_its_lock() {
        let temporary = tempfile::tempdir().expect("temporary directory should exist");
        let writer = start_writer(temporary.path(), "capture", "codex")
            .expect("capture writer should start");
        let request = CaptureRequest {
            request_id: Arc::from("request-one"),
            writer: Arc::clone(&writer),
        };
        request.record(
            CaptureLeg::ProviderRequest,
            br#"{"authorization":"secret","prompt":"keep"}"#,
        );
        drop(request);
        drop(writer);

        let capture_directory = temporary.path().join("captures/capture");
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(1);
        let payload = loop {
            let captured = fs::read_dir(&capture_directory)
                .expect("capture directory should be readable")
                .filter_map(Result::ok)
                .find(|entry| {
                    entry
                        .path()
                        .extension()
                        .is_some_and(|value| value == "jsonl")
                })
                .and_then(|entry| fs::read_to_string(entry.path()).ok())
                .filter(|contents| !contents.is_empty());
            if let Some(payload) = captured {
                break payload;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "writer should flush"
            );
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        };
        assert!(payload.contains("[REDACTED]"));
        assert!(payload.contains("keep"));
        assert!(!payload.contains("secret"));

        loop {
            let lock = nan_harness_private_fs::open_private_truncate(
                &temporary.path().join("capture.lock"),
            )
            .expect("capture lock should reopen");
            if lock.try_lock().is_ok() {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "writer lock should be released"
            );
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }
}
