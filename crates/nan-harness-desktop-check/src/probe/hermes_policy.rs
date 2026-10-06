//! The disposable Hermes qualification profile disables only automatic recovery.

use crate::report::Reason;
use nan_harness_private_fs::{open_private_new, open_private_read};
use serde::Deserialize;
use std::{
    io::{Read as _, Write as _},
    path::Path,
};

const LIMIT: u64 = 65_536;
const POLICY: &str = "\nagent:\n  auto_recovery_cycles: 0\n";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Receipt {
    schema_version: u8,
    owner_id: String,
    profile_name: String,
    gateway_port: Option<u16>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Marker {
    schema_version: u8,
    owner_id: String,
}

fn read(path: &Path) -> Result<Vec<u8>, Reason> {
    if !std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file()) {
        return Err(Reason::IsolationUnavailable);
    }
    let (file, _) = open_private_read(path).map_err(|_| Reason::IsolationUnavailable)?;
    let mut bytes = Vec::new();
    file.take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Reason::IsolationUnavailable)?;
    if bytes.len() as u64 > LIMIT {
        return Err(Reason::IsolationUnavailable);
    }
    Ok(bytes)
}

pub(super) fn verify_owner(workspace: &Path) -> Result<(), Reason> {
    if !workspace.is_absolute()
        || !std::fs::symlink_metadata(workspace).is_ok_and(|metadata| metadata.is_dir())
    {
        return Err(Reason::IsolationUnavailable);
    }
    for relative in [
        "profile",
        "profile/nanh",
        "profile/nanh/hermes-desktop",
        "profile/hermes",
        "profile/hermes/profiles",
        "profile/hermes/profiles/nan",
    ] {
        if !std::fs::symlink_metadata(workspace.join(relative))
            .is_ok_and(|metadata| metadata.is_dir())
        {
            return Err(Reason::IsolationUnavailable);
        }
    }
    let receipt: Receipt = serde_json::from_slice(&read(
        &workspace.join("profile/nanh/hermes-desktop/ownership.json"),
    )?)
    .map_err(|_| Reason::IsolationUnavailable)?;
    let marker: Marker = serde_json::from_slice(&read(
        &workspace.join("profile/hermes/profiles/nan/.nan-harness-owner.json"),
    )?)
    .map_err(|_| Reason::IsolationUnavailable)?;
    if receipt.schema_version != 1
        || marker.schema_version != 1
        || receipt.profile_name != "nan"
        || receipt.owner_id.is_empty()
        || receipt.owner_id.len() > 128
        || receipt.owner_id != marker.owner_id
        || receipt.gateway_port == Some(0)
    {
        return Err(Reason::IsolationUnavailable);
    }
    Ok(())
}

fn fresh_config(bytes: &[u8]) -> Result<(), Reason> {
    let text = std::str::from_utf8(bytes).map_err(|_| Reason::IsolationUnavailable)?;
    let mut model = false;
    let mut providers = false;
    // nANH generated this fresh owned profile with simple top-level mappings.
    // Refuse an existing agent section or other YAML forms rather than parsing
    // arbitrary user configuration or accidentally creating duplicate keys.
    for line in text
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
    {
        if line.starts_with(char::is_whitespace) {
            continue;
        }
        let (key, _) = line.split_once(':').ok_or(Reason::IsolationUnavailable)?;
        let key = key.trim().trim_matches(['\'', '"']);
        if key == "agent"
            || key.is_empty()
            || !key
                .chars()
                .all(|character| character.is_ascii_alphabetic() || matches!(character, '_' | '-'))
        {
            return Err(Reason::IsolationUnavailable);
        }
        model |= key == "model";
        providers |= key == "providers";
    }
    if !model || !providers {
        return Err(Reason::IsolationUnavailable);
    }
    Ok(())
}

fn replace(path: &Path, before: &[u8], after: &[u8]) -> Result<(), Reason> {
    let parent = path.parent().ok_or(Reason::IsolationUnavailable)?;
    // Create with the same private native handle rights as managed CLI writes.
    // A standard Windows tempfile handle need not grant WRITE_DAC, so trying
    // to harden it afterwards can fail even in a correctly owned directory.
    let mut temporary = tempfile::Builder::new()
        .make_in(parent, open_private_new)
        .map_err(|_| Reason::IsolationUnavailable)?;
    temporary
        .write_all(after)
        .map_err(|_| Reason::IsolationUnavailable)?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|_| Reason::IsolationUnavailable)?;
    if read(path)? != before {
        return Err(Reason::IsolationUnavailable);
    }
    temporary
        .persist(path)
        .map_err(|_| Reason::IsolationUnavailable)?;
    Ok(())
}

pub(super) fn prepare(workspace: &Path, facts: &Path) -> Result<(), Reason> {
    let mut stage = "ownership";
    let outcome = prepare_owned(workspace, facts, &mut stage);
    if outcome.is_err() && facts.is_absolute() && facts.is_dir() && !facts.is_symlink() {
        let mut nonce = [0_u8; 8];
        if getrandom::fill(&mut nonce).is_ok() {
            let diagnostic = serde_json::json!({"schemaVersion":1,
                "mechanism":"hermes-policy-preparation", "diagnosticsOnly":true, "stage":stage});
            if let Ok(mut file) = open_private_new(&facts.join(format!(
                "hermes-policy-failure-{}.json",
                u64::from_le_bytes(nonce)
            ))) {
                let _ = serde_json::to_writer(&mut file, &diagnostic);
            }
        }
    }
    outcome
}

fn prepare_owned(workspace: &Path, facts: &Path, stage: &mut &'static str) -> Result<(), Reason> {
    verify_owner(workspace)?;
    *stage = "facts-directory";
    if !facts.is_absolute()
        || !std::fs::symlink_metadata(facts).is_ok_and(|metadata| metadata.is_dir())
    {
        return Err(Reason::IsolationUnavailable);
    }
    let path = workspace.join("profile/hermes/profiles/nan/config.yaml");
    *stage = "config-read";
    let before = read(&path)?;
    *stage = "config-shape";
    fresh_config(&before)?;
    let mut after = before.clone();
    after.extend_from_slice(POLICY.as_bytes());
    if after.len() as u64 > LIMIT {
        return Err(Reason::IsolationUnavailable);
    }
    *stage = "ownership-recheck";
    verify_owner(workspace)?;
    *stage = "config-replace";
    replace(&path, &before, &after)?;
    *stage = "policy-receipt";
    let policy = serde_json::json!({"schemaVersion": 1, "mechanism": "hermes-retry-policy",
        "policy": "explicit-ui-retry", "autoRecoveryCycles": 0, "apiMaxRetries": 3,
        "configBeforeSha256": crate::report::digest(&before), "configAfterSha256": crate::report::digest(&after)});
    let mut nonce = [0_u8; 8];
    getrandom::fill(&mut nonce).map_err(|_| Reason::IsolationUnavailable)?;
    let destination = facts.join(format!(
        "hermes-retry-policy-{}.json",
        u64::from_le_bytes(nonce)
    ));
    let mut file = open_private_new(&destination).map_err(|_| Reason::IsolationUnavailable)?;
    serde_json::to_writer(&mut file, &policy).map_err(|_| Reason::IsolationUnavailable)?;
    file.write_all(b"\n")
        .map_err(|_| Reason::IsolationUnavailable)
}

#[cfg(test)]
mod tests {
    use super::*;
    const CONFIG: &[u8] = b"model:\n  default: qwen3.6\nproviders:\n  nan:\n    base_url: http://127.0.0.1/synthetic\n";
    fn fixture() -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        for path in [
            "profile/nanh/hermes-desktop",
            "profile/hermes/profiles/nan",
            "facts",
        ] {
            nan_harness_private_fs::create_private_dir_all(&directory.path().join(path)).unwrap();
        }
        std::fs::write(directory.path().join("profile/nanh/hermes-desktop/ownership.json"), br#"{"schemaVersion":1,"ownerId":"synthetic-owner","profileName":"nan","gatewayPort":1234}"#).unwrap();
        std::fs::write(
            directory
                .path()
                .join("profile/hermes/profiles/nan/.nan-harness-owner.json"),
            br#"{"schemaVersion":1,"ownerId":"synthetic-owner"}"#,
        )
        .unwrap();
        std::fs::write(
            directory
                .path()
                .join("profile/hermes/profiles/nan/config.yaml"),
            CONFIG,
        )
        .unwrap();
        directory
    }
    #[test]
    fn policy_preserves_generated_config_and_records_only_closed_identity() {
        let directory = fixture();
        prepare(directory.path(), &directory.path().join("facts")).unwrap();
        let after = read(
            &directory
                .path()
                .join("profile/hermes/profiles/nan/config.yaml"),
        )
        .unwrap();
        assert!(after.starts_with(CONFIG));
        assert!(after.ends_with(POLICY.as_bytes()));
        let fact_path = std::fs::read_dir(directory.path().join("facts"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let facts = read(&fact_path).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&facts).unwrap();
        assert_eq!(value["apiMaxRetries"], 3);
        assert_eq!(value["autoRecoveryCycles"], 0);
        assert_eq!(value["configBeforeSha256"], crate::report::digest(CONFIG));
        assert!(!String::from_utf8(facts).unwrap().contains("synthetic"));
        assert!(prepare(directory.path(), &directory.path().join("facts")).is_err());
    }
    #[test]
    fn ownership_and_existing_agent_conflicts_leave_config_untouched() {
        for (relative, bytes) in [
            (
                "profile/hermes/profiles/nan/.nan-harness-owner.json",
                br#"{"schemaVersion":1,"ownerId":"foreign"}"#.as_slice(),
            ),
            (
                "profile/hermes/profiles/nan/config.yaml",
                b"model:\nproviders:\n\"agent\": {}\n".as_slice(),
            ),
        ] {
            let directory = fixture();
            let path = directory.path().join(relative);
            std::fs::write(&path, bytes).unwrap();
            let config = directory
                .path()
                .join("profile/hermes/profiles/nan/config.yaml");
            let before = read(&config).unwrap();
            assert!(prepare(directory.path(), &directory.path().join("facts")).is_err());
            assert_eq!(read(&config).unwrap(), before);
            let entries = std::fs::read_dir(directory.path().join("facts"))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            assert_eq!(entries.len(), 1);
            let bytes = read(&entries[0].path()).unwrap();
            let diagnostic: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(diagnostic["mechanism"], "hermes-policy-preparation");
            assert_eq!(diagnostic["diagnosticsOnly"], true);
            assert_eq!(
                diagnostic["stage"],
                if relative.ends_with("config.yaml") {
                    "config-shape"
                } else {
                    "ownership"
                }
            );
            assert!(!String::from_utf8(bytes).unwrap().contains("foreign"));
        }
    }
    #[test]
    fn bounds_and_changed_config_fail_closed() {
        assert!(fresh_config(b"model:\nproviders:\nagent:\n").is_err());
        assert!(fresh_config(b"---\nmodel:\nproviders:\n").is_err());
        assert!(fresh_config(b"model:\nproviders:\n\xff").is_err());
        let directory = fixture();
        let path = directory
            .path()
            .join("profile/hermes/profiles/nan/config.yaml");
        assert!(replace(&path, b"different", b"replacement").is_err());
        assert_eq!(read(&path).unwrap(), CONFIG);
        std::fs::write(&path, vec![b'x'; usize::try_from(LIMIT).unwrap() + 1]).unwrap();
        assert!(prepare(directory.path(), &directory.path().join("facts")).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn symlinked_config_is_rejected_without_touching_target() {
        let directory = fixture();
        let path = directory
            .path()
            .join("profile/hermes/profiles/nan/config.yaml");
        let target = directory.path().join("foreign-config");
        std::fs::write(&target, CONFIG).unwrap();
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(prepare(directory.path(), &directory.path().join("facts")).is_err());
        assert_eq!(read(&target).unwrap(), CONFIG);
    }
}
