#[allow(clippy::wildcard_imports)]
use super::*;

pub(super) fn prepare_session_lock(
    paths: &DesktopPaths,
    process: &impl DesktopProcess,
) -> Result<SessionLock, ClaudeDesktopError> {
    process.ensure_available()?;
    SessionLock::acquire(&paths.lock)
}

pub(super) fn ensure_no_pending_recovery(paths: &DesktopPaths) -> Result<(), ClaudeDesktopError> {
    reject_symlink(&paths.receipt)?;
    reject_symlink(&paths.backup_directory)?;
    if paths.receipt.exists() {
        return Err(ClaudeDesktopError::OrphanReceipt);
    }
    if paths.backup_directory.exists() {
        return Err(ClaudeDesktopError::OrphanBackup);
    }
    Ok(())
}

pub(super) struct SessionLock {
    file: File,
}

impl SessionLock {
    pub(super) fn acquire(path: &Path) -> Result<Self, ClaudeDesktopError> {
        let parent = path.parent().ok_or(ClaudeDesktopError::InvalidStatePath)?;
        nan_harness_private_fs::create_private_dir_all(parent)
            .map_err(ClaudeDesktopError::CreateDirectory)?;
        reject_symlink(path)?;
        let mut file = match open_private_new(path) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)
                .map_err(ClaudeDesktopError::Lock)?,
            Err(error) => return Err(ClaudeDesktopError::Lock(error)),
        };
        nan_harness_private_fs::restrict_file(&mut file)
            .map_err(ClaudeDesktopError::Permissions)?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                return Err(ClaudeDesktopError::ConcurrentSession);
            }
            Err(TryLockError::Error(error)) => {
                return Err(ClaudeDesktopError::Lock(error));
            }
        }
        Ok(Self { file })
    }
}

impl Drop for SessionLock {
    fn drop(&mut self) {
        let _ = File::unlock(&self.file);
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Receipt {
    schema: u8,
    snapshots: Vec<Snapshot>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Snapshot {
    document_id: String,
    existed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    backup_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sha256: Option<String>,
    #[cfg(unix)]
    mode: Option<u32>,
}

impl Receipt {
    pub(super) fn capture(paths: &DesktopPaths) -> Result<Self, ClaudeDesktopError> {
        reject_symlink(&paths.backup_directory)?;
        if paths.backup_directory.exists() {
            return Err(ClaudeDesktopError::OrphanBackup);
        }
        let state_directory = paths
            .backup_directory
            .parent()
            .ok_or(ClaudeDesktopError::InvalidStatePath)?;
        nan_harness_private_fs::create_private_dir_all(state_directory)
            .map_err(ClaudeDesktopError::CreateBackupDirectory)?;
        nan_harness_private_fs::create_private_dir(&paths.backup_directory)
            .map_err(ClaudeDesktopError::CreateBackupDirectory)?;
        let result = paths
            .documents()
            .into_iter()
            .zip(DOCUMENT_IDS)
            .enumerate()
            .map(|(index, (path, document_id))| {
                Snapshot::capture(path, document_id, index, &paths.backup_directory)
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|snapshots| Self {
                schema: RECEIPT_SCHEMA,
                snapshots,
            });
        if result.is_err() {
            let _ = fs::remove_dir_all(&paths.backup_directory);
        }
        result
    }

    pub(super) fn write(&self, path: &Path) -> Result<(), ClaudeDesktopError> {
        reject_symlink(path)?;
        let payload = serde_json::to_vec(self).map_err(ClaudeDesktopError::SerializeReceipt)?;
        atomic_write(path, &payload, None, true)
    }

    pub(super) fn read(path: &Path) -> Result<Self, ClaudeDesktopError> {
        reject_symlink(path)?;
        let payload = fs::read(path).map_err(ClaudeDesktopError::ReadReceipt)?;
        let receipt: Self =
            serde_json::from_slice(&payload).map_err(ClaudeDesktopError::ParseReceipt)?;
        if receipt.schema != RECEIPT_SCHEMA
            || receipt.snapshots.len() != DOCUMENT_IDS.len()
            || receipt
                .snapshots
                .iter()
                .zip(DOCUMENT_IDS)
                .any(|(snapshot, expected)| snapshot.document_id != expected)
        {
            return Err(ClaudeDesktopError::UnsupportedReceipt);
        }
        Ok(receipt)
    }

    pub(super) fn restore(&self, paths: &DesktopPaths) -> Result<(), ClaudeDesktopError> {
        for (snapshot, path) in self.snapshots.iter().zip(paths.documents()) {
            snapshot.restore(path, &paths.backup_directory)?;
        }
        Ok(())
    }

    pub(super) fn remove_backups(paths: &DesktopPaths) {
        let _ = fs::remove_dir_all(&paths.backup_directory);
    }
}

impl Snapshot {
    fn capture(
        path: &Path,
        document_id: &str,
        index: usize,
        backup_directory: &Path,
    ) -> Result<Self, ClaudeDesktopError> {
        reject_symlink(path)?;
        match fs::read(path) {
            Ok(contents) => {
                let metadata = fs::metadata(path).map_err(ClaudeDesktopError::ReadConfig)?;
                let backup_file = format!("document-{index}.backup");
                write_private_new(&backup_directory.join(&backup_file), &contents)?;
                #[cfg(unix)]
                let mode = {
                    use std::os::unix::fs::PermissionsExt as _;
                    Some(metadata.permissions().mode())
                };
                Ok(Self {
                    document_id: document_id.to_owned(),
                    existed: true,
                    backup_file: Some(backup_file),
                    sha256: Some(sha256(&contents)),
                    #[cfg(unix)]
                    mode,
                })
            }
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Self {
                document_id: document_id.to_owned(),
                existed: false,
                backup_file: None,
                sha256: None,
                #[cfg(unix)]
                mode: None,
            }),
            Err(error) => Err(ClaudeDesktopError::ReadConfig(error)),
        }
    }

    fn restore(&self, path: &Path, backup_directory: &Path) -> Result<(), ClaudeDesktopError> {
        reject_symlink(path)?;
        if !self.existed {
            if self.backup_file.is_some() || self.sha256.is_some() {
                return Err(ClaudeDesktopError::UnsupportedReceipt);
            }
            return match fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
                Err(error) => Err(ClaudeDesktopError::Restore(error)),
            };
        }
        let backup_file = self
            .backup_file
            .as_deref()
            .ok_or(ClaudeDesktopError::UnsupportedReceipt)?;
        if Path::new(backup_file)
            .file_name()
            .and_then(|name| name.to_str())
            != Some(backup_file)
        {
            return Err(ClaudeDesktopError::UnsupportedReceipt);
        }
        let backup_path = backup_directory.join(backup_file);
        reject_symlink(&backup_path)?;
        let contents = fs::read(backup_path).map_err(ClaudeDesktopError::ReadBackup)?;
        let actual_sha256 = sha256(&contents);
        if self.sha256.as_deref() != Some(actual_sha256.as_str()) {
            return Err(ClaudeDesktopError::BackupHashMismatch);
        }
        #[cfg(unix)]
        let permissions = self.mode.map(|mode| {
            use std::os::unix::fs::PermissionsExt as _;
            Permissions::from_mode(mode)
        });
        #[cfg(not(unix))]
        let permissions = None;
        atomic_write(path, &contents, permissions.as_ref(), false)
    }
}

pub(super) fn restore_receipt(paths: &DesktopPaths) -> Result<(), ClaudeDesktopError> {
    reject_symlink(&paths.receipt)?;
    reject_symlink(&paths.backup_directory)?;
    if !paths.receipt.exists() {
        return if paths.backup_directory.exists() {
            Err(ClaudeDesktopError::OrphanBackup)
        } else {
            Err(ClaudeDesktopError::NoReceipt)
        };
    }
    let receipt = Receipt::read(&paths.receipt)?;
    receipt.restore(paths)?;
    fs::remove_file(&paths.receipt).map_err(ClaudeDesktopError::RemoveReceipt)?;
    fs::remove_dir_all(&paths.backup_directory).map_err(ClaudeDesktopError::RemoveBackup)
}

pub(super) fn sha256(payload: &[u8]) -> String {
    let digest = Sha256::digest(payload);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut encoded, "{byte:02x}");
    }
    encoded
}

fn write_private_new(path: &Path, payload: &[u8]) -> Result<(), ClaudeDesktopError> {
    let mut file = open_private_new(path).map_err(ClaudeDesktopError::WriteBackup)?;
    file.write_all(payload)
        .and_then(|()| file.flush())
        .and_then(|()| file.sync_all())
        .map_err(ClaudeDesktopError::WriteBackup)
}

pub(super) fn atomic_write(
    path: &Path,
    payload: &[u8],
    permissions: Option<&Permissions>,
    private: bool,
) -> Result<(), ClaudeDesktopError> {
    let parent = path.parent().ok_or(ClaudeDesktopError::InvalidStatePath)?;
    if private {
        nan_harness_private_fs::create_private_dir_all(parent)
            .map_err(ClaudeDesktopError::CreateDirectory)?;
    } else {
        fs::create_dir_all(parent).map_err(ClaudeDesktopError::CreateDirectory)?;
    }
    reject_symlink(path)?;
    let mut temporary = TempFileBuilder::new()
        .prefix(".nan-")
        .make_in(parent, open_private_new)
        .map_err(ClaudeDesktopError::Write)?;
    temporary
        .write_all(payload)
        .and_then(|()| temporary.flush())
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(ClaudeDesktopError::Write)?;
    if let Some(permissions) = permissions {
        temporary
            .as_file()
            .set_permissions(permissions.clone())
            .map_err(ClaudeDesktopError::Permissions)?;
    }
    temporary
        .persist(path)
        .map_err(|error| ClaudeDesktopError::Write(error.error))?;
    Ok(())
}

pub(super) fn reject_symlink(path: &Path) -> Result<(), ClaudeDesktopError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(ClaudeDesktopError::UnsafeSymlink),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(ClaudeDesktopError::ReadConfig(error)),
    }
}
