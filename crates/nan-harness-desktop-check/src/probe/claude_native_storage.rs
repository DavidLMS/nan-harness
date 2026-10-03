//! Advisory metadata of fixed native known folders; never reads profile contents.
use super::ProbeSpec;
use nan_harness_core::DesktopHarnessKind;
use nan_harness_private_fs::{open_private_new, open_private_read};
use std::io::Read as _;
use std::path::PathBuf;

#[cfg(windows)]
use crate::native::{ClaudeStoragePresence, Native};

fn scope(spec: &ProbeSpec) -> Option<PathBuf> {
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
    let directory = PathBuf::from(std::env::var_os("NANH_DESKTOP_QUALIFICATION_FACTS")?);
    if !directory.is_absolute()
        || directory.ancestors().any(|path| {
            !std::fs::symlink_metadata(path)
                .is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
        })
    {
        return None;
    }
    directory.canonicalize().ok()
}

#[cfg(windows)]
fn observe() -> Option<ClaudeStoragePresence> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    Native::new().ok()?.claude_storage_until(deadline)
}

#[cfg(not(windows))]
fn observe() -> Option<serde_json::Value> {
    None
}

pub(super) fn capture(spec: &ProbeSpec) {
    if scope(spec).is_none() {
        return;
    }
    let before = observe();
    if let Ok(file) = open_private_new(&spec.workspace.join("claude-native-storage-before.private"))
    {
        let _ = serde_json::to_writer(file, &before);
    }
}

pub(super) fn record(spec: &ProbeSpec) {
    let Some(directory) = scope(spec) else { return };
    let path = spec.workspace.join("claude-native-storage-before.private");
    let Ok((file, _)) = open_private_read(&path) else {
        return;
    };
    #[cfg(windows)]
    let Ok(before) = serde_json::from_reader::<_, Option<ClaudeStoragePresence>>(file.take(1024))
    else {
        return;
    };
    #[cfg(not(windows))]
    let Ok(before) = serde_json::from_reader::<_, Option<serde_json::Value>>(file.take(1024))
    else {
        return;
    };
    let _ = std::fs::remove_file(path);
    let after = observe();
    #[cfg(windows)]
    let fresh = before.map(ClaudeStoragePresence::fresh);
    #[cfg(not(windows))]
    let fresh: Option<bool> = None;
    let facts = serde_json::json!({
        "schemaVersion": 1, "mechanism": "claude-native-storage", "diagnosticsOnly": true,
        "freshBefore": fresh, "observationValid": before.is_some() && after.is_some(),
        "before": before, "after": after
    });
    let mut nonce = [0; 8];
    if getrandom::fill(&mut nonce).is_err() {
        return;
    }
    let output = directory.join(format!(
        "claude-native-storage-{}.json",
        u64::from_le_bytes(nonce)
    ));
    if let Ok(file) = open_private_new(&output) {
        let _ = serde_json::to_writer(file, &facts);
    }
}
