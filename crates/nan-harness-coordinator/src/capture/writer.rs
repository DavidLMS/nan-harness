//! Optional capture storage runs in finite blocking batches, with a process-wide
//! concurrency limit. Idle receivers remain async; producers never wait for disk
//! drainage. Closing all producers drains accepted records, flushes, writes any
//! incomplete marker and releases the purge lock.
//!
//! Runtime cancellation can discard queued records and skip finalization, as can
//! process termination. An already running batch retains its resources until it
//! returns; it cannot cancel a stuck OS write. Tokio runtime shutdown may therefore
//! wait for active storage I/O, but never for an idle capture receiver. Markers on
//! abrupt shutdown are best-effort, not a guarantee of capture completeness.

use super::Record;
use nan_harness_private_fs::open_private_new;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};
use tokio::sync::{Semaphore, mpsc};

const BATCH_RECORDS: usize = 16;
const CONCURRENT_BATCHES: usize = 4;
static WRITER_SLOTS: LazyLock<Arc<Semaphore>> =
    LazyLock::new(|| Arc::new(Semaphore::new(CONCURRENT_BATCHES)));

struct Storage<W> {
    file: W,
    lock: File,
    receiver: mpsc::Receiver<Record>,
    incomplete: Arc<AtomicBool>,
    incomplete_path: PathBuf,
}

pub(super) async fn write_records(
    file: File,
    lock: File,
    receiver: mpsc::Receiver<Record>,
    incomplete: Arc<AtomicBool>,
    incomplete_path: PathBuf,
) {
    Storage {
        file,
        lock,
        receiver,
        incomplete,
        incomplete_path,
    }
    .run(Arc::clone(&WRITER_SLOTS))
    .await;
}

impl<W: Write + Send + 'static> Storage<W> {
    async fn run(mut self, slots: Arc<Semaphore>) {
        loop {
            // Idle writers own no blocking task or semaphore permit. Receive only
            // one record ahead, preserving the existing queue admission bound.
            let first = self.receiver.recv().await;
            let Ok(permit) = Arc::clone(&slots).acquire_owned().await else {
                return;
            };
            // The closure owns the file, lock, receiver and credits until its
            // finite batch completes, even if the async task is cancelled.
            let batch = tokio::task::spawn_blocking(move || {
                let _permit = permit;
                self.write_batch(first)
            });
            match batch.await {
                Ok(Some(storage)) => self = storage,
                Ok(None) | Err(_) => return,
            }
        }
    }

    fn write_batch(mut self, mut first: Option<Record>) -> Option<Self> {
        if first.is_none() {
            self.finish();
            return None;
        }
        for _ in 0..BATCH_RECORDS {
            let record = match first.take().map_or_else(|| self.receiver.try_recv(), Ok) {
                Ok(record) => record,
                Err(mpsc::error::TryRecvError::Empty) => return Some(self),
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    self.finish();
                    return None;
                }
            };
            let result = serde_json::to_writer(&mut self.file, &record)
                .map_err(std::io::Error::other)
                .and_then(|()| self.file.write_all(b"\n"));
            drop(record);
            if result.is_err() {
                self.incomplete.store(true, Ordering::Relaxed);
                self.finish();
                return None;
            }
        }
        Some(self)
    }

    fn finish(self) {
        let Self {
            mut file,
            lock,
            receiver,
            incomplete,
            incomplete_path,
        } = self;
        // Close admission and release queued credits before final storage I/O.
        drop(receiver);
        if file.flush().is_err() {
            incomplete.store(true, Ordering::Relaxed);
        }
        if incomplete.load(Ordering::Relaxed)
            && let Ok(mut marker) = open_private_new(&incomplete_path)
        {
            let _ = marker.write_all(b"capture incomplete\n");
        }
        drop(file);
        drop(lock);
    }
}

#[cfg(test)]
mod tests;
