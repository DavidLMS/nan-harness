use super::restoration::{discard_unapplied_state, restore_session};
use crate::commands::desktop::{create_private_directory, write_private_atomic};
use crate::commands::zed_desktop::ZedDesktopError;
use crate::commands::zed_desktop::documents::{backup_file_name, patch_settings, read_optional};
use crate::commands::zed_desktop::paths::ZedPaths;
use crate::commands::zed_desktop::process::SystemZedProcess;
use nan_harness_core::CodingModelProfile;

pub(super) fn begin_session(
    paths: &ZedPaths,
    process: &SystemZedProcess,
    gateway_url: &str,
    models: &[CodingModelProfile],
    selected_model: &str,
) -> Result<(), ZedDesktopError> {
    begin_session_with_check(paths, gateway_url, models, selected_model, || {
        process.is_running()
    })
}

pub(in crate::commands::zed_desktop) fn begin_session_with_check(
    paths: &ZedPaths,
    gateway_url: &str,
    models: &[CodingModelProfile],
    selected_model: &str,
    process_is_running: impl FnOnce() -> Result<bool, ZedDesktopError>,
) -> Result<(), ZedDesktopError> {
    super::restoration::ensure_no_pending_session(paths)?;
    let original = read_optional(&paths.settings)?;
    let patched = patch_settings(original.as_deref(), gateway_url, models, selected_model)?;
    create_private_directory(&paths.backup_directory)?;
    if let Some(original) = original.as_deref() {
        write_private_atomic(&paths.backup_directory.join(backup_file_name()), original)?;
    }
    let receipt = super::receipt::from_prepared_settings(original.as_deref(), &patched);
    super::receipt::write(&paths.session_receipt, &receipt)?;

    let process_running = process_is_running();
    let current = read_optional(&paths.settings);
    match (process_running, current) {
        (Ok(false), Ok(current)) if same_snapshot(current.as_deref(), original.as_deref()) => {}
        (Ok(true), _) => {
            discard_unapplied_state(paths)?;
            return Err(ZedDesktopError::AlreadyRunning);
        }
        (Err(error), _) | (_, Err(error)) => {
            discard_unapplied_state(paths)?;
            return Err(error);
        }
        (Ok(false), Ok(_)) => {
            discard_unapplied_state(paths)?;
            return Err(ZedDesktopError::SettingsChangedBeforeWrite);
        }
    }

    if let Err(error) = write_private_atomic(&paths.settings, &patched.contents) {
        let error = ZedDesktopError::State(error);
        let _ = restore_session(paths);
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
pub(in crate::commands::zed_desktop) fn begin_session_for_test(
    paths: &ZedPaths,
    gateway_url: &str,
    models: &[CodingModelProfile],
    selected_model: &str,
    process_running: bool,
) -> Result<(), ZedDesktopError> {
    begin_session_with_check(paths, gateway_url, models, selected_model, || {
        Ok(process_running)
    })
}

fn same_snapshot(left: Option<&[u8]>, right: Option<&[u8]>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            crate::commands::zed_desktop::documents::sha256(left)
                == crate::commands::zed_desktop::documents::sha256(right)
        }
        _ => false,
    }
}
