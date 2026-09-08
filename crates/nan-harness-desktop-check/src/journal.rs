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
        let index = self
            .state
            .resources
            .iter()
            .position(|entry| entry.name == name)
            .ok_or(JournalError::Invalid)?;
        let fingerprint = tree_fingerprint(&self.root.join(name))?;
        self.state.resources[index].fingerprint = Some(fingerprint);
        self.save()
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
    let mut pending = vec![root.to_path_buf()];
    let mut entries = Vec::new();
    let mut remaining = MAX_TREE_BYTES;
    while let Some(path) = pending.pop() {
        if entries.len() >= MAX_ENTRIES {
            return Err(JournalError::Invalid);
        }
        let relative = path.strip_prefix(root).map_err(|_| JournalError::Invalid)?;
        if relative.components().count() > 64 {
            return Err(JournalError::Invalid);
        }
        let metadata = fs::symlink_metadata(&path)?;
        let payload = if metadata.file_type().is_symlink() {
            format!("link:{}", fs::read_link(&path)?.to_string_lossy())
        } else if metadata.is_dir() {
            for child in fs::read_dir(&path)? {
                if pending.len() + entries.len() >= MAX_ENTRIES {
                    return Err(JournalError::Invalid);
                }
                pending.push(child?.path());
            }
            "directory".into()
        } else if metadata.is_file() {
            use sha2::{Digest as _, Sha256};
            let mut hasher = Sha256::new();
            let mut file = File::open(&path)?;
            let mut buffer = vec![0u8; 64 * 1024];
            loop {
                let count = file.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                remaining = remaining
                    .checked_sub(count as u64)
                    .ok_or(JournalError::Invalid)?;
                hasher.update(&buffer[..count]);
            }
            digest(&hasher.finalize())
        } else {
            return Err(JournalError::Conflict);
        };
        entries.push((relative.to_path_buf(), payload));
    }
    entries.sort();
    let bytes = serde_json::to_vec(&entries).map_err(|_| JournalError::Invalid)?;
    Ok(digest(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

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
