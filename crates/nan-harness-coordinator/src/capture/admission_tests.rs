use super::{
    CaptureLeg, CaptureRequest, MAX_ENCODING_INPUT, RECORD_QUEUE_BYTES, Record, Writer,
    encode_payload, write_records,
};
use nan_harness_private_fs::open_private_new;
use std::fs::File;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::sync::mpsc;

fn request(records: usize, bytes: usize) -> (CaptureRequest, mpsc::Receiver<Record>) {
    let (sender, receiver) = mpsc::channel(records);
    let writer = Arc::new(Writer {
        sender,
        incomplete: Arc::new(AtomicBool::new(false)),
        queued_bytes: Arc::new(AtomicUsize::new(0)),
        byte_capacity: bytes,
        launch_id: Arc::from("launch"),
    });
    (
        CaptureRequest {
            request_id: Arc::from("request"),
            writer,
        },
        receiver,
    )
}

fn reject_without_encoding(request: &CaptureRequest, payload: &[u8]) {
    request.record_with_encoder(CaptureLeg::ProviderResponse, payload, |_, _| {
        panic!("rejected admission must not enter the encoder")
    });
    assert!(request.writer.incomplete.load(Ordering::Relaxed));
}

#[test]
fn full_record_queue_rejects_before_encoding_and_recovers() {
    let (request, mut receiver) = request(1, 4096);
    request.record(CaptureLeg::ProviderResponse, b"first");
    reject_without_encoding(&request, b"second");
    drop(receiver.try_recv().unwrap());
    assert_eq!(request.writer.queued_bytes.load(Ordering::Relaxed), 0);
    request.record(CaptureLeg::ProviderResponse, b"third");
    assert_eq!(receiver.try_recv().unwrap().payload, "third");
}

#[test]
fn full_byte_budget_rejects_before_encoding_and_returns_record_slot() {
    let (request, _receiver) = request(2, 400);
    request.record(CaptureLeg::ProviderResponse, b"first");
    reject_without_encoding(&request, b"second");
    assert_eq!(request.writer.sender.capacity(), 1);
}

#[test]
fn oversized_input_rejects_before_encoding_and_releases_slot() {
    let (request, _receiver) = request(1, RECORD_QUEUE_BYTES);
    reject_without_encoding(&request, &vec![b'x'; MAX_ENCODING_INPUT + 1]);
    assert_eq!(request.writer.sender.capacity(), 1);
    assert_eq!(request.writer.queued_bytes.load(Ordering::Relaxed), 0);
}

#[test]
fn concurrent_encoders_hold_record_and_byte_capacity() {
    for (records, bytes) in [(1, 4096), (2, 400)] {
        let (request, mut receiver) = request(records, bytes);
        let barrier = std::sync::Barrier::new(2);
        let rejected_encoder_entered = AtomicBool::new(false);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                request.record_with_encoder(
                    CaptureLeg::ProviderResponse,
                    b"first",
                    |data, limit| {
                        barrier.wait();
                        barrier.wait();
                        encode_payload(data, limit)
                    },
                );
            });
            barrier.wait();
            request.record_with_encoder(CaptureLeg::ProviderResponse, b"second", |_, _| {
                rejected_encoder_entered.store(true, Ordering::Relaxed);
                None
            });
            barrier.wait();
        });
        assert!(!rejected_encoder_entered.load(Ordering::Relaxed));
        assert!(request.writer.incomplete.load(Ordering::Relaxed));
        assert_eq!(receiver.try_recv().unwrap().payload, "first");
        assert_eq!(request.writer.queued_bytes.load(Ordering::Relaxed), 0);
    }
}

#[test]
fn failed_and_oversized_encoding_release_all_credits() {
    let (request, _receiver) = request(1, 4096);
    request.record_with_encoder(CaptureLeg::ProviderResponse, b"data", |_, _| None);
    request.record_with_encoder(CaptureLeg::ProviderResponse, b"data", |_, limit| {
        Some(("utf8", "x".repeat(limit + 1)))
    });
    assert!(request.writer.incomplete.load(Ordering::Relaxed));
    assert_eq!(request.writer.queued_bytes.load(Ordering::Relaxed), 0);
    assert_eq!(request.writer.sender.capacity(), 1);
}

#[test]
fn closed_receiver_rejects_before_encoding() {
    let (request, receiver) = request(1, 4096);
    drop(receiver);
    reject_without_encoding(&request, b"data");
    assert_eq!(request.writer.queued_bytes.load(Ordering::Relaxed), 0);
}

#[test]
fn receiver_closed_during_encoding_releases_all_credits() {
    let (request, receiver) = request(1, 4096);
    request.record_with_encoder(CaptureLeg::ProviderResponse, b"data", |data, limit| {
        drop(receiver);
        encode_payload(data, limit)
    });
    assert!(request.writer.incomplete.load(Ordering::Relaxed));
    assert_eq!(request.writer.queued_bytes.load(Ordering::Relaxed), 0);
}

#[test]
fn encoding_expansion_is_bounded_and_reservation_shrinks() {
    assert!(encode_payload(&[0xff], 3).is_none());
    assert_eq!(encode_payload(&[0xff], 4).unwrap().1, "/w==");
    assert!(encode_payload(br#"{"cookie":0}"#, 12).is_none());
    let (request, mut receiver) = request(2, 4096);
    request.record(CaptureLeg::ProviderResponse, br#"{"cookie":0}"#);
    let record = receiver.try_recv().unwrap();
    assert_eq!(record.payload, r#"{"cookie":"[REDACTED]"}"#);
    assert_eq!(
        request.writer.queued_bytes.load(Ordering::Relaxed),
        record.payload.len() + 269
    );
    drop(record);
    assert_eq!(request.writer.queued_bytes.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn writer_failure_releases_queued_credits_and_marks_incomplete() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("output");
    std::fs::write(&path, b"").unwrap();
    // A read-only file fails writes portably without using a platform device.
    let file = File::open(&path).unwrap();
    let lock = File::open(&path).unwrap();
    let marker = directory.path().join("output.incomplete");
    let (request, receiver) = request(2, 4096);
    request.record(CaptureLeg::ProviderResponse, b"first");
    request.record(CaptureLeg::ProviderResponse, b"second");
    write_records(
        file,
        lock,
        receiver,
        Arc::clone(&request.writer.incomplete),
        marker.clone(),
    )
    .await;
    assert!(marker.exists());
    assert_eq!(request.writer.queued_bytes.load(Ordering::Relaxed), 0);
    reject_without_encoding(&request, b"third");
}

#[tokio::test]
async fn admission_rejection_persists_incomplete_marker() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("output");
    let file = open_private_new(&path).unwrap();
    let lock = File::open(&path).unwrap();
    let marker = directory.path().join("output.incomplete");
    let (request, receiver) = request(1, 1);
    reject_without_encoding(&request, b"data");
    let incomplete = Arc::clone(&request.writer.incomplete);
    drop(request);
    write_records(file, lock, receiver, incomplete, marker.clone()).await;
    assert!(marker.exists());
}
