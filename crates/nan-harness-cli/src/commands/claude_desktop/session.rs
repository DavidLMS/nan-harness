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
    persist_written_configuration(temporary, path, document)
}

// The written file is durable and has its final private permissions here;
// persistence and failure diagnostics share ownership of the same temporary.
fn persist_written_configuration(
    temporary: tempfile::NamedTempFile,
    path: &Path,
    document: Option<qualification_prelaunch::ConfigurationDocument>,
) -> Result<(), ClaudeDesktopError> {
    #[cfg(all(windows, feature = "desktop-qualification"))]
    if matches!(
        document,
        Some(qualification_prelaunch::ConfigurationDocument::NormalConfig)
    ) && std_rename_policy_requested()?
    {
        let mut boundary = qualification_prelaunch::StdRenameBoundary::OriginalSource;
        let result = rename_written_configuration_once(
            temporary,
            path,
            qualification_persist_owners::configuration_retry_deadline(),
            &mut boundary,
        )
        .inspect_err(|error| {
            if error.error.raw_os_error() == Some(32) {
                qualification_persist_owners::observe_retained_path(&error.path, path);
            }
        })
        .map_err(|error| ClaudeDesktopError::Write(error.error));
        return qualification_prelaunch::observe_configuration_rename(result, Some(boundary));
    }
    #[cfg(all(windows, feature = "desktop-qualification"))]
    if matches!(
        document,
        Some(qualification_prelaunch::ConfigurationDocument::NormalConfig)
    ) && qualification_prelaunch::enabled()
    {
        // Trial-only variation: close the fully written and synced file handle,
        // retaining tempfile's original RAII path and atomic replacement contract.
        let result = persist_closed_configuration_until(
            temporary.into_temp_path(),
            path,
            qualification_persist_owners::configuration_retry_deadline(),
        )
        .inspect_err(|error| {
            if error.error.raw_os_error() == Some(32) {
                qualification_persist_owners::observe_retained_path(&error.path, path);
            }
        })
        .map_err(|error| ClaudeDesktopError::Write(error.error));
        return qualification_prelaunch::observe_configuration_rename(result, None);
    }
    #[cfg(all(windows, feature = "desktop-qualification"))]
    let attributes_before = {
        use std::os::windows::fs::MetadataExt as _;
        (qualification_prelaunch::enabled()
            && matches!(
                document,
                Some(qualification_prelaunch::ConfigurationDocument::NormalConfig)
            ))
        .then(|| {
            temporary
                .as_file()
                .metadata()
                .ok()
                .map(|metadata| metadata.file_attributes())
        })
        .flatten()
    };
    let result = persist_configuration_file(temporary, path, document.is_some());
    #[cfg(all(windows, feature = "desktop-qualification"))]
    let attributes = {
        use std::os::windows::fs::MetadataExt as _;
        let after = result
            .as_ref()
            .err()
            .filter(|error| attributes_before.is_some() && error.error.raw_os_error() == Some(32))
            .and_then(|error| error.file.as_file().metadata().ok())
            .map(|metadata| metadata.file_attributes());
        qualification_prelaunch::persist_attributes(attributes_before, after)
    };
    #[cfg(all(windows, feature = "desktop-qualification"))]
    let result = result.inspect_err(|error| {
        if error.error.raw_os_error() == Some(32)
            && matches!(
                document,
                Some(qualification_prelaunch::ConfigurationDocument::NormalConfig)
            )
        {
            qualification_persist_owners::observe(&error.file, path);
        }
    });
    let result = result.map_err(|error| ClaudeDesktopError::Write(error.error));
    #[cfg(all(windows, feature = "desktop-qualification"))]
    let result = qualification_prelaunch::observe_configuration_persist_attributes(
        result, document, attributes,
    );
    #[cfg(not(all(windows, feature = "desktop-qualification")))]
    let result = qualification_prelaunch::observe_configuration_persist(result, document);
    result?;
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

#[cfg(all(windows, feature = "desktop-qualification"))]
fn std_rename_policy_requested() -> Result<bool, ClaudeDesktopError> {
    let policy = std::env::var("NANH_CLAUDE_WINDOWS_PERSIST_POLICY")
        .map(Some)
        .or_else(|error| match error {
            std::env::VarError::NotPresent => Ok(None),
            std::env::VarError::NotUnicode(_) => Err(ClaudeDesktopError::InvalidStatePath),
        })?;
    let scope = qualification_prelaunch::enabled()
        && std::env::var("NANH_CLAUDE_WINDOWS_CHAT_ONLY").as_deref() == Ok("1")
        && std::env::var("NANH_CLAUDE_WINDOWS_SOURCE_POLICY").as_deref()
            == Ok("official-2.19675.0-97910a066871");
    std_rename_requested(policy.as_deref(), scope)
}

#[cfg(all(windows, any(feature = "desktop-qualification", test)))]
fn std_rename_requested(
    value: Option<&str>,
    owned_scope: bool,
) -> Result<bool, ClaudeDesktopError> {
    match value {
        None => Ok(false),
        Some("std-rename") if owned_scope => Ok(true),
        _ => Err(ClaudeDesktopError::InvalidStatePath),
    }
}

#[cfg(all(windows, any(feature = "desktop-qualification", test)))]
fn retained_rename_source(path: &Path, share: u32) -> std::io::Result<same_file::Handle> {
    use std::os::windows::fs::OpenOptionsExt as _;
    let file = fs::OpenOptions::new()
        .read(true)
        .access_mode(0x8002_0000)
        .share_mode(share)
        .custom_flags(0x0020_0000)
        .open(path)?;
    nan_harness_private_fs::verify_private_file(&file)?;
    same_file::Handle::from_file(file)
}

#[cfg(all(windows, any(feature = "desktop-qualification", test)))]
fn rename_written_configuration_once(
    temporary: tempfile::NamedTempFile,
    path: &Path,
    deadline: std::time::Instant,
    boundary: &mut qualification_prelaunch::StdRenameBoundary,
) -> Result<(), tempfile::PathPersistError> {
    use qualification_prelaunch::StdRenameBoundary as Boundary;
    use std::os::windows::fs::MetadataExt as _;
    use std::time::Instant;
    let (written, mut temporary) = temporary.into_parts();
    let result = (|| {
        let fail = |kind| std::io::Error::from(kind);
        if Instant::now() >= deadline {
            *boundary = Boundary::Deadline;
            return Err(fail(ErrorKind::TimedOut));
        }
        *boundary = Boundary::OriginalSource;
        let original = same_file::Handle::from_file(written)?;
        let metadata = original.as_file().metadata()?;
        if !metadata.is_file() || metadata.file_attributes() & (0x1 | 0x100 | 0x400) != 0 {
            return Err(fail(ErrorKind::InvalidInput));
        }
        nan_harness_private_fs::verify_private_file(original.as_file())?;
        // This initial reader shares WRITE while the actual written handle exists.
        // It retains its proven file identity across closing that written handle.
        *boundary = Boundary::BridgeReader;
        let bridge = retained_rename_source(&temporary, 7)?;
        if bridge != original {
            return Err(fail(ErrorKind::InvalidInput));
        }
        drop(original);
        // The final read authority allows DELETE/rename, but denies content writes.
        *boundary = Boundary::RetainedReader;
        let held = retained_rename_source(&temporary, 5)?;
        if held != bridge {
            return Err(fail(ErrorKind::InvalidInput));
        }
        drop(bridge);
        *boundary = Boundary::DestinationPreflight;
        match fs::symlink_metadata(path) {
            Err(error) if error.kind() == ErrorKind::NotFound => (),
            Err(error) => return Err(error),
            Ok(_) => return Err(fail(ErrorKind::AlreadyExists)),
        }
        if Instant::now() >= deadline {
            *boundary = Boundary::Deadline;
            return Err(fail(ErrorKind::TimedOut));
        }
        // One safe Rust dispatch, no retry/fallback added by this caller. The std
        // Windows implementation itself may use its ACCESS_DENIED fallback.
        *boundary = Boundary::RenameDispatch;
        fs::rename(&temporary, path)?;
        *boundary = Boundary::DestinationIdentity;
        let destination = retained_rename_source(path, 5)?;
        if destination != held {
            return Err(fail(ErrorKind::InvalidInput));
        }
        *boundary = Boundary::PrivatePostcheck;
        let after = destination.as_file().metadata()?;
        if !after.is_file() || after.file_attributes() & (0x1 | 0x100 | 0x400) != 0 {
            return Err(fail(ErrorKind::InvalidInput));
        }
        if Instant::now() >= deadline {
            *boundary = Boundary::Deadline;
            return Err(fail(ErrorKind::TimedOut));
        }
        // keep() would call SetFileAttributes again on Windows. Disarm only after
        // exact source-object and read-only private-DACL postconditions succeed.
        temporary.disable_cleanup(true);
        Ok(())
    })();
    result.map_err(|error| tempfile::PathPersistError {
        error,
        path: temporary,
    })
}

#[cfg(all(windows, test))]
fn persist_closed_configuration(
    temporary: tempfile::TempPath,
    path: &Path,
) -> Result<(), tempfile::PathPersistError> {
    persist_closed_configuration_until(
        temporary,
        path,
        std::time::Instant::now() + Duration::from_millis(250),
    )
}

#[cfg(all(windows, any(feature = "desktop-qualification", test)))]
fn persist_closed_configuration_until(
    mut temporary: tempfile::TempPath,
    path: &Path,
    deadline: std::time::Instant,
) -> Result<(), tempfile::PathPersistError> {
    use std::time::{Duration, Instant};
    loop {
        match temporary.persist(path) {
            Ok(()) => return Ok(()),
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
                temporary = error.path;
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
    #[cfg(feature = "desktop-qualification")]
    #[derive(Clone, Copy, serde::Serialize)]
    enum PrimitiveIoError {
        #[serde(rename = "5")]
        AccessDenied,
        #[serde(rename = "32")]
        SharingViolation,
        #[serde(rename = "other")]
        Other,
    }
    #[cfg(feature = "desktop-qualification")]
    #[derive(Clone, Copy, Default, serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct PrimitiveStage {
        attempted: bool,
        succeeded: bool,
        raw_os_error: Option<PrimitiveIoError>,
    }
    #[cfg(feature = "desktop-qualification")]
    impl PrimitiveStage {
        fn from_result<T>(result: &std::io::Result<T>) -> Self {
            Self {
                attempted: true,
                succeeded: result.is_ok(),
                raw_os_error: result
                    .as_ref()
                    .err()
                    .map(|error| match error.raw_os_error() {
                        Some(5) => PrimitiveIoError::AccessDenied,
                        Some(32) => PrimitiveIoError::SharingViolation,
                        _ => PrimitiveIoError::Other,
                    }),
            }
        }
    }
    #[cfg(feature = "desktop-qualification")]
    #[derive(Default, serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct PrimitiveStages {
        setup: PrimitiveStage,
        source_attributes: PrimitiveStage,
        parent_delete_open: PrimitiveStage,
        source_delete_open: PrimitiveStage,
        absent_destination_std_rename: PrimitiveStage,
        target_delete_open: PrimitiveStage,
        target_blocked_std_rename: PrimitiveStage,
        original_preserved: bool,
        replacement_preserved: bool,
    }
    #[cfg(feature = "desktop-qualification")]
    impl PrimitiveStages {
        fn expected(&self, parent_share: Option<u32>) -> bool {
            if !self.setup.succeeded
                || !self.source_attributes.succeeded
                || !self.source_delete_open.succeeded
            {
                return false;
            }
            if parent_share.is_some_and(|share| share & 4 == 0)
                && !matches!(
                    self.parent_delete_open.raw_os_error,
                    Some(PrimitiveIoError::SharingViolation)
                )
            {
                return false;
            }
            if parent_share == Some(1) {
                return matches!(
                    self.absent_destination_std_rename.raw_os_error,
                    Some(PrimitiveIoError::SharingViolation)
                );
            }
            self.absent_destination_std_rename.succeeded
                && matches!(
                    self.target_delete_open.raw_os_error,
                    Some(PrimitiveIoError::SharingViolation)
                )
                && matches!(
                    self.target_blocked_std_rename.raw_os_error,
                    Some(PrimitiveIoError::AccessDenied | PrimitiveIoError::SharingViolation)
                )
                && self.original_preserved
                && self.replacement_preserved
        }
    }

    #[cfg(feature = "desktop-qualification")]
    // Query DELETE sharing without changing data, attributes, DACL, or pathname.
    fn primitive_delete_open(path: &Path) -> std::io::Result<File> {
        fs::OpenOptions::new()
            .access_mode(0x0001_0000)
            .share_mode(7)
            .custom_flags(0x0200_0000 | 0x0020_0000)
            .open(path)
    }
    #[cfg(feature = "desktop-qualification")]
    fn primitive_prelaunch_roots(workspace: &Path, share: u32) -> std::io::Result<Vec<File>> {
        let local = workspace.join("profile/home/AppData/Local");
        let roaming = workspace.join("profile/home/AppData/Roaming");
        nan_harness_private_fs::create_private_dir_all(&local)?;
        nan_harness_private_fs::create_private_dir_all(&roaming)?;
        let lease = |path: &Path| {
            fs::OpenOptions::new()
                .read(true)
                .share_mode(share)
                .custom_flags(0x0200_0000 | 0x0020_0000)
                .open(path)
        };
        let mut held = local
            .ancestors()
            .map(lease)
            .collect::<std::io::Result<Vec<_>>>()?;
        held.push(lease(&roaming)?);
        for root in [roaming.join("Claude"), local.join("Claude-3p")] {
            nan_harness_private_fs::create_private_dir(&root)?;
            held.push(lease(&root)?);
        }
        Ok(held)
    }
    #[cfg(feature = "desktop-qualification")]
    fn primitive_stages(
        workspace: &Path,
        parent_share: Option<u32>,
        close_writer: bool,
        retain_source: bool,
    ) -> PrimitiveStages {
        let mut record = PrimitiveStages::default();
        let result = (|| -> std::io::Result<()> {
            let _parents = if let Some(share) = parent_share {
                primitive_prelaunch_roots(workspace, share)?
            } else {
                Vec::new()
            };
            let directory = if parent_share.is_some() {
                workspace.join("profile/home/AppData/Roaming/Claude")
            } else {
                workspace.to_owned()
            };
            if parent_share.is_some() {
                let probe = primitive_delete_open(&directory);
                record.parent_delete_open = PrimitiveStage::from_result(&probe);
                drop(probe);
            }
            let mut source = tempfile::Builder::new().make_in(&directory, open_private_new)?;
            source.write_all(b"original")?;
            source.as_file().sync_all()?;
            let (written, temporary) = source.into_parts();
            let written = if close_writer {
                drop(written);
                None
            } else {
                Some(written)
            };
            let _reader = if retain_source {
                Some(retained_rename_source(&temporary, 5)?)
            } else {
                None
            };
            record.setup = PrimitiveStage::from_result(&Ok(()));
            // tempfile::keep uses exactly the same SetFileAttributesW(NORMAL)
            // primitive as tempfile::persist, isolated before rename dispatch.
            let source = temporary.keep().map_err(|error| error.error);
            record.source_attributes = PrimitiveStage::from_result(&source);
            let Ok(source) = source else {
                return Ok(());
            };
            let probe = primitive_delete_open(&source);
            record.source_delete_open = PrimitiveStage::from_result(&probe);
            drop(probe);
            let destination = directory.join("primitive-destination.json");
            let rename = fs::rename(&source, &destination);
            record.absent_destination_std_rename = PrimitiveStage::from_result(&rename);
            if rename.is_err() {
                return Ok(());
            }
            // Target sharing is isolated from the source's earlier writer state.
            drop(written);
            let target = fs::OpenOptions::new()
                .read(true)
                .share_mode(1)
                .open(&destination)?;
            let probe = primitive_delete_open(&destination);
            record.target_delete_open = PrimitiveStage::from_result(&probe);
            drop(probe);
            let mut replacement = tempfile::Builder::new().make_in(&directory, open_private_new)?;
            replacement.write_all(b"replacement")?;
            replacement.as_file().sync_all()?;
            let (replacement_file, replacement_path) = replacement.into_parts();
            let replacement_file = if close_writer {
                drop(replacement_file);
                None
            } else {
                Some(replacement_file)
            };
            let rename = fs::rename(&replacement_path, &destination);
            record.target_blocked_std_rename = PrimitiveStage::from_result(&rename);
            record.original_preserved = fs::read(&destination)? == b"original";
            record.replacement_preserved = fs::read(&replacement_path)? == b"replacement";
            drop(target);
            drop(replacement_file);
            Ok(())
        })();
        if let Err(error) = result {
            record.setup = PrimitiveStage::from_result(&Err::<(), _>(error));
        }
        record
    }
    #[cfg(feature = "desktop-qualification")]
    #[test]
    fn configuration_persist_primitive_stage_diagnostics() {
        let mut records = Vec::new();
        let mut passed = true;
        for (kind, parent_share, closed, retained) in [
            ("ordinary-open", None, false, false),
            ("ordinary-closed", None, true, false),
            ("qualification-closed", Some(1), true, false),
            ("qualification-retained-reader", Some(1), true, true),
            ("qualification-write-shared", Some(3), true, true),
            ("qualification-delete-shared", Some(7), true, true),
        ] {
            let outcome = (|| -> std::io::Result<PrimitiveStages> {
                let temporary = tempfile::tempdir()?;
                let workspace = temporary.path().canonicalize()?;
                let record = primitive_stages(&workspace, parent_share, closed, retained);
                Ok(record)
            })();
            let record = outcome.unwrap_or_else(|error| PrimitiveStages {
                setup: PrimitiveStage::from_result(&Err::<(), _>(error)),
                ..PrimitiveStages::default()
            });
            passed &= record.expected(parent_share);
            records.push(serde_json::json!({"kind":kind,"stages":record}));
        }
        println!(
            "{}",
            serde_json::json!({"schemaVersion":1,"mechanism":"windows-preinstall-persist-primitives",
            "diagnosticsOnly":true,"records":records})
        );
        assert!(passed, "preinstall persistence primitive contract failed");
    }

    // This child executes only configuration/session bookkeeping with placeholders.
    // No DesktopProcess, bridge listener, vendor executable or native query is used.
    #[cfg(feature = "desktop-qualification")]
    #[test]
    fn configuration_lifecycle_child_worker() {
        if std::env::var("NANH_CONFIGURATION_LIFECYCLE_WORKER").as_deref() != Ok("1") {
            return;
        }
        let mut stage = 0_u8;
        let result = (|| {
            // Match the canonical workspace spelling injected by the parent.
            stage = 0;
            let workspace = std::env::current_dir()
                .and_then(|directory| directory.canonicalize())
                .map_err(ClaudeDesktopError::ReadConfig)?;
            let profile = workspace.join("profile");
            let expected = DesktopPaths::new(
                &profile.join("home/AppData/Roaming/Claude"),
                &profile.join("home/AppData/Local/Claude-3p"),
                &profile.join("nanh"),
            );
            // Exercise production environment resolution, not a fixture-only path constructor.
            stage = 1;
            let paths = DesktopPaths::from_environment(DesktopPlatform::Windows)?;
            stage = 2;
            if paths.documents() != expected.documents()
                || paths.receipt != expected.receipt
                || paths.backup_directory != expected.backup_directory
                || paths.lock != expected.lock
            {
                return Err(ClaudeDesktopError::InvalidStatePath);
            }
            stage = 3;
            if !qualification_prelaunch::enabled()
                || qualification_config::observation_directory(&paths).is_none()
            {
                return Err(ClaudeDesktopError::InvalidStatePath);
            }
            stage = 4;
            if std::env::var("NANH_CONFIGURATION_STD_RENAME_WORKER").as_deref() == Ok("1")
                && !std_rename_policy_requested()?
            {
                return Err(ClaudeDesktopError::InvalidStatePath);
            }
            stage = 5;
            let _lock = SessionLock::acquire(&paths.lock)?;
            stage = 6;
            ensure_no_pending_recovery(&paths)?;
            stage = 7;
            let receipt = Receipt::capture(&paths)?;
            stage = 8;
            receipt.write(&paths.receipt)?;
            stage = 9;
            apply_gateway(&paths, "http://127.0.0.1:9", "synthetic-token")?;
            stage = 10;
            let normal = read_json_object(paths.documents()[0])?;
            let managed = read_json_object(paths.documents()[3])?;
            if normal.get("deploymentMode").and_then(Value::as_str) != Some("3p")
                || managed.get("coworkTabEnabled").and_then(Value::as_bool) != Some(false)
            {
                return Err(ClaudeDesktopError::InvalidStatePath);
            }
            stage = 11;
            for document in paths.documents() {
                nan_harness_private_fs::open_private_read(document)
                    .map_err(ClaudeDesktopError::ReadConfig)?;
            }
            stage = 12;
            restore_receipt(&paths)?;
            stage = 13;
            if paths.documents().into_iter().any(Path::exists) {
                return Err(ClaudeDesktopError::InvalidStatePath);
            }
            Ok(())
        })();
        // All child output is suppressed; only fixed exit categories cross process.
        std::process::exit(match result {
            Ok(()) => 0,
            Err(ClaudeDesktopError::Write(error)) if error.raw_os_error() == Some(5) => 5,
            Err(ClaudeDesktopError::Write(error)) if error.raw_os_error() == Some(32) => 32,
            Err(error) => {
                let category = match error {
                    ClaudeDesktopError::InvalidStatePath => 1,
                    ClaudeDesktopError::ReadConfig(_) => 2,
                    ClaudeDesktopError::Lock(_) | ClaudeDesktopError::ConcurrentSession => 3,
                    ClaudeDesktopError::Permissions(_) | ClaudeDesktopError::CreateDirectory(_) => {
                        4
                    }
                    ClaudeDesktopError::Write(_) | ClaudeDesktopError::Restore(_) => 5,
                    ClaudeDesktopError::ParseConfig(_) | ClaudeDesktopError::ConfigRoot => 6,
                    _ => 7,
                };
                64 + i32::from(stage) * 8 + category
            }
        });
    }

    #[cfg(feature = "desktop-qualification")]
    fn lifecycle_child(workspace: &Path, source_policy: Option<&str>) -> Option<i32> {
        use std::process::{Command, Stdio};
        use std::time::{Duration, Instant};
        let home = workspace.join("profile/home");
        let mut command = Command::new(std::env::current_exe().ok()?);
        command.args(["--exact", "commands::claude_desktop::session::configuration_persist_tests::configuration_lifecycle_child_worker"])
            .current_dir(workspace)
            .env("NANH_CONFIGURATION_LIFECYCLE_WORKER", "1")
            .env("GITHUB_ACTIONS", "true")
            .env("RUNNER_ENVIRONMENT", "github-hosted")
            .env("RUNNER_OS", "Windows")
            .env("NANH_DESKTOP_QUALIFICATION_MODE", "startup-baseline")
            .env("NANH_CLAUDE_WINDOWS_PROFILE_POLICY", "private-env")
            .env("NANH_CLAUDE_WINDOWS_CHAT_ONLY", "1")
            .env("NANH_CLAUDE_PRELAUNCH_DIAGNOSTICS", "1")
            .env("NANH_DESKTOP_QUALIFICATION_FACTS", workspace.join("facts"))
            .env("HOME", &home).env("USERPROFILE", &home)
            .env("LOCALAPPDATA", home.join("AppData/Local"))
            .env("APPDATA", home.join("AppData/Roaming"))
            .env("NAN_HARNESS_CONFIG_DIR", workspace.join("profile/nanh"))
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        for key in [
            "CLAUDE_USER_DATA_DIR",
            "CLAUDE_CDP_AUTH",
            "NANH_CLAUDE_MAC_CHAT_NAVIGATION",
            "NANH_CLAUDE_LINUX_CHAT_ONLY",
            "NANH_CLAUDE_MCP_FIXTURE",
            "NANH_CLAUDE_LINUX_MCP_FIXTURE",
            "NANH_CLAUDE_PERSIST_OWNERS",
        ] {
            command.env_remove(key);
        }
        // This is the same configuration writer and source-policy branch as the
        // launched CLI, bounded by this fixture's existing child watchdog.
        for key in [
            "NANH_CONFIGURATION_STD_RENAME_WORKER",
            "NANH_CLAUDE_WINDOWS_PERSIST_POLICY",
            "NANH_CLAUDE_WINDOWS_SOURCE_POLICY",
            "NANH_CLAUDE_PERSIST_CUTOFF_MS",
        ] {
            command.env_remove(key);
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        if let Some(source_policy) = source_policy {
            let cutoff = std::time::SystemTime::now()
                .checked_add(deadline.saturating_duration_since(Instant::now()))?
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?
                .as_millis();
            command
                .env("NANH_CONFIGURATION_STD_RENAME_WORKER", "1")
                .env("NANH_CLAUDE_WINDOWS_PERSIST_POLICY", "std-rename")
                .env("NANH_CLAUDE_WINDOWS_SOURCE_POLICY", source_policy)
                .env("NANH_CLAUDE_PERSIST_CUTOFF_MS", cutoff.to_string());
        }
        let mut child = command.spawn().ok()?;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    return (Instant::now() < deadline).then(|| status.code()).flatten();
                }
                Ok(None) => (),
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
            }
            if Instant::now() >= deadline {
                child.kill().ok()?;
                child.wait().ok()?;
                return None;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[cfg(feature = "desktop-qualification")]
    fn lifecycle_roots(workspace: &Path) -> Vec<File> {
        let home = workspace.join("profile/home");
        let local = home.join("AppData/Local");
        let roaming = home.join("AppData/Roaming");
        for directory in [
            &local,
            &roaming,
            &workspace.join("facts"),
            &workspace.join("profile/nanh"),
        ] {
            nan_harness_private_fs::create_private_dir_all(directory).unwrap();
        }
        let lease = |directory: &Path| {
            fs::OpenOptions::new()
                .read(true)
                .share_mode(3)
                .custom_flags(0x0200_0000 | 0x0020_0000)
                .open(directory)
                .unwrap()
        };
        // Match token preparation: hold every Local ancestor and Roaming BEFORE
        // exclusive creation of either source-defined app root.
        let mut held: Vec<_> = local.ancestors().map(lease).collect();
        held.push(lease(&roaming));
        for root in [roaming.join("Claude"), local.join("Claude-3p")] {
            nan_harness_private_fs::create_private_dir(&root).unwrap();
            held.push(lease(&root));
        }
        held
    }

    #[cfg(feature = "desktop-qualification")]
    #[test]
    fn configuration_lifecycle_under_real_precreation_leases_preserves_atomicity() {
        let base = std::env::var_os("RUNNER_TEMP")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let temporary = tempfile::tempdir_in(base).unwrap();
        let workspace = temporary.path().canonicalize().unwrap();
        nan_harness_private_fs::restrict_path(
            &workspace,
            nan_harness_private_fs::PrivatePathKind::Directory,
        )
        .unwrap();
        let directories = lifecycle_roots(&workspace);
        assert_eq!(
            lifecycle_child(&workspace, None),
            Some(0),
            "closed lifecycle failed"
        );
        let destination =
            workspace.join("profile/home/AppData/Roaming/Claude/claude_desktop_config.json");
        let mut file = open_private_new(&destination).unwrap();
        file.write_all(b"{}").unwrap();
        drop(file);
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&destination)
            .unwrap();
        assert_eq!(
            lifecycle_child(&workspace, None),
            Some(5),
            "target-blocked access failure not preserved"
        );
        assert_eq!(fs::read(&destination).unwrap(), b"{}");
        drop(held);
        assert_eq!(
            fs::read_dir(destination.parent().unwrap()).unwrap().count(),
            1
        );
        drop(directories);
    }

    #[cfg(feature = "desktop-qualification")]
    fn record_postinstall_fixture_outcome(result: Option<i32>) {
        if std::env::var("NANH_CONFIGURATION_POSTINSTALL_OBSERVATION").as_deref() != Ok("1") {
            return;
        }
        for (key, value) in [
            ("GITHUB_ACTIONS", "true"),
            ("RUNNER_ENVIRONMENT", "github-hosted"),
            ("RUNNER_OS", "Windows"),
        ] {
            assert_eq!(std::env::var(key).as_deref(), Ok(value));
        }
        let source = std::env::var("GITHUB_SHA").unwrap();
        assert!(source.len() == 40 && source.bytes().all(|byte| byte.is_ascii_hexdigit()));
        let outcome = match result {
            Some(0) => "passed",
            Some(32) => "sharing-violation",
            Some(33) => "other-failure",
            Some(_) => "unexpected-exit",
            None => "deadline-or-unavailable",
        };
        let directory = PathBuf::from(std::env::var_os("RUNNER_TEMP").unwrap());
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join("configuration-fixture.json"))
            .unwrap();
        let facts = serde_json::json!({"schemaVersion":1,"mechanism":"windows-configuration-fixture",
            "diagnosticsOnly":true,"sourceSha":source,"phase":"after-installation","outcome":outcome});
        serde_json::to_writer(&mut output, &facts).unwrap();
        output.sync_all().unwrap();
    }

    #[cfg(feature = "desktop-qualification")]
    #[test]
    fn production_std_rename_lifecycle_under_precreation_leases_restores_documents() {
        let base = std::env::var_os("RUNNER_TEMP")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        for (source_policy, expected) in [
            ("official-2.19675.0-97910a066871", 0),
            ("unadmitted-synthetic-policy", 97),
        ] {
            let temporary = tempfile::tempdir_in(&base).unwrap();
            let workspace = temporary.path().canonicalize().unwrap();
            nan_harness_private_fs::restrict_path(
                &workspace,
                nan_harness_private_fs::PrivatePathKind::Directory,
            )
            .unwrap();
            let directories = lifecycle_roots(&workspace);
            let result = lifecycle_child(&workspace, Some(source_policy));
            if expected == 0 {
                record_postinstall_fixture_outcome(result);
            }
            assert_eq!(
                result,
                Some(expected),
                "production writer returned an unexpected closed exit category"
            );
            let paths = DesktopPaths::new(
                &workspace.join("profile/home/AppData/Roaming/Claude"),
                &workspace.join("profile/home/AppData/Local/Claude-3p"),
                &workspace.join("profile/nanh"),
            );
            assert!(
                paths.documents().into_iter().all(|path| !path.exists()),
                "configuration destinations survived restoration or rejected policy"
            );
            for root in [
                paths.documents()[0].parent().unwrap(),
                paths.documents()[3].parent().unwrap(),
            ] {
                if expected != 0 && root == paths.documents()[3].parent().unwrap() {
                    // Rejected policy must not create the configuration library.
                    assert!(!root.exists());
                    continue;
                }
                assert_eq!(
                    fs::read_dir(root).unwrap().count(),
                    0,
                    "temporary configuration survived the bounded child"
                );
            }
            drop(directories);
        }
    }

    #[cfg(feature = "desktop-qualification")]
    #[test]
    fn std_rename_under_exact_fresh_profile_leases_preserves_source_identity() {
        use std::time::{Duration, Instant};
        let base = std::env::var_os("RUNNER_TEMP")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let temporary = tempfile::tempdir_in(base).unwrap();
        let workspace = temporary.path().canonicalize().unwrap();
        nan_harness_private_fs::restrict_path(
            &workspace,
            nan_harness_private_fs::PrivatePathKind::Directory,
        )
        .unwrap();
        // This existing fixture helper matches FreshClaudeWindowsProfile's exact
        // READ|WRITE directory sharing and lock-before-root-creation ordering.
        let directories = lifecycle_roots(&workspace);
        let destination =
            workspace.join("profile/home/AppData/Roaming/Claude/claude_desktop_config.json");
        let mut source = TempFileBuilder::new()
            .prefix(".nan-")
            .make_in(destination.parent().unwrap(), open_private_new)
            .unwrap();
        source.write_all(b"synthetic-normal-config").unwrap();
        source.flush().unwrap();
        source.as_file().sync_all().unwrap();
        let expected = retained_rename_source(source.path(), 7).unwrap();
        let mut boundary = qualification_prelaunch::StdRenameBoundary::OriginalSource;
        assert!(
            rename_written_configuration_once(
                source,
                &destination,
                Instant::now() + Duration::from_secs(2),
                &mut boundary
            )
            .is_ok(),
            "closed leased-parent rename failed"
        );
        let actual = retained_rename_source(&destination, 5).unwrap();
        assert!(actual == expected);
        nan_harness_private_fs::verify_private_file(actual.as_file()).unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"synthetic-normal-config");
        drop(actual);
        drop(expected);

        // Under the SAME live parent leases, a genuine source DELETE-share
        // denial must still fail once and retain both historical identities.
        let mut replacement = TempFileBuilder::new()
            .prefix(".nan-")
            .make_in(destination.parent().unwrap(), open_private_new)
            .unwrap();
        replacement.write_all(b"replacement-fixture").unwrap();
        replacement.flush().unwrap();
        replacement.as_file().sync_all().unwrap();
        let historical = replacement.path().to_owned();
        let replacement_identity = retained_rename_source(&historical, 7).unwrap();
        let denial = fs::OpenOptions::new()
            .read(true)
            .share_mode(3)
            .open(&historical)
            .unwrap();
        let absent_destination = destination.with_file_name("absent-synthetic.json");
        let error = rename_written_configuration_once(
            replacement,
            &absent_destination,
            Instant::now() + Duration::from_secs(2),
            &mut boundary,
        )
        .unwrap_err();
        assert_eq!(
            boundary,
            qualification_prelaunch::StdRenameBoundary::RenameDispatch
        );
        assert_eq!(error.error.raw_os_error(), Some(32));
        assert!(!absent_destination.exists());
        let retained = retained_rename_source(&historical, 7).unwrap();
        assert!(retained == replacement_identity);
        assert_eq!(fs::read(&historical).unwrap(), b"replacement-fixture");
        assert_eq!(fs::read(&destination).unwrap(), b"synthetic-normal-config");
        drop(retained);
        drop(replacement_identity);
        drop(denial);
        drop(error);
        drop(directories);
    }

    #[test]
    fn std_rename_trial_preserves_original_private_object_and_failed_destination() {
        use std::time::{Duration, Instant};
        let directory = tempfile::tempdir().unwrap();
        let source = TempFileBuilder::new()
            .prefix(".nan-")
            .make_in(directory.path(), open_private_new)
            .unwrap();
        let mut source = source;
        source.write_all(b"synthetic-private").unwrap();
        source.flush().unwrap();
        source.as_file().sync_all().unwrap();
        let original = same_file::Handle::from_file(source.as_file().try_clone().unwrap()).unwrap();
        let destination = directory.path().join("configuration.json");
        // The test's original handle must close its WRITE access just like the writer.
        let expected = retained_rename_source(source.path(), 7).unwrap();
        assert!(expected == original);
        drop(original);
        rename_written_configuration_once(
            source,
            &destination,
            Instant::now() + Duration::from_secs(2),
            &mut qualification_prelaunch::StdRenameBoundary::OriginalSource,
        )
        .unwrap();
        let actual = retained_rename_source(&destination, 5).unwrap();
        assert!(actual == expected);
        nan_harness_private_fs::verify_private_file(actual.as_file()).unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"synthetic-private");
        drop(actual);
        drop(expected);
        let mut second = TempFileBuilder::new()
            .prefix(".nan-")
            .make_in(directory.path(), open_private_new)
            .unwrap();
        second.write_all(b"new-payload").unwrap();
        second.flush().unwrap();
        second.as_file().sync_all().unwrap();
        let historical = second.path().to_owned();
        let mut boundary = qualification_prelaunch::StdRenameBoundary::OriginalSource;
        let error = rename_written_configuration_once(
            second,
            &destination,
            Instant::now() + Duration::from_secs(2),
            &mut boundary,
        )
        .unwrap_err();
        assert_eq!(error.error.kind(), ErrorKind::AlreadyExists);
        assert_eq!(
            boundary,
            qualification_prelaunch::StdRenameBoundary::DestinationPreflight
        );
        assert_eq!(fs::read(&destination).unwrap(), b"synthetic-private");
        assert_eq!(fs::read(&historical).unwrap(), b"new-payload");
        drop(error);
        assert!(!historical.exists());
    }
    #[test]
    fn std_rename_trial_failed_dispatch_retains_exact_source_without_fallback() {
        use std::time::{Duration, Instant};
        let directory = tempfile::tempdir().unwrap();
        let mut source = TempFileBuilder::new()
            .prefix(".nan-")
            .make_in(directory.path(), open_private_new)
            .unwrap();
        source.write_all(b"retained-private").unwrap();
        source.flush().unwrap();
        source.as_file().sync_all().unwrap();
        let historical = source.path().to_owned();
        // Permit the existing writer, but forbid DELETE on this exact source.
        let blocker = fs::OpenOptions::new()
            .read(true)
            .share_mode(3)
            .open(&historical)
            .unwrap();
        let expected = same_file::Handle::from_file(blocker.try_clone().unwrap()).unwrap();
        let destination = directory.path().join("absent.json");
        let error = rename_written_configuration_once(
            source,
            &destination,
            Instant::now() + Duration::from_secs(2),
            &mut qualification_prelaunch::StdRenameBoundary::OriginalSource,
        )
        .unwrap_err();
        assert_eq!(error.error.raw_os_error(), Some(32));
        assert!(!destination.exists());
        let retained = retained_rename_source(&historical, 5).unwrap();
        assert!(retained == expected);
        assert_eq!(fs::read(&historical).unwrap(), b"retained-private");
        nan_harness_private_fs::verify_private_file(retained.as_file()).unwrap();
        drop(retained);
        drop(expected);
        drop(blocker);
        drop(error);
        assert!(!historical.exists());
    }

    #[test]
    fn std_rename_trial_rejects_unscoped_flags_and_expired_dispatch() {
        use std::time::Instant;
        assert!(!std_rename_requested(None, false).unwrap());
        assert!(std_rename_requested(Some("std-rename"), true).unwrap());
        assert!(std_rename_requested(Some("std-rename"), false).is_err());
        assert!(std_rename_requested(Some("other"), true).is_err());
        let directory = tempfile::tempdir().unwrap();
        let source = TempFileBuilder::new()
            .make_in(directory.path(), open_private_new)
            .unwrap();
        let historical = source.path().to_owned();
        let destination = directory.path().join("new.json");
        let error = rename_written_configuration_once(
            source,
            &destination,
            Instant::now(),
            &mut qualification_prelaunch::StdRenameBoundary::OriginalSource,
        )
        .unwrap_err();
        assert_eq!(error.error.kind(), ErrorKind::TimedOut);
        assert!(!destination.exists());
        assert!(historical.exists());
        drop(error);
        assert!(!historical.exists());
    }

    #[test]
    fn closed_persist_preserves_private_file_and_cleans_failed_original_path() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("config.json");
        let mut temporary = tempfile::Builder::new()
            .make_in(temp.path(), open_private_new)
            .unwrap();
        temporary.write_all(b"replacement").unwrap();
        temporary.as_file().sync_all().unwrap();
        persist_closed_configuration(temporary.into_temp_path(), &destination).unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"replacement");
        // The same source file ACL survives MoveFileEx, independently checked by
        // the private reader rather than inferred from a permission bit.
        assert!(nan_harness_private_fs::open_private_read(&destination).is_ok());
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&destination)
            .unwrap();
        let mut temporary = tempfile::Builder::new()
            .make_in(temp.path(), open_private_new)
            .unwrap();
        temporary.write_all(b"must not replace").unwrap();
        temporary.as_file().sync_all().unwrap();
        let source = temporary.path().to_owned();
        let error =
            persist_closed_configuration(temporary.into_temp_path(), &destination).unwrap_err();
        assert_eq!(error.error.raw_os_error(), Some(5));
        let retained: &Path = error.path.as_ref();
        assert_eq!(retained, source);
        assert_eq!(fs::read(&destination).unwrap(), b"replacement");
        assert_eq!(fs::read(&source).unwrap(), b"must not replace");
        drop(error);
        assert!(!source.exists());
        drop(held);
    }

    #[test]
    fn closed_persist_retries_original_path_after_source_releases() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("config.json");
        fs::write(&destination, b"original").unwrap();
        let mut temporary = tempfile::Builder::new()
            .make_in(temp.path(), open_private_new)
            .unwrap();
        temporary.write_all(b"replacement").unwrap();
        temporary.as_file().sync_all().unwrap();
        let temporary = temporary.into_temp_path();
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(3)
            .open(&temporary)
            .unwrap();
        let source = temporary.to_path_buf();
        let initial = temporary.persist(&destination).unwrap_err();
        assert_eq!(initial.error.raw_os_error(), Some(32));
        let retained: &Path = initial.path.as_ref();
        assert_eq!(retained, source);
        assert_eq!(fs::read(&source).unwrap(), b"replacement");
        assert_eq!(fs::read(&destination).unwrap(), b"original");
        let temporary = initial.path;
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(40));
            drop(held);
        });
        let result = persist_closed_configuration(temporary, &destination);
        release.join().unwrap();
        assert!(result.is_ok());
        assert_eq!(fs::read(destination).unwrap(), b"replacement");
    }

    #[test]
    fn closed_persist_can_outlast_old_window_without_rewriting_file() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("config.json");
        fs::write(&destination, b"original").unwrap();
        let mut temporary = tempfile::Builder::new()
            .make_in(temp.path(), open_private_new)
            .unwrap();
        temporary.write_all(b"replacement").unwrap();
        temporary.as_file().sync_all().unwrap();
        let temporary = temporary.into_temp_path();
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(3)
            .open(&temporary)
            .unwrap();
        let source = temporary.to_path_buf();
        let initial = temporary.persist(&destination).unwrap_err();
        assert_eq!(initial.error.raw_os_error(), Some(32));
        let retained: &Path = initial.path.as_ref();
        assert_eq!(retained, source);
        assert_eq!(fs::read(&source).unwrap(), b"replacement");
        assert_eq!(fs::read(&destination).unwrap(), b"original");
        let temporary = initial.path;
        let original_path = temporary.to_path_buf();
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(400));
            drop(held);
        });
        let result = persist_closed_configuration_until(
            temporary,
            &destination,
            std::time::Instant::now() + Duration::from_secs(2),
        );
        release.join().unwrap();
        assert!(result.is_ok());
        assert!(!original_path.exists());
        assert_eq!(fs::read(destination).unwrap(), b"replacement");
    }

    #[test]
    fn closed_persist_source_sharing_failure_retains_original_path_until_cleanup() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("config.json");
        fs::write(&destination, b"original").unwrap();
        let mut temporary = tempfile::Builder::new()
            .make_in(temp.path(), open_private_new)
            .unwrap();
        temporary.write_all(b"replacement").unwrap();
        temporary.as_file().sync_all().unwrap();
        let temporary = temporary.into_temp_path();
        let source = temporary.to_path_buf();
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(3)
            .open(&source)
            .unwrap();
        let error = persist_closed_configuration_until(
            temporary,
            &destination,
            std::time::Instant::now() + Duration::from_millis(10),
        )
        .unwrap_err();
        assert_eq!(error.error.raw_os_error(), Some(32));
        let retained: &Path = error.path.as_ref();
        assert_eq!(retained, source);
        assert_eq!(fs::read(&source).unwrap(), b"replacement");
        assert_eq!(fs::read(&destination).unwrap(), b"original");
        drop(held);
        drop(error);
        assert!(!source.exists());
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
    }

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
                fs::OpenOptions::new()
                    .read(true)
                    .share_mode(3)
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
    fn configuration_persist_retries_same_file_after_source_handle_release() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");
        fs::write(&path, b"original").unwrap();
        let mut temporary = tempfile::Builder::new()
            .prefix(".nan-")
            .make_in(temp.path(), open_private_new)
            .unwrap();
        temporary.write_all(b"replacement").unwrap();
        temporary.flush().unwrap();
        let source = temporary.path().to_owned();
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(3)
            .open(&source)
            .unwrap();
        let initial = temporary.persist(&path).unwrap_err();
        assert_eq!(initial.error.raw_os_error(), Some(32));
        assert_eq!(initial.file.path(), source);
        assert_eq!(fs::read(&path).unwrap(), b"original");
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(40));
            drop(held);
        });
        let result = persist_windows_configuration(initial.file, &path);
        release.join().unwrap();
        assert!(result.is_ok());
        assert_eq!(fs::read(path).unwrap(), b"replacement");
    }

    #[test]
    fn configuration_persist_target_access_failure_preserves_original() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");
        fs::write(&path, b"original").unwrap();
        let held = fs::OpenOptions::new()
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
            matches!(result, Err(ClaudeDesktopError::Write(error)) if error.raw_os_error()==Some(5))
        );
        assert!(started.elapsed() < Duration::from_secs(2));
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
        let mut temporary = tempfile::NamedTempFile::new_in(temp.path()).unwrap();
        temporary.write_all(b"replacement").unwrap();
        let source = temporary.path().to_owned();
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(3)
            .open(&source)
            .unwrap();
        let error = persist_configuration_file(temporary, &path, false).unwrap_err();
        assert_eq!(error.error.raw_os_error(), Some(32));
        assert_eq!(fs::read(&path).unwrap(), b"original");
        assert!(source.exists());
        drop(held);
        drop(error);
        assert!(!source.exists());
    }
    #[test]
    fn configuration_persist_child_worker() {
        use std::io::Read as _;
        if std::env::var("NANH_CONFIGURATION_PERSIST_WORKER").as_deref() != Ok("1") {
            return;
        }
        let mut input = String::new();
        std::io::stdin()
            .take(4097)
            .read_to_string(&mut input)
            .unwrap();
        if input.len() > 4096 {
            std::process::exit(34);
        }
        let path = PathBuf::from(input);
        if !path.is_absolute() || path.is_symlink() || !path.parent().is_some_and(Path::is_dir) {
            std::process::exit(34);
        }
        let result = atomic_write_configuration(
            &path,
            b"{\"deploymentMode\":\"3p\"}\n",
            None,
            qualification_prelaunch::ConfigurationDocument::NormalConfig,
        );
        // Closed exit codes only; no child output or private path is retained.
        std::process::exit(match result {
            Ok(()) => 0,
            Err(ClaudeDesktopError::Write(error)) if error.raw_os_error() == Some(32) => 32,
            Err(_) => 33,
        });
    }

    fn child_configuration_persist(path: &Path) -> Option<i32> {
        use std::process::{Command, Stdio};
        use std::time::{Duration, Instant};
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "commands::claude_desktop::session::configuration_persist_tests::configuration_persist_child_worker"])
            .env("NANH_CONFIGURATION_PERSIST_WORKER", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(path.to_str().unwrap().as_bytes())
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                return status.code();
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                return None;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn configuration_persist_child_with_parent_retained_directory_handles() {
        let temp = match std::env::var_os("RUNNER_TEMP") {
            Some(value) => {
                let directory = PathBuf::from(value);
                assert!(
                    directory.is_absolute() && directory.is_dir(),
                    "invalid runner temporary directory"
                );
                tempfile::tempdir_in(directory).unwrap()
            }
            None => tempfile::tempdir().unwrap(),
        };
        let root = temp.path().canonicalize().unwrap();
        let app_data = root.join("profile").join("home").join("AppData");
        let local = app_data.join("Local");
        let roaming = app_data.join("Roaming");
        let normal = roaming.join("Claude");
        let third_party = local.join("Claude-3p");
        nan_harness_private_fs::create_private_dir_all(&normal).unwrap();
        nan_harness_private_fs::create_private_dir_all(&third_party).unwrap();
        let retained: Vec<_> = local
            .ancestors()
            .chain([roaming.as_path(), normal.as_path(), third_party.as_path()])
            .map(|directory| {
                fs::OpenOptions::new()
                    .read(true)
                    .share_mode(3)
                    .custom_flags(0x0200_0000 | 0x0020_0000)
                    .open(directory)
                    .unwrap()
            })
            .collect();
        let config = normal.join("claude_desktop_config.json");
        let while_retained = child_configuration_persist(&config);
        let while_retained_replace = child_configuration_persist(&config);
        let third_party_config = third_party.join("claude_desktop_config.json");
        let local_while_retained = child_configuration_persist(&third_party_config);
        drop(retained);
        let after_release = child_configuration_persist(&config);
        assert_eq!(
            after_release,
            Some(0),
            "child persistence after releasing directories failed"
        );
        assert_eq!(
            while_retained,
            Some(0),
            "parent-directory sharing prevented child persistence (closed exit code)"
        );
        assert_eq!(
            while_retained_replace,
            Some(0),
            "parent-directory sharing prevented child replacement (closed exit code)"
        );
        assert_eq!(
            local_while_retained,
            Some(0),
            "parent-directory sharing prevented local child persistence (closed exit code)"
        );
        assert_eq!(fs::read(config).unwrap(), b"{\"deploymentMode\":\"3p\"}\n");
    }
}
