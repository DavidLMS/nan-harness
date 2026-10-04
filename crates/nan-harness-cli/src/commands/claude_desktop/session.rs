#[allow(clippy::wildcard_imports)]
use super::*;

pub(super) fn prepare_session_lock(
    paths: &DesktopPaths,
    process: &impl DesktopProcess,
) -> Result<SessionLock, ClaudeDesktopError> {
    process.ensure_available()?;
    SessionLock::acquire(&paths.lock)
}

// Keep launch exclusivity through policy checks and the entire managed session.
pub(super) fn prepare_managed_session(
    paths: &DesktopPaths,
    process: &impl DesktopProcess,
) -> Result<SessionLock, ClaudeDesktopError> {
    let lock = observe_prelaunch(
        prepare_session_lock(paths, process),
        PrelaunchStage::SessionLock,
    )?;
    observe_prelaunch(
        ensure_no_pending_recovery(paths),
        PrelaunchStage::PendingRecovery,
    )?;
    if observe_prelaunch(process.is_running(), PrelaunchStage::ProcessQuery)? {
        return observe_prelaunch(
            Err(ClaudeDesktopError::AlreadyRunning),
            PrelaunchStage::ProcessPresent,
        );
    }
    Ok(lock)
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
        let file = nan_harness_private_fs::open_private_read_write(path)
            .map_err(ClaudeDesktopError::Lock)?;
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
                #[cfg(unix)]
                let metadata = fs::metadata(path).map_err(ClaudeDesktopError::ReadConfig)?;
                #[cfg(not(unix))]
                let _ = fs::metadata(path).map_err(ClaudeDesktopError::ReadConfig)?;
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
    atomic_write_inner(path, payload, permissions, private, None)
}

pub(super) fn atomic_write_configuration(
    path: &Path,
    payload: &[u8],
    permissions: Option<&Permissions>,
    document: qualification_prelaunch::ConfigurationDocument,
) -> Result<(), ClaudeDesktopError> {
    atomic_write_inner(path, payload, permissions, false, Some(document))
}

fn atomic_write_inner(
    path: &Path,
    payload: &[u8],
    permissions: Option<&Permissions>,
    private: bool,
    document: Option<qualification_prelaunch::ConfigurationDocument>,
) -> Result<(), ClaudeDesktopError> {
    use qualification_prelaunch::ConfigurationSubstage as Substage;
    fn observe<T>(
        result: Result<T, ClaudeDesktopError>,
        substage: Substage,
        document: Option<qualification_prelaunch::ConfigurationDocument>,
    ) -> Result<T, ClaudeDesktopError> {
        if let Some(document) = document {
            qualification_prelaunch::observe_configuration(result, substage, Some(document))
        } else {
            result
        }
    }
    let parent = observe(
        path.parent().ok_or(ClaudeDesktopError::InvalidStatePath),
        Substage::ParentCreate,
        document,
    )?;
    if private {
        observe(
            nan_harness_private_fs::create_private_dir_all(parent)
                .map_err(ClaudeDesktopError::CreateDirectory),
            Substage::ParentCreate,
            document,
        )?;
    } else {
        observe(
            fs::create_dir_all(parent).map_err(ClaudeDesktopError::CreateDirectory),
            Substage::ParentCreate,
            document,
        )?;
    }
    observe(reject_symlink(path), Substage::PathCheck, document)?;
    let mut temporary = observe(
        TempFileBuilder::new()
            .prefix(".nan-")
            .make_in(parent, open_private_new)
            .map_err(ClaudeDesktopError::Write),
        Substage::TemporaryCreate,
        document,
    )?;
    observe(
        temporary
            .write_all(payload)
            .and_then(|()| temporary.flush())
            .and_then(|()| temporary.as_file().sync_all())
            .map_err(ClaudeDesktopError::Write),
        Substage::TemporaryWrite,
        document,
    )?;
    if let Some(permissions) = permissions {
        observe(
            temporary
                .as_file()
                .set_permissions(permissions.clone())
                .map_err(ClaudeDesktopError::Permissions),
            Substage::TemporaryPermissions,
            document,
        )?;
    }
    qualification_prelaunch::observe_configuration_persist(
        persist_configuration_file(temporary, path, document.is_some())
            .map_err(|error| ClaudeDesktopError::Write(error.error)),
        document,
    )?;
    Ok(())
}

// Windows may temporarily deny replacement while another handle lacks delete
// sharing. Retry only that exact error, retaining this already-written file.
fn persist_configuration_file(
    temporary: tempfile::NamedTempFile,
    path: &Path,
    configuration: bool,
) -> Result<File, tempfile::PersistError> {
    #[cfg(windows)]
    if configuration {
        return persist_windows_configuration(temporary, path);
    }
    #[cfg(not(windows))]
    let _ = configuration;
    temporary.persist(path)
}

#[cfg(windows)]
fn persist_windows_configuration(
    mut temporary: tempfile::NamedTempFile,
    path: &Path,
) -> Result<File, tempfile::PersistError> {
    use std::time::{Duration, Instant};
    let deadline = Instant::now() + Duration::from_millis(250);
    loop {
        match temporary.persist(path) {
            Ok(file) => return Ok(file),
            Err(error) => {
                if error.error.raw_os_error() != Some(32) || Instant::now() >= deadline {
                    return Err(error);
                }
                std::thread::sleep(
                    Duration::from_millis(5)
                        .min(deadline.saturating_duration_since(Instant::now())),
                );
                if Instant::now() >= deadline {
                    return Err(error);
                }
                temporary = error.file;
            }
        }
    }
}

pub(super) fn reject_symlink(path: &Path) -> Result<(), ClaudeDesktopError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(ClaudeDesktopError::UnsafeSymlink),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(ClaudeDesktopError::ReadConfig(error)),
    }
}

#[cfg(all(test, windows))]
mod configuration_persist_tests {
    use super::*;
    use std::os::windows::fs::OpenOptionsExt as _;
    #[test]
    fn configuration_persist_with_retained_read_only_parent_and_canonical_paths() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let parent = root
            .join("profile")
            .join("home")
            .join("AppData")
            .join("Roaming")
            .join("Claude");
        nan_harness_private_fs::create_private_dir_all(&parent).unwrap();
        let local = root
            .join("profile")
            .join("home")
            .join("AppData")
            .join("Local");
        let third_party = local.join("Claude-3p");
        nan_harness_private_fs::create_private_dir_all(&third_party).unwrap();
        let roaming = parent.parent().unwrap();
        let mut retained = Vec::new();
        // Match FreshClaudeWindowsProfile::prepare, including volume root.
        for directory in local
            .ancestors()
            .chain([roaming, parent.as_path(), third_party.as_path()])
        {
            retained.push(
                std::fs::OpenOptions::new()
                    .read(true)
                    .share_mode(1)
                    .custom_flags(0x0200_0000 | 0x0020_0000)
                    .open(directory)
                    .unwrap(),
            );
        }
        let path = parent.join("claude_desktop_config.json");
        let result = atomic_write_configuration(
            &path,
            b"{\"deploymentMode\":\"3p\"}\n",
            None,
            qualification_prelaunch::ConfigurationDocument::NormalConfig,
        );
        if let Err(ClaudeDesktopError::Write(error)) = &result {
            panic!(
                "closed persistence category: {:?}",
                qualification_prelaunch::ConfigurationIoFailure::from_error(error)
            );
        }
        assert!(result.is_ok(), "non-write configuration failure");
        assert_eq!(fs::read(&path).unwrap(), b"{\"deploymentMode\":\"3p\"}\n");
        drop(retained);
    }
    #[test]
    fn configuration_persist_retries_same_file_after_target_handle_release() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");
        fs::write(&path, b"original").unwrap();
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&path)
            .unwrap();
        let mut temporary = tempfile::Builder::new()
            .prefix(".nan-")
            .make_in(temp.path(), nan_harness_private_fs::open_private_new)
            .unwrap();
        temporary.write_all(b"replacement").unwrap();
        temporary.flush().unwrap();
        let initial = temporary.persist(&path).unwrap_err();
        assert_eq!(initial.error.raw_os_error(), Some(32));
        assert_eq!(fs::read(&path).unwrap(), b"original");
        let release = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(40));
            drop(held);
        });
        let result = persist_windows_configuration(initial.file, &path);
        release.join().unwrap();
        assert!(result.is_ok());
        assert_eq!(fs::read(path).unwrap(), b"replacement");
    }

    #[test]
    fn configuration_persist_permanent_sharing_failure_preserves_original() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");
        fs::write(&path, b"original").unwrap();
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&path)
            .unwrap();
        let started = std::time::Instant::now();
        let result = atomic_write_configuration(
            &path,
            b"replacement",
            None,
            qualification_prelaunch::ConfigurationDocument::NormalConfig,
        );
        assert!(
            matches!(result, Err(ClaudeDesktopError::Write(error)) if error.raw_os_error()==Some(32))
        );
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        assert_eq!(fs::read(&path).unwrap(), b"original");
        drop(held);
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
    }

    #[test]
    fn configuration_persist_unrelated_failure_preserves_destination() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("directory");
        fs::create_dir(&destination).unwrap();
        let original = fs::read_dir(temp.path()).unwrap().count();
        assert!(
            atomic_write_configuration(
                &destination,
                b"replacement",
                None,
                qualification_prelaunch::ConfigurationDocument::NormalConfig
            )
            .is_err()
        );
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), original);
        assert!(destination.is_dir());
    }
    #[test]
    fn ordinary_persist_does_not_retry_sharing_failure() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");
        fs::write(&path, b"original").unwrap();
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&path)
            .unwrap();
        let mut temporary = tempfile::NamedTempFile::new_in(temp.path()).unwrap();
        temporary.write_all(b"replacement").unwrap();
        let error = persist_configuration_file(temporary, &path, false).unwrap_err();
        assert_eq!(error.error.raw_os_error(), Some(32));
        assert_eq!(fs::read(&path).unwrap(), b"original");
        drop(error);
        drop(held);
    }
}
