use super::TemporaryError;
use nan_harness_core::launch_plan::{
    ConfigurationOverlay, PersistentStateEntry, PersistentStateKind,
};
use nan_harness_private_fs::{create_private_dir, create_private_dir_all, open_private_new};
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

/// Creates the missing harness-owned state entries in the real overlay source.
///
/// The overlay mirrors only what already exists in its source, so state a
/// harness creates during a launch would otherwise be written into the
/// temporary overlay and deleted at exit. Entries that exist in any form,
/// including dangling symlinks, are left untouched; nothing is followed.
pub(super) fn ensure_persistent_state(
    overlay: &ConfigurationOverlay,
    source: &Path,
) -> Result<(), TemporaryError> {
    if overlay.persistent_state.is_empty() {
        return Ok(());
    }
    let failed = |source| TemporaryError::MirrorOverlay {
        overlay_id: overlay.id.clone(),
        source,
    };
    create_private_dir_all(source).map_err(failed)?;
    for entry in &overlay.persistent_state {
        ensure_entry(source, entry).map_err(failed)?;
    }
    Ok(())
}

fn ensure_entry(source: &Path, entry: &PersistentStateEntry) -> std::io::Result<()> {
    let path = source.join(&entry.path);
    if let Some(parent) = path.parent() {
        create_private_dir_all(parent)?;
    }
    match fs::symlink_metadata(&path) {
        Ok(_) => return Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let created = match entry.kind {
        PersistentStateKind::Directory => create_private_dir(&path),
        PersistentStateKind::File => open_private_new(&path).map(drop),
    };
    match created {
        // A concurrent launch created the same entry first.
        Err(error) if error.kind() == ErrorKind::AlreadyExists => Ok(()),
        other => other,
    }
}
