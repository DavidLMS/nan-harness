//! Failure-only typed observations; they never change restoration or its verdict.
use super::ChatGptDesktopError;
use crate::commands::desktop::DesktopStateError;
use serde::Serialize;
use std::{io::Write as _, path::Path};

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Stage {
    Lock,
    Process,
    Ownership,
    Restore,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Cause {
    SessionBusy,
    AppRunning,
    ProcessInspection,
    UnsafePath,
    ProfileInvalid,
    ReceiptInvalid,
    BackupMissing,
    BackupMismatch,
    ConfigInvalid,
    OrphanedSession,
    Io,
    Persistence,
    Unclassified,
}

fn cause(error: &ChatGptDesktopError) -> Cause {
    match error {
        ChatGptDesktopError::State(DesktopStateError::AlreadyLocked) => Cause::SessionBusy,
        ChatGptDesktopError::AppAlreadyRunning => Cause::AppRunning,
        ChatGptDesktopError::InspectProcess(_) | ChatGptDesktopError::ProcessInspectionFailed => {
            Cause::ProcessInspection
        }
        ChatGptDesktopError::State(DesktopStateError::Symlink | DesktopStateError::InvalidPath) => {
            Cause::UnsafePath
        }
        ChatGptDesktopError::UnmanagedProfile
        | ChatGptDesktopError::InvalidMarker
        | ChatGptDesktopError::ParseMarker(_) => Cause::ProfileInvalid,
        ChatGptDesktopError::InvalidReceipt | ChatGptDesktopError::ParseReceipt(_) => {
            Cause::ReceiptInvalid
        }
        ChatGptDesktopError::MissingBackup => Cause::BackupMissing,
        ChatGptDesktopError::BackupHashMismatch => Cause::BackupMismatch,
        ChatGptDesktopError::MalformedConfig | ChatGptDesktopError::IncompatibleConfigSetting => {
            Cause::ConfigInvalid
        }
        ChatGptDesktopError::OrphanedSessionFiles => Cause::OrphanedSession,
        ChatGptDesktopError::InspectProfile(_)
        | ChatGptDesktopError::ReadState(_)
        | ChatGptDesktopError::WriteState(_)
        | ChatGptDesktopError::State(DesktopStateError::Io(_)) => Cause::Io,
        ChatGptDesktopError::Persistence(_) => Cause::Persistence,
        _ => Cause::Unclassified,
    }
}

fn private_directory(path: &Path) -> bool {
    if !path.is_absolute() || !std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir()) {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        path.metadata()
            .is_ok_and(|m| m.permissions().mode().trailing_zeros() >= 6)
    }
    #[cfg(not(unix))]
    {
        nan_harness_private_fs::restrict_path(
            path,
            nan_harness_private_fs::PrivatePathKind::Directory,
        )
        .is_ok()
    }
}

fn owned_profile(profile: &Path, workspace: &Path) -> bool {
    let expected = workspace.join("profile/nanh/chatgpt-desktop/profile");
    let Some(state) = profile.parent().and_then(Path::parent) else {
        return false;
    };
    let base = workspace.join("profile/nanh");
    // Inspect the containing private state, not the profile itself: an unsafe
    // profile symlink is one of the failures this diagnostic must distinguish.
    profile.file_name() == expected.file_name()
        && profile.parent().and_then(Path::file_name) == expected.parent().and_then(Path::file_name)
        && private_directory(&base)
        && state
            .canonicalize()
            .is_ok_and(|actual| base.canonicalize().is_ok_and(|expected| actual == expected))
}

pub(super) fn emit(profile: &Path, stage: Stage, error: &ChatGptDesktopError, debug: bool) {
    let expected_os = if cfg!(target_os = "macos") {
        "macOS"
    } else if cfg!(windows) {
        "Windows"
    } else {
        "Linux"
    };
    if debug
        || std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
        || std::env::var("RUNNER_OS").as_deref() != Ok(expected_os)
    {
        return;
    }
    let Some(directory) = std::env::var_os("NANH_DESKTOP_QUALIFICATION_FACTS") else {
        return;
    };
    let directory = std::path::PathBuf::from(directory);
    let Ok(workspace) = std::env::current_dir() else {
        return;
    };
    if !private_directory(&directory) || !owned_profile(profile, &workspace) {
        return;
    }
    let Ok(directory) = directory.canonicalize() else {
        return;
    };
    let _ = write_fact(&directory, stage, cause(error));
}

fn write_fact(directory: &Path, stage: Stage, cause: Cause) -> std::io::Result<()> {
    let mut nonce = [0_u8; 16];
    getrandom::fill(&mut nonce).map_err(std::io::Error::other)?;
    let value = serde_json::json!({"schemaVersion":1,"mechanism":"codex-restore", "diagnosticsOnly":true, "stage":stage,"cause":cause});
    let name = format!("codex-restore-{:032x}.json", u128::from_be_bytes(nonce));
    let mut file = nan_harness_private_fs::open_private_new(&directory.join(name))?;
    let bytes = serde_json::to_vec(&value)?;
    file.write_all(&bytes).and_then(|()| file.sync_all())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typed_causes_never_serialize_private_error_details() {
        let errors = [
            (
                ChatGptDesktopError::State(DesktopStateError::AlreadyLocked),
                Cause::SessionBusy,
            ),
            (ChatGptDesktopError::AppAlreadyRunning, Cause::AppRunning),
            (
                ChatGptDesktopError::State(DesktopStateError::Symlink),
                Cause::UnsafePath,
            ),
            (ChatGptDesktopError::InvalidMarker, Cause::ProfileInvalid),
            (ChatGptDesktopError::InvalidReceipt, Cause::ReceiptInvalid),
            (ChatGptDesktopError::MissingBackup, Cause::BackupMissing),
            (
                ChatGptDesktopError::BackupHashMismatch,
                Cause::BackupMismatch,
            ),
            (ChatGptDesktopError::MalformedConfig, Cause::ConfigInvalid),
            (
                ChatGptDesktopError::OrphanedSessionFiles,
                Cause::OrphanedSession,
            ),
            (
                ChatGptDesktopError::ReadState(std::io::Error::other("PRIVATE_SENTINEL")),
                Cause::Io,
            ),
            (
                ChatGptDesktopError::InspectProcess(std::io::Error::other("PRIVATE_SENTINEL")),
                Cause::ProcessInspection,
            ),
        ];
        let directory = tempfile::tempdir().unwrap();
        for (error, expected) in errors {
            assert_eq!(cause(&error), expected);
            write_fact(directory.path(), Stage::Restore, cause(&error)).unwrap();
        }
        for path in std::fs::read_dir(directory.path()).unwrap() {
            let bytes = std::fs::read(path.unwrap().path()).unwrap();
            assert!(!String::from_utf8_lossy(&bytes).contains("PRIVATE_SENTINEL"));
            let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(value.as_object().unwrap().len(), 5);
            assert_eq!(value["diagnosticsOnly"], true);
        }
    }
    #[test]
    fn owned_private_profile_relation_accepts_only_the_disposable_state() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = directory.path();
        let profile = workspace.join("profile/nanh/chatgpt-desktop/profile");
        nan_harness_private_fs::create_private_dir_all(profile.parent().unwrap()).unwrap();
        assert!(owned_profile(&profile, workspace));
        assert!(!owned_profile(
            &workspace.join("profile/nanh/other/profile"),
            workspace
        ));
        let foreign = tempfile::tempdir().unwrap();
        assert!(!owned_profile(&profile, foreign.path()));
    }
    #[test]
    fn foreign_profile_is_rejected_and_diagnostic_write_failure_is_separate() {
        let directory = tempfile::tempdir().unwrap();
        assert!(!owned_profile(
            Path::new("/foreign/profile"),
            directory.path()
        ));
        assert!(
            write_fact(
                &directory.path().join("absent"),
                Stage::Lock,
                Cause::SessionBusy
            )
            .is_err()
        );
    }
}
