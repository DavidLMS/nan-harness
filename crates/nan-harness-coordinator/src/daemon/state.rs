use crate::CoordinatorError;
use crate::protocol::Receipt;
use nan_harness_private_fs::{open_private_new, open_private_read, open_private_truncate};
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::Write as _;
use std::path::Path;
use subtle::ConstantTimeEq as _;

pub(super) fn acquire_process_lock(directory: &Path) -> Result<File, CoordinatorError> {
    let path = directory.join("process.lock");
    let file = open_private_truncate(&path).map_err(|source| state_error(&path, source))?;
    file.try_lock()
        .map_err(|_| CoordinatorError::Protocol("another coordinator is already running"))?;
    Ok(file)
}

pub(super) fn write_receipt(directory: &Path, receipt: &Receipt) -> Result<(), CoordinatorError> {
    let temporary = directory.join(format!("receipt-{}.tmp", receipt.generation));
    let target = directory.join("receipt.json");
    let payload = serde_json::to_vec(receipt)?;
    let mut file =
        open_private_new(&temporary).map_err(|source| state_error(&temporary, source))?;
    file.write_all(&payload)
        .and_then(|()| file.sync_all())
        .map_err(|source| state_error(&temporary, source))?;
    if target.exists() {
        fs::remove_file(&target).map_err(|source| state_error(&target, source))?;
    }
    fs::rename(&temporary, &target).map_err(|source| state_error(&target, source))
}

pub(super) fn remove_own_receipt(directory: &Path, generation: &str) {
    let path = directory.join("receipt.json");
    let matches = open_private_read(&path)
        .ok()
        .and_then(|(file, _)| serde_json::from_reader::<_, Receipt>(file).ok())
        .is_some_and(|receipt| receipt.generation == generation);
    if matches {
        let _ = fs::remove_file(path);
    }
}

pub(super) fn random_hex() -> Result<String, CoordinatorError> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes)?;
    let mut value = String::with_capacity(64);
    for byte in bytes {
        let _ = write!(&mut value, "{byte:02x}");
    }
    Ok(value)
}

pub(super) fn tokens_match(expected: &str, supplied: &str) -> bool {
    expected.len() == supplied.len() && bool::from(expected.as_bytes().ct_eq(supplied.as_bytes()))
}

pub(super) fn state_error(path: &Path, source: std::io::Error) -> CoordinatorError {
    CoordinatorError::State {
        path: path.to_path_buf(),
        source,
    }
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
