//! Failure-only exact-file ownership evidence; never changes the persist result.
use serde::{Deserialize, Serialize};
#[cfg(windows)]
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(windows)]
const SOURCE_HASH: &str = "c36a40af9911c12933acca7124424a1169f6f7d139bcd4bbb862a2c3c57e2703";
#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Status {
    Observed,
    Unavailable,
    Deadline,
}
#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Stage {
    Request,
    Platform,
    Scope,
    Session,
    Register,
    List,
    Identity,
    Deadline,
    Query,
    Complete,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum DeleteAccess {
    Available,
    SharingDenied,
    AccessDenied,
    Missing,
    QueryFailed,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Reply {
    status: Status,
    stage: Stage,
    destination_present: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_delete_access: Option<DeleteAccess>,
    owner_count: Option<u8>,
    current_process_count: Option<u8>,
    other_process_count: Option<u8>,
}
impl Reply {
    fn unavailable(stage: Stage) -> Self {
        Self {
            status: if stage == Stage::Deadline {
                Status::Deadline
            } else {
                Status::Unavailable
            },
            stage,
            destination_present: None,
            source_delete_access: None,
            owner_count: None,
            current_process_count: None,
            other_process_count: None,
        }
    }
    fn valid(&self) -> bool {
        if self.status == Status::Observed {
            self.stage == Stage::Complete
                && self.destination_present.is_some()
                && matches!((self.owner_count,self.current_process_count,self.other_process_count),
                    (Some(total),Some(current),Some(other)) if total <= 64 && current <= 1 && u16::from(current)+u16::from(other)==u16::from(total))
        } else {
            self.stage != Stage::Complete
                && (self.status == Status::Deadline) == (self.stage == Stage::Deadline)
                && self.destination_present.is_none()
                && self.source_delete_access.is_none()
                && self.owner_count.is_none()
                && self.current_process_count.is_none()
                && self.other_process_count.is_none()
        }
    }
}
fn cutoff(value: &str, now: SystemTime, instant: Instant) -> Option<(u64, Instant)> {
    let epoch: u64 = value.parse().ok()?;
    let now_ms = u64::try_from(now.duration_since(UNIX_EPOCH).ok()?.as_millis()).ok()?;
    let remaining = epoch.checked_sub(now_ms)?;
    (remaining > 0 && remaining <= 45_000)
        .then(|| (epoch, instant + Duration::from_millis(remaining)))
}
// A longer qualification retry window must consume the existing parent budget.
#[cfg(windows)]
pub(super) fn configuration_retry_deadline() -> Instant {
    let now = Instant::now();
    retry_deadline(
        std::env::var("NANH_CLAUDE_PERSIST_CUTOFF_MS")
            .ok()
            .as_deref(),
        SystemTime::now(),
        now,
    )
}
#[cfg(any(windows, test))]
fn retry_deadline(value: Option<&str>, system: SystemTime, now: Instant) -> Instant {
    value
        .and_then(|value| cutoff(value, system, now))
        .map_or(now, |(_, parent)| parent.min(now + Duration::from_secs(2)))
}
#[cfg(windows)]
fn regular(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt as _;
    let mut root = PathBuf::new();
    for component in path.components() {
        root.push(component);
        if matches!(component, std::path::Component::Prefix(_)) {
            continue;
        }
        if !std::fs::symlink_metadata(&root).is_ok_and(|m| m.file_attributes() & 0x400 == 0) {
            return false;
        }
    }
    path.is_absolute() && path.is_file()
}
#[cfg(windows)]
fn held_public(path: &Path, expected: &str, max: u64) -> Option<std::fs::File> {
    use super::session::sha256;
    use std::io::Read as _;
    use std::os::windows::fs::OpenOptionsExt as _;
    if !regular(path) {
        return None;
    }
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(path)
        .ok()?;
    if file.metadata().ok()?.len() > max {
        return None;
    }
    let mut bytes = Vec::new();
    file.by_ref().take(max + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= max && sha256(&bytes) == expected).then_some(file)
}
#[cfg(windows)]
fn execute(python: &Path, script: &Path, request: Vec<u8>, deadline: Instant) -> Reply {
    use std::io::{Read as _, Write as _};
    use std::process::{Command, Stdio};
    if request.len() > 4096 || Instant::now() >= deadline {
        return Reply::unavailable(Stage::Deadline);
    }
    let Ok(mut child) = Command::new(python)
        .arg("-I")
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return Reply::unavailable(Stage::Query);
    };
    let Some(mut input) = child.stdin.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Reply::unavailable(Stage::Query);
    };
    let Some(output) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Reply::unavailable(Stage::Query);
    };
    let writer = std::thread::spawn(move || input.write_all(&request));
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        output.take(1025).read_to_end(&mut bytes).map(|_| bytes)
    });
    let status = loop {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
        std::thread::sleep(
            Duration::from_millis(2).min(deadline.saturating_duration_since(Instant::now())),
        );
    };
    let written = writer.join().ok().and_then(Result::ok);
    let bytes = reader.join().ok().and_then(Result::ok);
    if Instant::now() >= deadline {
        return Reply::unavailable(Stage::Deadline);
    }
    if !status.is_some_and(|s| s.success()) || written.is_none() {
        return Reply::unavailable(Stage::Query);
    }
    let Some(bytes) = bytes.filter(|b| b.len() <= 1024) else {
        return Reply::unavailable(Stage::Query);
    };
    serde_json::from_slice::<Reply>(&bytes)
        .ok()
        .filter(Reply::valid)
        .unwrap_or_else(|| Reply::unavailable(Stage::Query))
}
#[cfg(windows)]
fn observe_inner(temporary: &Path, destination: &Path) -> Reply {
    let Some((epoch, deadline)) = std::env::var("NANH_CLAUDE_PERSIST_CUTOFF_MS")
        .ok()
        .and_then(|value| cutoff(&value, SystemTime::now(), Instant::now()))
    else {
        return Reply::unavailable(Stage::Deadline);
    };
    let Some(workspace) = std::env::current_dir()
        .ok()
        .and_then(|p| p.canonicalize().ok())
    else {
        return Reply::unavailable(Stage::Scope);
    };
    let profile = workspace.join("profile");
    let expected_parent = profile
        .join("home")
        .join("AppData")
        .join("Roaming")
        .join("Claude");
    let Some(expected_parent) = expected_parent.canonicalize().ok() else {
        return Reply::unavailable(Stage::Scope);
    };
    if destination.file_name() != Some(std::ffi::OsStr::new("claude_desktop_config.json"))
        || destination
            .parent()
            .and_then(|p| p.canonicalize().ok())
            .as_ref()
            != Some(&expected_parent)
        || temporary
            .parent()
            .and_then(|p| p.canonicalize().ok())
            .as_ref()
            != Some(&expected_parent)
        || !super::qualification_config::private_directory(&profile)
    {
        return Reply::unavailable(Stage::Scope);
    }
    let destination = expected_parent.join("claude_desktop_config.json");
    let temporary_path = expected_parent.join(temporary.file_name().unwrap_or_default());
    let paths = (
        std::env::var_os("NANH_CLAUDE_PERSIST_PYTHON"),
        std::env::var_os("NANH_CLAUDE_PERSIST_SCRIPT"),
    );
    let (Some(python), Some(script)) = paths else {
        return Reply::unavailable(Stage::Scope);
    };
    let python = PathBuf::from(python);
    let script = PathBuf::from(script);
    let Some(python_digest) = std::env::var("NANH_CLAUDE_PERSIST_PYTHON_SHA256").ok() else {
        return Reply::unavailable(Stage::Scope);
    };
    let (Some(_python), Some(_script)) = (
        held_public(&python, &python_digest, 1_048_576),
        held_public(&script, SOURCE_HASH, 32768),
    ) else {
        return Reply::unavailable(Stage::Scope);
    };
    if Instant::now() >= deadline {
        return Reply::unavailable(Stage::Deadline);
    }
    let request = serde_json::json!({"workspace":workspace,"temporary":temporary_path,"destination":destination,"cliPid":std::process::id(),"deadlineMs":epoch});
    let Ok(mut request) = serde_json::to_vec(&request) else {
        return Reply::unavailable(Stage::Request);
    };
    request.push(b'\n');
    execute(&python, &script, request, deadline)
}
#[cfg(windows)]
pub(super) fn observe(temporary: &tempfile::NamedTempFile, destination: &Path) {
    observe_path(temporary.path(), destination);
}
#[cfg(windows)]
pub(super) fn observe_retained_path(temporary: &tempfile::TempPath, destination: &Path) {
    observe_path(temporary, destination);
}
#[cfg(windows)]
fn observe_path(temporary: &Path, destination: &Path) {
    if std::env::var("NANH_CLAUDE_PERSIST_OWNERS").as_deref() != Ok("1")
        || !super::qualification_prelaunch::enabled()
    {
        return;
    }
    let Some(directory) = super::qualification_prelaunch::facts_directory() else {
        return;
    };
    let reply = observe_inner(temporary, destination);
    let Ok(mut record) = serde_json::to_value(reply) else {
        return;
    };
    record["schemaVersion"] = 1.into();
    record["mechanism"] = "claude-config-persist-owners".into();
    record["diagnosticsOnly"] = true.into();
    if let Ok(mut file) = nan_harness_private_fs::open_private_new(
        &directory.join("claude-config-persist-owners.json"),
    ) {
        let _ = serde_json::to_writer(&mut file, &record);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cutoff_never_renews_expired_or_excessive_parent_budget() {
        let now = UNIX_EPOCH + Duration::from_secs(1);
        let instant = Instant::now();
        assert!(cutoff("1000", now, instant).is_none());
        assert!(cutoff("46001", now, instant).is_none());
        assert_eq!(
            cutoff("1200", now, instant)
                .unwrap()
                .1
                .duration_since(instant),
            Duration::from_millis(200)
        );
    }
    #[test]
    fn sharing_retry_is_clipped_to_original_parent_cutoff() {
        let system = UNIX_EPOCH + Duration::from_secs(1);
        let now = Instant::now();
        for value in [None, Some("bad"), Some("1000"), Some("46001")] {
            assert_eq!(retry_deadline(value, system, now), now);
        }
        assert_eq!(
            retry_deadline(Some("1200"), system, now),
            now + Duration::from_millis(200)
        );
        assert_eq!(
            retry_deadline(Some("5000"), system, now),
            now + Duration::from_secs(2)
        );
    }
    #[test]
    fn protocol_rejects_private_data_and_incomplete_owner_claims() {
        let good = r#"{"status":"observed","stage":"complete","destinationPresent":false,"ownerCount":2,"currentProcessCount":1,"otherProcessCount":1}"#;
        assert!(serde_json::from_str::<Reply>(good).unwrap().valid());
        for category in [
            "available",
            "sharing-denied",
            "access-denied",
            "missing",
            "query-failed",
        ] {
            let extended = good.replace('}', &format!(",\"sourceDeleteAccess\":\"{category}\"}}"));
            assert!(serde_json::from_str::<Reply>(&extended).unwrap().valid());
        }
        assert!(
            serde_json::from_str::<Reply>(
                &good.replace('}', ",\"sourceDeleteAccess\":\"PRIVATE\"}"),
            )
            .is_err()
        );
        for bad in [
            good.replace("\"ownerCount\":2", "\"ownerCount\":1"),
            good.replace("\"ownerCount\":2", "\"ownerCount\":65"),
            good.replace("\"complete\"", "\"query\""),
            good.replace('}', ",\"path\":\"PRIVATE\"}"),
            format!("{good} PRIVATE"),
        ] {
            assert!(!serde_json::from_str::<Reply>(&bad).is_ok_and(|r| r.valid()));
        }
        assert!(Reply::unavailable(Stage::Deadline).valid());
        assert!(Reply::unavailable(Stage::Query).valid());
    }
}
