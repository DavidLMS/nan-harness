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
    ) && qualification_prelaunch::enabled()
    {
        // Trial-only variation: close the fully written and synced file handle,
        // retaining tempfile's original RAII path and atomic replacement contract.
        let result = persist_closed_configuration(temporary.into_temp_path(), path)
            .inspect_err(|error| {
                if error.error.raw_os_error() == Some(32) {
                    qualification_persist_owners::observe_retained_path(&error.path, path);
                }
            })
            .map_err(|error| ClaudeDesktopError::Write(error.error));
        return qualification_prelaunch::observe_configuration_persist(result, document);
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

#[cfg(all(windows, any(feature = "desktop-qualification", test)))]
fn persist_closed_configuration(
    mut temporary: tempfile::TempPath,
    path: &Path,
) -> Result<(), tempfile::PathPersistError> {
    use std::time::{Duration, Instant};
    let deadline = Instant::now() + Duration::from_millis(250);
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
    // This child executes only configuration/session bookkeeping with placeholders.
    // No DesktopProcess, bridge listener, vendor executable or native query is used.
    #[cfg(feature = "desktop-qualification")]
    #[test]
    fn configuration_lifecycle_child_worker() {
        if std::env::var("NANH_CONFIGURATION_LIFECYCLE_WORKER").as_deref() != Ok("1") {
            return;
        }
        let result = (|| {
            let workspace = std::env::current_dir().map_err(ClaudeDesktopError::ReadConfig)?;
            let profile = workspace.join("profile");
            let paths = DesktopPaths::new(
                &profile.join("home/AppData/Roaming/Claude"),
                &profile.join("home/AppData/Local/Claude-3p"),
                &profile.join("nanh"),
            );
            if !qualification_prelaunch::enabled()
                || super::super::qualification_config::observation_directory(&paths).is_none()
            {
                return Err(ClaudeDesktopError::InvalidStatePath);
            }
            let _lock = SessionLock::acquire(&paths.lock)?;
            ensure_no_pending_recovery(&paths)?;
            let receipt = Receipt::capture(&paths)?;
            receipt.write(&paths.receipt)?;
            super::super::configuration::apply_gateway(
                &paths,
                "http://127.0.0.1:9",
                "synthetic-token",
            )?;
            let normal = super::super::configuration::read_json_object(paths.documents()[0])?;
            let managed = super::super::configuration::read_json_object(paths.documents()[3])?;
            if normal.get("deploymentMode").and_then(Value::as_str) != Some("3p")
                || managed.get("coworkTabEnabled").and_then(Value::as_bool) != Some(false)
            {
                return Err(ClaudeDesktopError::InvalidStatePath);
            }
            for document in paths.documents() {
                nan_harness_private_fs::open_private_read(document)
                    .map_err(ClaudeDesktopError::ReadConfig)?;
            }
            restore_receipt(&paths)?;
            if paths.documents().into_iter().any(Path::exists) {
                return Err(ClaudeDesktopError::InvalidStatePath);
            }
            Ok(())
        })();
        // All child output is suppressed; only fixed exit categories cross process.
        std::process::exit(match result {
            Ok(()) => 0,
            Err(ClaudeDesktopError::Write(error)) if error.raw_os_error() == Some(32) => 32,
            Err(_) => 33,
        });
    }

    #[cfg(feature = "desktop-qualification")]
    fn lifecycle_child(workspace: &Path) -> Option<i32> {
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
        let mut child = command.spawn().ok()?;
        let deadline = Instant::now() + Duration::from_secs(5);
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
            std::fs::OpenOptions::new()
                .read(true)
                .share_mode(1)
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
            lifecycle_child(&workspace),
            Some(0),
            "closed lifecycle failed"
        );
        let destination =
            workspace.join("profile/home/AppData/Roaming/Claude/claude_desktop_config.json");
        let mut file = nan_harness_private_fs::open_private_new(&destination).unwrap();
        file.write_all(b"{}").unwrap();
        drop(file);
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&destination)
            .unwrap();
        assert_eq!(
            lifecycle_child(&workspace),
            Some(32),
            "sharing failure not preserved"
        );
        assert_eq!(fs::read(&destination).unwrap(), b"{}");
        drop(held);
        assert_eq!(
            fs::read_dir(destination.parent().unwrap()).unwrap().count(),
            1
        );
        drop(directories);
    }

    #[test]
    fn closed_persist_preserves_private_file_and_cleans_failed_original_path() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("config.json");
        let mut temporary = tempfile::Builder::new()
            .make_in(temp.path(), nan_harness_private_fs::open_private_new)
            .unwrap();
        temporary.write_all(b"replacement").unwrap();
        temporary.as_file().sync_all().unwrap();
        persist_closed_configuration(temporary.into_temp_path(), &destination).unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"replacement");
        // The same source file ACL survives MoveFileEx, independently checked by
        // the private reader rather than inferred from a permission bit.
        assert!(nan_harness_private_fs::open_private_read(&destination).is_ok());
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&destination)
            .unwrap();
        let mut temporary = tempfile::Builder::new()
            .make_in(temp.path(), nan_harness_private_fs::open_private_new)
            .unwrap();
        temporary.write_all(b"must not replace").unwrap();
        temporary.as_file().sync_all().unwrap();
        let source = temporary.path().to_owned();
        let error =
            persist_closed_configuration(temporary.into_temp_path(), &destination).unwrap_err();
        assert_eq!(error.error.raw_os_error(), Some(32));
        let retained: &Path = error.path.as_ref();
        assert_eq!(retained, source);
        assert_eq!(fs::read(&destination).unwrap(), b"replacement");
        assert_eq!(fs::read(&source).unwrap(), b"must not replace");
        drop(error);
        assert!(!source.exists());
        drop(held);
    }

    #[test]
    fn closed_persist_retries_original_path_after_target_releases() {
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("config.json");
        fs::write(&destination, b"original").unwrap();
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&destination)
            .unwrap();
        let mut temporary = tempfile::Builder::new()
            .make_in(temp.path(), nan_harness_private_fs::open_private_new)
            .unwrap();
        temporary.write_all(b"replacement").unwrap();
        temporary.as_file().sync_all().unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(40));
            drop(held);
        });
        let result = persist_closed_configuration(temporary.into_temp_path(), &destination);
        release.join().unwrap();
        assert!(result.is_ok());
        assert_eq!(fs::read(destination).unwrap(), b"replacement");
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
                std::fs::OpenOptions::new()
                    .read(true)
                    .share_mode(1)
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
