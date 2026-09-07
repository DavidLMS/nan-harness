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
// These are logical admission limits, not an allocator/RSS guarantee. Parsing
// is restricted to 8 MiB; each encoder reserves input plus up to six times its
// length for escaped/redacted output. Large records may be rejected even when
// their eventual encoding would fit. Routing must still deliver them intact.
const MAX_ENCODING_INPUT: usize = 8 * 1024 * 1024;

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
    byte_capacity: usize,
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
    _reservation: ByteReservation,
}

struct ByteReservation {
    used: Arc<AtomicUsize>,
    bytes: usize,
}

impl ByteReservation {
    fn acquire(used: &Arc<AtomicUsize>, bytes: usize, capacity: usize) -> Option<Self> {
        used.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            current.checked_add(bytes).filter(|next| *next <= capacity)
        })
        .ok()?;
        Some(Self {
            used: Arc::clone(used),
            bytes,
        })
    }

    fn shrink(&mut self, bytes: usize) {
        self.used.fetch_sub(self.bytes - bytes, Ordering::Relaxed);
        self.bytes = bytes;
    }
}

impl Drop for ByteReservation {
    fn drop(&mut self) {
        self.used.fetch_sub(self.bytes, Ordering::Relaxed);
    }
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
        self.record_with_encoder(leg, payload, encode_payload);
    }

    fn record_with_encoder(
        &self,
        leg: CaptureLeg,
        payload: &[u8],
        encode: impl FnOnce(&[u8], usize) -> Option<(&'static str, String)>,
    ) {
        if self.try_record(leg, payload, encode).is_none() {
            self.mark_incomplete();
        }
    }

    fn try_record(
        &self,
        leg: CaptureLeg,
        payload: &[u8],
        encode: impl FnOnce(&[u8], usize) -> Option<(&'static str, String)>,
    ) -> Option<()> {
        // A channel permit counts encoders in flight as well as queued records.
        let permit = self.writer.sender.try_reserve().ok()?;
        if payload.len() > MAX_ENCODING_INPUT {
            return None;
        }
        let output_limit = payload.len().checked_mul(6)?.checked_add(64)?;
        let metadata = self
            .writer
            .launch_id
            .len()
            .checked_add(self.request_id.len())?
            .checked_add(256)?;
        let bytes = payload
            .len()
            .checked_add(output_limit)?
            .checked_add(metadata)?;
        let mut reservation =
            ByteReservation::acquire(&self.writer.queued_bytes, bytes, self.writer.byte_capacity)?;
        let (encoding, payload) = encode(payload, output_limit)?;
        if payload.len() > output_limit || self.writer.sender.is_closed() {
            return None;
        }
        reservation.shrink(payload.len() + metadata);
        let record = Record {
            schema_version: 1,
            timestamp_unix_millis: now_millis(),
            launch_id: self.writer.launch_id.to_string(),
            request_id: self.request_id.to_string(),
            leg,
            encoding,
            payload,
            _reservation: reservation,
        };
        permit.send(record);
        Some(())
    }

    /// Marks the capture incomplete without recording partial payload data.
    pub fn mark_incomplete(&self) {
        self.writer.incomplete.store(true, Ordering::Relaxed);
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
        incomplete_path,
    ));
    Some(Arc::new(Writer {
        sender,
        incomplete,
        queued_bytes,
        byte_capacity: RECORD_QUEUE_BYTES,
        launch_id: Arc::from(launch_id),
    }))
}

async fn write_records(
    mut file: File,
    lock: File,
    mut receiver: mpsc::Receiver<Record>,
    incomplete: Arc<AtomicBool>,
    incomplete_path: PathBuf,
) {
    while let Some(record) = receiver.recv().await {
        let result = serde_json::to_writer(&mut file, &record)
            .map_err(std::io::Error::other)
            .and_then(|()| file.write_all(b"\n"));
        if result.is_err() {
            incomplete.store(true, Ordering::Relaxed);
            break;
        }
    }
    // Dropping queued records also releases their credits on writer failure.
    drop(receiver);
    if file.flush().is_err() {
        incomplete.store(true, Ordering::Relaxed);
    }
    if incomplete.load(Ordering::Relaxed)
        && let Ok(mut marker) = open_private_new(&incomplete_path)
    {
        let _ = marker.write_all(b"capture incomplete\n");
    }
    drop(lock);
}

fn encode_payload(payload: &[u8], output_limit: usize) -> Option<(&'static str, String)> {
    if let Ok(text) = std::str::from_utf8(payload) {
        if let Ok(mut value) = serde_json::from_str::<Value>(text) {
            redact_sensitive_fields(&mut value);
            let mut output = BoundedOutput {
                bytes: Vec::new(),
                limit: output_limit,
            };
            serde_json::to_writer(&mut output, &value).ok()?;
            return Some(("utf8", String::from_utf8(output.bytes).ok()?));
        }
        return (text.len() <= output_limit).then(|| ("utf8", text.to_owned()));
    }
    let encoded_len = payload
        .len()
        .checked_add(2)?
        .checked_div(3)?
        .checked_mul(4)?;
    if encoded_len > output_limit {
        return None;
    }
    Some((
        "base64",
        base64::engine::general_purpose::STANDARD.encode(payload),
    ))
}

struct BoundedOutput {
    bytes: Vec<u8>,
    limit: usize,
}

impl std::io::Write for BoundedOutput {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.limit - self.bytes.len() {
            return Err(std::io::Error::other("capture encoding limit exceeded"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
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
mod admission_tests;

#[cfg(test)]
mod tests {
    use super::{
        ByteReservation, CaptureLeg, CaptureRequest, RECORD_QUEUE_BYTES, encode_payload,
        start_writer,
    };
    use std::fs;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn structured_credentials_are_redacted_without_removing_prompt_text() {
        let (_, encoded) = encode_payload(
            br#"{"api_key":"secret","messages":[{"content":"keep this prompt"}]}"#,
            1024,
        )
        .unwrap();
        assert!(!encoded.contains("secret"));
        assert!(encoded.contains("keep this prompt"));
    }

    #[test]
    fn binary_payloads_are_preserved_as_base64() {
        let (encoding, payload) = encode_payload(&[0xff, 0x00, 0x01], 4).unwrap();
        assert_eq!(encoding, "base64");
        assert_eq!(payload, "/wAB");
    }

    #[test]
    fn writer_queue_has_a_byte_bound_in_addition_to_its_record_bound() {
        let queued = Arc::new(AtomicUsize::new(0));
        let reservation =
            ByteReservation::acquire(&queued, RECORD_QUEUE_BYTES, RECORD_QUEUE_BYTES).unwrap();
        assert!(ByteReservation::acquire(&queued, 1, RECORD_QUEUE_BYTES).is_none());
        drop(reservation);
        assert_eq!(queued.load(std::sync::atomic::Ordering::Relaxed), 0);
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

    #[tokio::test]
    async fn explicit_incomplete_state_uses_the_private_marker() {
        let temporary = tempfile::tempdir().expect("temporary directory should exist");
        let writer = start_writer(temporary.path(), "capture", "codex")
            .expect("capture writer should start");
        let request = CaptureRequest {
            request_id: Arc::from("request-incomplete"),
            writer: Arc::clone(&writer),
        };
        request.mark_incomplete();
        drop(request);
        drop(writer);

        let capture_directory = temporary.path().join("captures/capture");
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(1);
        let marker = loop {
            let marker = fs::read_dir(&capture_directory)
                .expect("capture directory should be readable")
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .find(|path| {
                    path.extension()
                        .is_some_and(|extension| extension == "incomplete")
                });
            if let Some(marker) = marker {
                break marker;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "incomplete marker should be written"
            );
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        };
        assert_eq!(
            fs::read_to_string(&marker).expect("incomplete marker should be readable"),
            "capture incomplete\n"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = fs::metadata(marker)
                .expect("incomplete marker metadata")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }
}
