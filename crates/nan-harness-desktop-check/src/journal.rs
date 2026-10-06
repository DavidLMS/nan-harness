//! Recovery owns only newly allocated children of a private run directory.

use crate::report::digest;
use nan_harness_private_fs::{
    create_private_dir, create_private_dir_all, open_private_new, open_private_read_write,
};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{self, Read as _, Write as _};
use std::path::{Component, Path, PathBuf};

const MAX_JOURNAL_BYTES: u64 = 128 * 1024;
const MAX_ENTRIES: usize = 100_000;
const MAX_TREE_BYTES: u64 = 16 * 1024 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct State {
    schema_version: u8,
    run_id: String,
    resources: Vec<Resource>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Resource {
    name: String,
    fingerprint: Option<String>,
    removed: bool,
}

/// The OS file lock is held until the journal is dropped, including on errors.
pub struct Journal {
    root: PathBuf,
    state: State,
    lock: File,
}

impl Drop for Journal {
    fn drop(&mut self) {
        // A concurrent fork can temporarily inherit the open file description
        // before close-on-exec runs. Release ownership explicitly, not only by
        // closing our descriptor and waiting for that child's copy to disappear.
        let _ = self.lock.unlock();
    }
}

#[derive(Debug, thiserror::Error)]
pub enum JournalError {
    #[error("private recovery state could not be accessed")]
    Io(#[from] io::Error),
    #[error("another checker owns this run")]
    Locked,
    #[error("recovery state is invalid")]
    Invalid,
    #[error("owned files changed; recovery state was preserved")]
    Conflict,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SealOperation {
    Fingerprint,
    Persist,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SealObservation {
    pub(crate) operation: SealOperation,
    pub(crate) fingerprint_failure: Option<FingerprintFailure>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct FingerprintFailure {
    artifact_kind: FingerprintArtifact,
    stage: FingerprintStage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    file_access: Option<FileAccessObservation>,
}

// Metadata only: never export a filename, numeric owner, mode or contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FileAccessObservation {
    owner_readable: bool,
    owner_matches: bool,
    multiple_links: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum FingerprintArtifact {
    ProbeRoot,
    Workspace,
    ManagedProfile,
    ProbeReceipt,
    ProbeSpec,
    OtherOwned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum FingerprintStage {
    Metadata,
    DirectoryEnumeration,
    DirectoryEntry,
    SymlinkTarget,
    FileOpen,
    FileRead,
}

fn fingerprint_failure(relative: &Path, stage: FingerprintStage) -> FingerprintFailure {
    let components: Vec<_> = relative.components().take(3).collect();
    let artifact_kind = match components.as_slice() {
        [] => FingerprintArtifact::ProbeRoot,
        [Component::Normal(a), Component::Normal(b), ..]
            if *a == "workspace" && *b == "profile" =>
        {
            FingerprintArtifact::ManagedProfile
        }
        [Component::Normal(a), ..] if *a == "workspace" => FingerprintArtifact::Workspace,
        [Component::Normal(a)] if *a == "probe.json" => FingerprintArtifact::ProbeReceipt,
        [Component::Normal(a)] if *a == "spec.json" => FingerprintArtifact::ProbeSpec,
        _ => FingerprintArtifact::OtherOwned,
    };
    FingerprintFailure {
        artifact_kind,
        stage,
        file_access: None,
    }
}

fn fingerprint_io_failure(
    relative: &Path,
    stage: FingerprintStage,
    error: io::Error,
) -> (JournalError, Option<FingerprintFailure>) {
    (
        JournalError::Io(error),
        Some(fingerprint_failure(relative, stage)),
    )
}

impl Journal {
    /// Allocate a new run; existing runs and resources are never reused.
    ///
    /// # Errors
    /// Fails if private ownership or exclusive locking cannot be established.
    pub fn create(parent: &Path) -> Result<Self, JournalError> {
        create_private_dir_all(parent)?;
        let mut random = [0u8; 16];
        getrandom::fill(&mut random).map_err(|_| JournalError::Invalid)?;
        let run_id = digest(&random)[..32].to_owned();
        let root = parent.join(&run_id);
        create_private_dir(&root)?;
        let lock = open_private_new(&root.join("lock"))?;
        lock.try_lock().map_err(|_| JournalError::Locked)?;
        let journal = Self {
            root,
            state: State {
                schema_version: 1,
                run_id,
                resources: Vec::new(),
            },
            lock,
        };
        journal.save()?;
        Ok(journal)
    }

    /// Open a previous run without following a substituted run directory.
    ///
    /// # Errors
    /// Fails on invalid identity, symlinks, corrupt state or a live owner.
    pub fn open(parent: &Path, run_id: &str) -> Result<Self, JournalError> {
        if run_id.len() != 32 || !run_id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(JournalError::Invalid);
        }
        let root = parent.join(run_id);
        if !fs::symlink_metadata(&root)?.file_type().is_dir() {
            return Err(JournalError::Invalid);
        }
        for name in ["lock", "journal.json"] {
            if !fs::symlink_metadata(root.join(name))?.file_type().is_file() {
                return Err(JournalError::Invalid);
            }
        }
        let lock = open_private_read_write(&root.join("lock"))?;
        lock.try_lock().map_err(|_| JournalError::Locked)?;
        let mut bytes = Vec::new();
        File::open(root.join("journal.json"))?
            .take(MAX_JOURNAL_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_JOURNAL_BYTES {
            return Err(JournalError::Invalid);
        }
        let state: State = serde_json::from_slice(&bytes).map_err(|_| JournalError::Invalid)?;
        if state.schema_version != 1
            || state.run_id != run_id
            || state.resources.len() > 64
            || state
                .resources
                .iter()
                .any(|resource| !safe_name(&resource.name))
        {
            return Err(JournalError::Invalid);
        }
        Ok(Self { root, state, lock })
    }

    #[must_use]
    pub fn run_id(&self) -> &str {
        &self.state.run_id
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn pending_names(&self) -> Vec<String> {
        self.state
            .resources
            .iter()
            .filter(|entry| !entry.removed && entry.fingerprint.is_none())
            .map(|entry| entry.name.clone())
            .collect()
    }

    /// Record ownership before creating a resource directory.
    ///
    /// # Errors
    /// Rejects unsafe names, existing paths and duplicate claims.
    pub fn reserve(&mut self, name: &str) -> Result<PathBuf, JournalError> {
        if !safe_name(name)
            || self.state.resources.len() >= 64
            || self.state.resources.iter().any(|entry| entry.name == name)
        {
            return Err(JournalError::Invalid);
        }
        let path = self.root.join(name);
        if fs::symlink_metadata(&path).is_ok() {
            return Err(JournalError::Conflict);
        }
        self.state.resources.push(Resource {
            name: name.into(),
            fingerprint: None,
            removed: false,
        });
        self.save()?;
        create_private_dir(&path)?;
        Ok(path)
    }

    /// Snapshot installed content; a changed installation is preserved at cleanup.
    ///
    /// # Errors
    /// Fails if the owned directory cannot be completely fingerprinted.
    pub fn seal(&mut self, name: &str) -> Result<(), JournalError> {
        self.seal_observed(name).map_err(|(error, _)| error)
    }

    pub(crate) fn seal_observed(
        &mut self,
        name: &str,
    ) -> Result<(), (JournalError, Option<SealObservation>)> {
        let index = self
            .state
            .resources
            .iter()
            .position(|entry| entry.name == name)
            .ok_or((JournalError::Invalid, None))?;
        let fingerprint =
            tree_fingerprint_observed(&self.root.join(name)).map_err(|(error, context)| {
                (
                    error,
                    Some(SealObservation {
                        operation: SealOperation::Fingerprint,
                        fingerprint_failure: context,
                    }),
                )
            })?;
        self.state.resources[index].fingerprint = Some(fingerprint);
        self.save().map_err(|error| {
            (
                error,
                Some(SealObservation {
                    operation: SealOperation::Persist,
                    fingerprint_failure: None,
                }),
            )
        })
    }

    /// Remove only owned, unchanged resources; keep the journal for repeatable recovery.
    ///
    /// # Errors
    /// On conflict or deletion failure, preserves the affected resource and receipt.
    pub fn cleanup(&mut self, ephemeral: bool) -> Result<(), JournalError> {
        if ephemeral {
            return Ok(());
        }
        for index in (0..self.state.resources.len()).rev() {
            self.remove_resource(index)?;
        }
        Ok(())
    }

    fn remove_resource(&mut self, index: usize) -> Result<(), JournalError> {
        let resource = &self.state.resources[index];
        if resource.removed {
            return Ok(());
        }
        let path = self.root.join(&resource.name);
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
            Ok(metadata) => {
                if !metadata.file_type().is_dir() {
                    return Err(JournalError::Conflict);
                }
                if let Some(expected) = &resource.fingerprint {
                    if &tree_fingerprint(&path)? != expected {
                        return Err(JournalError::Conflict);
                    }
                } else if fs::read_dir(&path)?.next().is_some() {
                    // A crash before sealing cannot establish the final ownership snapshot.
                    return Err(JournalError::Conflict);
                }
                fs::remove_dir_all(&path)?;
            }
        }
        self.state.resources[index].removed = true;
        self.save()
    }

    fn save(&self) -> Result<(), JournalError> {
        let bytes = serde_json::to_vec(&self.state).map_err(|_| JournalError::Invalid)?;
        // Windows needs WRITE_DAC on the original handle before hardening it.
        let mut temporary = tempfile::Builder::new()
            .prefix(".journal-")
            .make_in(&self.root, open_private_new)?;
        temporary.write_all(&bytes)?;
        temporary.as_file().sync_all()?;
        temporary
            .persist(self.root.join("journal.json"))
            .map_err(|error| error.error)?;
        Ok(())
    }
}

fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 80
        && name != "lock"
        && name != "journal.json"
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && Path::new(name)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

fn tree_fingerprint(root: &Path) -> Result<String, JournalError> {
    tree_fingerprint_observed(root).map_err(|(error, _)| error)
}

fn tree_fingerprint_observed(
    root: &Path,
) -> Result<String, (JournalError, Option<FingerprintFailure>)> {
    let mut pending = vec![root.to_path_buf()];
    let mut entries = Vec::new();
    let mut remaining = MAX_TREE_BYTES;
    while let Some(path) = pending.pop() {
        if entries.len() >= MAX_ENTRIES {
            return Err((JournalError::Invalid, None));
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| (JournalError::Invalid, None))?;
        if relative.components().count() > 64 {
            return Err((JournalError::Invalid, None));
        }
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| fingerprint_io_failure(relative, FingerprintStage::Metadata, error))?;
        let payload = if metadata.file_type().is_symlink() {
            format!(
                "link:{}",
                fs::read_link(&path)
                    .map_err(|error| (
                        JournalError::Io(error),
                        Some(fingerprint_failure(
                            relative,
                            FingerprintStage::SymlinkTarget
                        ))
                    ))?
                    .to_string_lossy()
            )
        } else if metadata.is_dir() {
            for child in fs::read_dir(&path).map_err(|error| {
                fingerprint_io_failure(relative, FingerprintStage::DirectoryEnumeration, error)
            })? {
                if pending.len() + entries.len() >= MAX_ENTRIES {
                    return Err((JournalError::Invalid, None));
                }
                pending.push(
                    child
                        .map_err(|error| {
                            fingerprint_io_failure(
                                relative,
                                FingerprintStage::DirectoryEntry,
                                error,
                            )
                        })?
                        .path(),
                );
            }
            "directory".into()
        } else if metadata.is_file() {
            use sha2::{Digest as _, Sha256};
            let mut hasher = Sha256::new();
            let mut file = File::open(&path).map_err(|error| {
                let (error, context) =
                    fingerprint_io_failure(relative, FingerprintStage::FileOpen, error);
                #[cfg(unix)]
                let context = {
                    let mut context = context;
                    use std::os::unix::fs::MetadataExt as _;
                    if let Some(context) = &mut context {
                        context.file_access = Some(FileAccessObservation {
                            owner_readable: metadata.mode() & 0o400 != 0,
                            owner_matches: metadata.uid() == nix::unistd::Uid::effective().as_raw(),
                            multiple_links: metadata.nlink() > 1,
                        });
                    }
                    context
                };
                (error, context)
            })?;
            let mut buffer = vec![0u8; 64 * 1024];
            loop {
                let count = file.read(&mut buffer).map_err(|error| {
                    fingerprint_io_failure(relative, FingerprintStage::FileRead, error)
                })?;
                if count == 0 {
                    break;
                }
                remaining = remaining
                    .checked_sub(count as u64)
                    .ok_or((JournalError::Invalid, None))?;
                hasher.update(&buffer[..count]);
            }
            digest(&hasher.finalize())
        } else {
            return Err((JournalError::Conflict, None));
        };
        entries.push((relative.to_path_buf(), payload));
    }
    entries.sort();
    let bytes = serde_json::to_vec(&entries).map_err(|_| (JournalError::Invalid, None))?;
    Ok(digest(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn unreadable_owned_file_reports_access_without_changing_permissions() {
        use std::os::unix::fs::PermissionsExt as _;
        if nix::unistd::Uid::effective().is_root() {
            return;
        }
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("private-name");
        fs::write(&file, "private contents").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o200)).unwrap();
        let (_, context) = tree_fingerprint_observed(root.path()).unwrap_err();
        let context = context.unwrap();
        assert_eq!(context.stage, FingerprintStage::FileOpen);
        assert_eq!(
            context.file_access,
            Some(FileAccessObservation {
                owner_readable: false,
                owner_matches: true,
                multiple_links: false,
            })
        );
        let receipt = serde_json::to_string(&context).unwrap();
        assert!(!receipt.contains("private"));
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o200
        );
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    }

    #[test]
    fn injected_permission_failure_keeps_source_and_closed_file_read_context() {
        let error = io::Error::new(io::ErrorKind::PermissionDenied, "private message");
        let (error, context) = fingerprint_io_failure(
            Path::new("workspace/profile/private-name"),
            FingerprintStage::FileRead,
            error,
        );
        assert!(
            matches!(error, JournalError::Io(ref source) if source.kind() == io::ErrorKind::PermissionDenied)
        );
        let value = serde_json::to_value(context.unwrap()).unwrap();
        assert_eq!(
            value,
            serde_json::json!({"artifactKind":"managed-profile","stage":"file-read"})
        );
    }

    #[test]
    fn fingerprint_failure_classifies_only_fixed_owned_components() {
        for (path, expected) in [
            ("", FingerprintArtifact::ProbeRoot),
            ("workspace/read-target.txt", FingerprintArtifact::Workspace),
            (
                "workspace/profile/home/state",
                FingerprintArtifact::ManagedProfile,
            ),
            ("probe.json", FingerprintArtifact::ProbeReceipt),
            ("spec.json", FingerprintArtifact::ProbeSpec),
            ("nested/probe.json", FingerprintArtifact::OtherOwned),
            ("private-name", FingerprintArtifact::OtherOwned),
        ] {
            let context = fingerprint_failure(Path::new(path), FingerprintStage::FileRead);
            assert_eq!(context.artifact_kind, expected);
            let serialized = serde_json::to_string(&context).unwrap();
            assert!(!serialized.contains("private-name"));
            assert!(!serialized.contains("read-target"));
        }
    }

    #[test]
    fn fingerprint_metadata_failure_preserves_io_kind_and_owned_root_context() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("missing-private-root");
        let (error, context) = tree_fingerprint_observed(&root).unwrap_err();
        assert!(
            matches!(error, JournalError::Io(ref error) if error.kind() == io::ErrorKind::NotFound)
        );
        let context = context.unwrap();
        assert_eq!(context.artifact_kind, FingerprintArtifact::ProbeRoot);
        assert_eq!(context.stage, FingerprintStage::Metadata);
        assert!(matches!(tree_fingerprint(&root), Err(JournalError::Io(_))));
    }

    #[test]
    fn cleanup_preserves_preexisting_and_modified_content() {
        let parent = tempfile::tempdir().unwrap();
        let original = parent.path().join("existing");
        fs::write(&original, "user data").unwrap();
        let mut journal = Journal::create(parent.path()).unwrap();
        let app = journal.reserve("app").unwrap();
        fs::write(app.join("binary"), "installed").unwrap();
        journal.seal("app").unwrap();
        fs::write(app.join("binary"), "updated").unwrap();
        assert!(matches!(
            journal.cleanup(false),
            Err(JournalError::Conflict)
        ));
        assert_eq!(fs::read_to_string(original).unwrap(), "user data");
        assert_eq!(fs::read_to_string(app.join("binary")).unwrap(), "updated");
    }

    #[test]
    fn cleanup_is_idempotent_and_ephemeral_retains_installations() {
        let parent = tempfile::tempdir().unwrap();
        let mut journal = Journal::create(parent.path()).unwrap();
        let app = journal.reserve("app").unwrap();
        fs::write(app.join("binary"), "installed").unwrap();
        journal.seal("app").unwrap();
        journal.cleanup(true).unwrap();
        assert!(app.exists());
        journal.cleanup(false).unwrap();
        assert!(!app.exists());
        journal.cleanup(false).unwrap();
        let id = journal.run_id().to_owned();
        drop(journal);
        Journal::open(parent.path(), &id)
            .unwrap()
            .cleanup(false)
            .unwrap();
    }

    #[test]
    fn dropping_the_owner_unlocks_even_while_a_descriptor_copy_exists() {
        let parent = tempfile::tempdir().unwrap();
        let journal = Journal::create(parent.path()).unwrap();
        let id = journal.run_id().to_owned();
        let inherited = journal.lock.try_clone().unwrap();
        assert!(matches!(
            Journal::open(parent.path(), &id),
            Err(JournalError::Locked)
        ));
        drop(journal);
        let reopened = Journal::open(parent.path(), &id).unwrap();
        assert_eq!(reopened.run_id(), id);
        drop(inherited);
        assert!(matches!(
            Journal::open(parent.path(), &id),
            Err(JournalError::Locked)
        ));
    }

    #[test]
    fn rejects_paths_and_concurrent_owners_and_preserves_partial_installs() {
        let parent = tempfile::tempdir().unwrap();
        let mut journal = Journal::create(parent.path()).unwrap();
        assert!(Journal::open(parent.path(), journal.run_id()).is_err());
        assert!(journal.reserve("../outside").is_err());
        let pending = journal.reserve("partial").unwrap();
        fs::write(pending.join("download"), "partial").unwrap();
        assert!(matches!(
            journal.cleanup(false),
            Err(JournalError::Conflict)
        ));
        assert!(pending.exists());
    }

    #[cfg(unix)]
    #[test]
    fn substituted_directory_cannot_delete_a_user_target() {
        let parent = tempfile::tempdir().unwrap();
        let mut journal = Journal::create(parent.path()).unwrap();
        let app = journal.reserve("app").unwrap();
        journal.seal("app").unwrap();
        fs::remove_dir(&app).unwrap();
        std::os::unix::fs::symlink(parent.path(), &app).unwrap();
        assert!(matches!(
            journal.cleanup(false),
            Err(JournalError::Conflict)
        ));
        assert!(parent.path().exists());
    }
}
