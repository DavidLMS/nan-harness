//! Metadata-only storage observations owned by the hosted Windows checker.
use super::ProbeSpec;
use nan_harness_core::DesktopHarnessKind;
use std::io::Read as _;
use std::path::{Path, PathBuf};

#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Presence {
    #[serde(flatten)]
    normal: Normal,
    #[serde(flatten)]
    third_party: ThirdParty,
}
#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Normal {
    claude_local_state: bool,
    claude_preferences: bool,
}
#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ThirdParty {
    third_party_local_state: bool,
    third_party_preferences: bool,
}

fn scope(spec: &ProbeSpec) -> Option<([PathBuf; 2], PathBuf)> {
    if !cfg!(windows)
        || spec.kind != DesktopHarnessKind::Claude
        || spec.session != crate::cli::SessionMode::GithubHosted
        || std::env::var("RUNNER_OS").as_deref() != Ok("Windows")
        || std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
        || std::env::var("NANH_CLAUDE_WINDOWS_PROFILE_POLICY").as_deref() != Ok("private-env")
        || std::env::var("NANH_DESKTOP_QUALIFICATION_MODE").as_deref() != Ok("startup-baseline")
    {
        return None;
    }
    let workspace = spec.workspace.canonicalize().ok()?;
    let roots = [
        workspace
            .join("profile")
            .join("home")
            .join("AppData")
            .join("Roaming")
            .join("Claude"),
        workspace
            .join("profile")
            .join("home")
            .join("AppData")
            .join("Local")
            .join("Claude-3p"),
    ];
    for root in &roots {
        for parent in root.ancestors() {
            if std::fs::symlink_metadata(parent)
                .is_ok_and(|m| m.file_type().is_symlink() || !m.is_dir())
            {
                return None;
            }
        }
    }
    let directory = PathBuf::from(std::env::var_os("NANH_DESKTOP_QUALIFICATION_FACTS")?);
    if !directory.is_absolute() || !regular_directory(&directory) {
        return None;
    }
    if directory.ancestors().any(|p| !regular_directory(p)) {
        return None;
    }
    let directory = directory.canonicalize().ok()?;
    if !regular_directory(&directory) {
        return None;
    }
    Some((roots, directory))
}
fn regular_directory(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
}
fn present(root: &Path, parts: &[&str]) -> Option<bool> {
    if !regular_directory(root) || root.canonicalize().ok().as_deref() != Some(root) {
        return None;
    }
    let mut path = root.to_path_buf();
    for (i, part) in parts.iter().enumerate() {
        path.push(part);
        match std::fs::symlink_metadata(&path) {
            Ok(m)
                if !m.file_type().is_symlink()
                    && if i + 1 == parts.len() {
                        m.is_file()
                    } else {
                        m.is_dir()
                    } => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Some(false),
            _ => return None,
        }
    }
    Some(true)
}
fn observe(roots: &[PathBuf; 2]) -> Option<Presence> {
    Some(Presence {
        normal: Normal {
            claude_local_state: present(&roots[0], &["Local State"])?,
            claude_preferences: present(&roots[0], &["Default", "Preferences"])?,
        },
        third_party: ThirdParty {
            third_party_local_state: present(&roots[1], &["Local State"])?,
            third_party_preferences: present(&roots[1], &["Default", "Preferences"])?,
        },
    })
}
fn command_directory_matches(
    command: &tokio::process::Command,
    key: &str,
    expected: &Path,
) -> bool {
    command.as_std().get_envs().any(|(name, value)| {
        name == key
            && value
                .and_then(|path| Path::new(path).canonicalize().ok())
                .as_deref()
                == Some(expected)
    })
}
#[derive(Clone, Copy, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
enum StorageStage {
    Captured,
    ScopeRejected,
    WorkspaceUnavailable,
    HomeUnavailable,
    EnvironmentUnbound,
    RootRejected,
    RootCreateFailed,
    SnapshotUnavailable,
    CheckpointWriteFailed,
    CheckpointReadFailed,
    CheckpointDecodeFailed,
    Recorded,
}

fn record_stage(spec: &ProbeSpec, phase: &'static str, stage: StorageStage) {
    if !cfg!(windows)
        || spec.kind != DesktopHarnessKind::Claude
        || spec.session != crate::cli::SessionMode::GithubHosted
        || std::env::var("RUNNER_OS").as_deref() != Ok("Windows")
        || std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
        || std::env::var("NANH_CLAUDE_WINDOWS_PROFILE_POLICY").as_deref() != Ok("private-env")
        || std::env::var("NANH_DESKTOP_QUALIFICATION_MODE").as_deref() != Ok("startup-baseline")
    {
        return;
    }
    let Some(directory) = std::env::var_os("NANH_DESKTOP_QUALIFICATION_FACTS").map(PathBuf::from)
    else {
        return;
    };
    if !directory.is_absolute() || directory.ancestors().any(|path| !regular_directory(path)) {
        return;
    }
    let Ok(directory) = directory.canonicalize() else {
        return;
    };
    let mut nonce = [0; 8];
    if getrandom::fill(&mut nonce).is_err() {
        return;
    }
    let path = directory.join(format!(
        "claude-private-storage-stage-{}.json",
        u64::from_le_bytes(nonce)
    ));
    let value = serde_json::json!({"schemaVersion":1,"mechanism":"claude-private-storage-stage",
        "diagnosticsOnly":true,"phase":phase,"stage":stage});
    if let Ok(file) = nan_harness_private_fs::open_private_new(&path) {
        let _ = serde_json::to_writer(file, &value);
    }
}

pub(super) fn capture(spec: &ProbeSpec, command: &tokio::process::Command) {
    let stage = capture_inner(spec, command)
        .err()
        .unwrap_or(StorageStage::Captured);
    record_stage(spec, "before-launch", stage);
}

fn capture_inner(spec: &ProbeSpec, command: &tokio::process::Command) -> Result<(), StorageStage> {
    let Some((roots, _)) = scope(spec) else {
        return Err(StorageStage::ScopeRejected);
    };
    let workspace = spec.workspace.canonicalize().ok();
    let Some(workspace) = workspace else {
        return Err(StorageStage::WorkspaceUnavailable);
    };
    let home = workspace.join("profile").join("home");
    if !regular_directory(&home) {
        return Err(StorageStage::HomeUnavailable);
    }
    for (key, expected) in [
        ("HOME", home.clone()),
        ("USERPROFILE", home.clone()),
        ("LOCALAPPDATA", home.join("AppData").join("Local")),
        ("APPDATA", home.join("AppData").join("Roaming")),
    ] {
        if !command_directory_matches(command, key, &expected) {
            return Err(StorageStage::EnvironmentUnbound);
        }
    }
    for root in &roots {
        let mut current = spec.workspace.canonicalize().unwrap_or_default();
        let Ok(relative) = root.strip_prefix(&current) else {
            return Err(StorageStage::RootRejected);
        };
        for part in relative.components() {
            current.push(part);
            if std::fs::symlink_metadata(&current)
                .is_ok_and(|m| m.file_type().is_symlink() || !m.is_dir())
            {
                return Err(StorageStage::RootRejected);
            }
        }
        if nan_harness_private_fs::create_private_dir_all(root).is_err() {
            return Err(StorageStage::RootCreateFailed);
        }
    }
    let Some(before) = observe(&roots) else {
        return Err(StorageStage::SnapshotUnavailable);
    };
    let path = spec.workspace.join("claude-storage-before.private");
    let file = nan_harness_private_fs::open_private_new(&path)
        .map_err(|_| StorageStage::CheckpointWriteFailed)?;
    serde_json::to_writer(file, &before).map_err(|_| StorageStage::CheckpointWriteFailed)
}
pub(super) fn record(spec: &ProbeSpec) {
    let Some((roots, directory)) = scope(spec) else {
        record_stage(spec, "after-stop", StorageStage::ScopeRejected);
        return;
    };
    let path = spec.workspace.join("claude-storage-before.private");
    let Ok((file, _)) = nan_harness_private_fs::open_private_read(&path) else {
        record_stage(spec, "after-stop", StorageStage::CheckpointReadFailed);
        return;
    };
    let Ok(before) = serde_json::from_reader::<_, Presence>(file.take(1024)) else {
        record_stage(spec, "after-stop", StorageStage::CheckpointDecodeFailed);
        return;
    };
    let _ = std::fs::remove_file(path);
    let fresh = !before.normal.claude_local_state
        && !before.normal.claude_preferences
        && !before.third_party.third_party_local_state
        && !before.third_party.third_party_preferences;
    let after = observe(&roots);
    record_stage(
        spec,
        "after-stop",
        if after.is_some() {
            StorageStage::Recorded
        } else {
            StorageStage::SnapshotUnavailable
        },
    );
    let facts = serde_json::json!({"schemaVersion":1,"mechanism":"claude-storage-use","diagnosticsOnly":true,"freshBefore":fresh,"observationValid":after.is_some(),"before":before,"after":after.unwrap_or_default()});
    let mut nonce = [0; 8];
    if getrandom::fill(&mut nonce).is_err() {
        return;
    }
    let output = directory.join(format!(
        "claude-storage-checker-{}.json",
        u64::from_le_bytes(nonce)
    ));
    if let Ok(file) = nan_harness_private_fs::open_private_new(&output) {
        let _ = serde_json::to_writer(file, &facts);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn environment_binding_compares_owned_directory_identity() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        std::fs::create_dir(&home).unwrap();
        let canonical = home.canonicalize().unwrap();
        let mut command = tokio::process::Command::new("unused-synthetic-program");
        command.env("HOME", home.join("../home"));
        assert!(command_directory_matches(&command, "HOME", &canonical));
        assert!(!command_directory_matches(&command, "HOME", temp.path()));
        command.env_remove("HOME");
        assert!(!command_directory_matches(&command, "HOME", &canonical));
    }
    #[test]
    fn fixed_presence_does_not_read_payload_and_missing_parent_is_absent() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let roots = [root.clone(), root];
        assert!(!observe(&roots).unwrap().normal.claude_preferences);
        std::fs::write(temp.path().join("Local State"), b"PRIVATE_SENTINEL").unwrap();
        let value = serde_json::to_string(&observe(&roots).unwrap()).unwrap();
        let checkpoint = temp.path().join("checkpoint.private");
        serde_json::to_writer(
            nan_harness_private_fs::open_private_new(&checkpoint).unwrap(),
            &observe(&roots).unwrap(),
        )
        .unwrap();
        let (file, _) = nan_harness_private_fs::open_private_read(&checkpoint).unwrap();
        assert!(serde_json::from_reader::<_, Presence>(file.take(1024)).is_ok());
        let mut forged: serde_json::Value = serde_json::from_str(&value).unwrap();
        forged["privatePayload"] = serde_json::json!("PRIVATE_SENTINEL");
        assert!(serde_json::from_value::<Presence>(forged).is_err());
        assert!(serde_json::from_str::<Presence>(&value).is_ok());
        assert!(value.contains("true"));
        assert!(!value.contains("PRIVATE_SENTINEL"));
        std::fs::create_dir(temp.path().join("Default")).unwrap();
        std::fs::create_dir(temp.path().join("Default/Preferences")).unwrap();
        assert!(observe(&roots).is_none());
    }
    #[cfg(unix)]
    #[test]
    fn replaced_root_or_parent_cannot_observe_foreign_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let foreign = root.join("foreign");
        std::fs::create_dir(&foreign).unwrap();
        std::fs::write(foreign.join("Local State"), b"PRIVATE_SENTINEL").unwrap();
        let replacement = root.join("replacement");
        std::os::unix::fs::symlink(&foreign, &replacement).unwrap();
        assert!(present(&replacement, &["Local State"]).is_none());
        let child = foreign.join("child");
        std::fs::create_dir(&child).unwrap();
        assert!(present(&replacement.join("child"), &["Local State"]).is_none());
    }
}
