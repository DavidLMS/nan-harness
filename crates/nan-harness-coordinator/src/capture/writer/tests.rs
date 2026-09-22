use super::{BATCH_RECORDS, CONCURRENT_BATCHES, Storage};
use crate::capture::admission_tests::request;
use crate::capture::{CaptureLeg, CaptureRequest};
use nan_harness_private_fs::open_private_new;
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;
use tokio::sync::{Semaphore, oneshot};

const DEADLINE: Duration = Duration::from_secs(5);

#[derive(Default)]
struct TestFile {
    bytes: Arc<Mutex<Vec<u8>>>,
    pause: Option<(oneshot::Sender<()>, mpsc::Receiver<()>)>,
    fail_write: bool,
    fail_flush: bool,
}

impl Write for TestFile {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if let Some((entered, release)) = self.pause.take() {
            let _ = entered.send(());
            release.recv_timeout(DEADLINE).map_err(io::Error::other)?;
        }
        if self.fail_write {
            return Err(io::Error::other("synthetic write failure"));
        }
        self.bytes.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.fail_flush {
            Err(io::Error::other("synthetic flush failure"))
        } else {
            Ok(())
        }
    }
}

struct Release(mpsc::Sender<()>);

impl Drop for Release {
    fn drop(&mut self) {
        // A failed assertion must also release a blocked test writer.
        let _ = self.0.send(());
    }
}

fn paused_file() -> (TestFile, oneshot::Receiver<()>, Release) {
    let (entered, waiting) = oneshot::channel();
    let (release, receiver) = mpsc::channel();
    (
        TestFile {
            pause: Some((entered, receiver)),
            ..TestFile::default()
        },
        waiting,
        Release(release),
    )
}

fn storage(directory: &Path, file: TestFile) -> (CaptureRequest, Storage<TestFile>) {
    let (request, receiver) = request(256, 64 * 1024 * 1024);
    let lock = open_private_new(&directory.join("lock")).unwrap();
    lock.try_lock_shared().unwrap();
    let storage = Storage {
        file,
        lock,
        receiver,
        incomplete: Arc::clone(&request.writer.incomplete),
        incomplete_path: directory.join("output.incomplete"),
    };
    (request, storage)
}

async fn completes<T>(future: impl Future<Output = T>) -> T {
    tokio::time::timeout(DEADLINE, future)
        .await
        .expect("capture operation should complete")
}

#[tokio::test(flavor = "current_thread")]
async fn slow_storage_allows_async_progress_and_preserves_order_across_batches() {
    let directory = tempfile::tempdir().unwrap();
    let (file, entered, release) = paused_file();
    let bytes = Arc::clone(&file.bytes);
    let (request, storage) = storage(directory.path(), file);
    let credits = Arc::clone(&request.writer.queued_bytes);
    let count = BATCH_RECORDS * 2 + 1;
    for index in 0..count {
        request.record(CaptureLeg::ProviderResponse, index.to_string().as_bytes());
    }
    let task = tokio::spawn(storage.run(Arc::new(Semaphore::new(1))));
    completes(entered).await.unwrap();
    let heartbeat = completes(tokio::spawn(async {
        tokio::task::yield_now().await;
        42
    }))
    .await
    .unwrap();
    assert_eq!(heartbeat, 42);
    assert!(credits.load(Ordering::Relaxed) > 0);
    assert!(
        std::fs::File::open(directory.path().join("lock"))
            .unwrap()
            .try_lock()
            .is_err()
    );
    drop(request);
    drop(release);
    completes(task).await.unwrap();
    assert_eq!(credits.load(Ordering::Relaxed), 0);
    let output = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
    let records: Vec<serde_json::Value> = output
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(records.len(), count);
    assert!(output.ends_with('\n'));
    for (index, record) in records.iter().enumerate() {
        assert_eq!(record["payload"], index.to_string());
        assert_eq!(record["schema_version"], 1);
    }
    assert!(!directory.path().join("output.incomplete").exists());
    std::fs::File::open(directory.path().join("lock"))
        .unwrap()
        .try_lock()
        .unwrap();
}

#[tokio::test]
async fn write_and_flush_failures_finalize_and_release_credits() {
    for fail_write in [true, false] {
        let directory = tempfile::tempdir().unwrap();
        let (request, storage) = storage(
            directory.path(),
            TestFile {
                fail_write,
                fail_flush: !fail_write,
                ..TestFile::default()
            },
        );
        request.record(CaptureLeg::ProviderResponse, b"first");
        request.record(CaptureLeg::ProviderResponse, b"second");
        let credits = Arc::clone(&request.writer.queued_bytes);
        let incomplete = Arc::clone(&request.writer.incomplete);
        let task = tokio::spawn(storage.run(Arc::new(Semaphore::new(1))));
        if fail_write {
            // Failure closes admission even while producers remain alive.
            completes(request.writer.sender.closed()).await;
            request.record(CaptureLeg::ProviderResponse, b"rejected");
        }
        drop(request);
        completes(task).await.unwrap();
        assert_eq!(credits.load(Ordering::Relaxed), 0);
        assert!(incomplete.load(Ordering::Relaxed));
        assert_eq!(
            std::fs::read(directory.path().join("output.incomplete")).unwrap(),
            b"capture incomplete\n"
        );
        std::fs::File::open(directory.path().join("lock"))
            .unwrap()
            .try_lock()
            .unwrap();
    }
}

#[tokio::test]
async fn idle_writer_holds_no_blocking_slot_and_closes_without_records() {
    let directory = tempfile::tempdir().unwrap();
    let (request, storage) = storage(directory.path(), TestFile::default());
    let slots = Arc::new(Semaphore::new(1));
    let task = tokio::spawn(storage.run(Arc::clone(&slots)));
    tokio::task::yield_now().await;
    assert_eq!(slots.available_permits(), 1);
    assert!(!task.is_finished());
    request.mark_incomplete();
    drop(request);
    completes(task).await.unwrap();
    assert!(directory.path().join("output.incomplete").exists());
    assert_eq!(slots.available_permits(), 1);
}

#[tokio::test]
async fn concurrent_writers_share_a_bounded_blocking_budget() {
    let slots = Arc::new(Semaphore::new(CONCURRENT_BATCHES));
    let mut directories = Vec::new();
    let mut releases = Vec::new();
    let mut tasks = Vec::new();
    for _ in 0..CONCURRENT_BATCHES {
        let directory = tempfile::tempdir().unwrap();
        let (file, entered, release) = paused_file();
        let (request, storage) = storage(directory.path(), file);
        request.record(CaptureLeg::ProviderResponse, b"first");
        drop(request);
        tasks.push(tokio::spawn(storage.run(Arc::clone(&slots))));
        releases.push(release);
        directories.push(directory);
        completes(entered).await.unwrap();
    }
    assert_eq!(slots.available_permits(), 0);
    let directory = tempfile::tempdir().unwrap();
    let (file, mut entered, release) = paused_file();
    let (request, storage) = storage(directory.path(), file);
    request.record(CaptureLeg::ProviderResponse, b"waiting");
    drop(request);
    tasks.push(tokio::spawn(storage.run(Arc::clone(&slots))));
    tokio::task::yield_now().await;
    assert!(matches!(
        entered.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    ));
    drop(releases);
    completes(entered).await.unwrap();
    drop(release);
    for task in tasks {
        completes(task).await.unwrap();
    }
    assert_eq!(slots.available_permits(), CONCURRENT_BATCHES);
}

#[tokio::test]
async fn cancelled_writer_releases_queued_credits_and_lock_after_active_batch() {
    let directory = tempfile::tempdir().unwrap();
    let (file, entered, release) = paused_file();
    let (request, storage) = storage(directory.path(), file);
    let credits = Arc::clone(&request.writer.queued_bytes);
    let slots = Arc::new(Semaphore::new(1));
    for _ in 0..BATCH_RECORDS + 2 {
        request.record(CaptureLeg::ProviderResponse, b"queued");
    }
    let task = tokio::spawn(storage.run(Arc::clone(&slots)));
    completes(entered).await.unwrap();
    task.abort();
    assert!(completes(task).await.unwrap_err().is_cancelled());
    drop(release);
    completes(request.writer.sender.closed()).await;
    // Sender closure and credit release can precede the end of detached task
    // cleanup. Observe the lock itself instead of using either as a completion
    // signal for storage ownership.
    let lock = std::fs::File::open(directory.path().join("lock")).unwrap();
    completes(async {
        while lock.try_lock().is_err() {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        while credits.load(Ordering::Relaxed) != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await;
}

#[test]
fn runtime_shutdown_child() {
    let Ok(mode) = std::env::var("NAN_CAPTURE_SHUTDOWN_TEST") else {
        return;
    };
    let directory = tempfile::tempdir().unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (file, entered, release) = paused_file();
    let (request, storage) = storage(directory.path(), file);
    let task = runtime.spawn(storage.run(Arc::new(Semaphore::new(1))));
    if mode == "active" {
        request.record(CaptureLeg::ProviderResponse, b"first");
        runtime.block_on(completes(entered)).unwrap();
    } else {
        runtime.block_on(tokio::task::yield_now());
    }
    // Keep the producer alive across shutdown: an idle writer must not leave
    // an infinite blocking receive preventing Runtime::drop from returning.
    drop(release);
    drop(runtime);
    assert!(task.is_finished());
    assert!(request.writer.sender.is_closed());
    assert_eq!(request.writer.queued_bytes.load(Ordering::Relaxed), 0);
}

#[test]
fn process_shutdown_completes_with_idle_and_active_writers() {
    for mode in ["idle", "active"] {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "capture::writer::tests::runtime_shutdown_child",
                "--nocapture",
            ])
            .env("NAN_CAPTURE_SHUTDOWN_TEST", mode)
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + DEADLINE;
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "shutdown child failed: {mode}");
                break;
            }
            if std::time::Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("shutdown child timed out: {mode}");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
