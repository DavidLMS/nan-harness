//! Read-only process absence supplements window absence before restoration.
use crate::report::Reason;
#[cfg(windows)]
use nan_harness_core::DesktopHarnessKind;

#[cfg(windows)]
pub(super) fn ensure_absent(kind: DesktopHarnessKind) -> Result<(), Reason> {
    if kind == DesktopHarnessKind::ChatGpt {
        use std::time::{Duration, Instant};
        return wait_absent(
            |deadline| inspect(deadline, b"ChatGPT.exe"),
            Instant::now,
            std::thread::sleep,
            Instant::now() + Duration::from_secs(2),
        );
    }
    Ok(())
}

#[cfg(windows)]
pub(super) fn inspect_absent(
    kind: DesktopHarnessKind,
    deadline: std::time::Instant,
) -> Result<(), Reason> {
    let image: &[u8] = match kind {
        DesktopHarnessKind::ChatGpt => b"ChatGPT.exe",
        DesktopHarnessKind::Claude => b"Claude.exe",
        _ => return Ok(()),
    };
    if inspect(deadline, image)? {
        Err(Reason::AlreadyRunning)
    } else {
        Ok(())
    }
}

#[cfg(any(windows, test))]
pub(super) fn mark_rejection_observation(
    observed: &mut bool,
    ax_presence: bool,
    now: std::time::Instant,
    deadline: std::time::Instant,
) -> bool {
    if *observed || !ax_presence || now >= deadline {
        return false;
    }
    *observed = true;
    true
}

#[cfg(any(windows, test))]
#[derive(Clone, Copy, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
enum PostStopState {
    Present,
    Absent,
    QueryFailed,
}

#[cfg(any(windows, test))]
fn post_stop_facts(state: PostStopState) -> serde_json::Value {
    serde_json::json!({"schemaVersion":1,"mechanism":"windows-post-stop-process",
        "diagnosticsOnly":true,"phase":"first-accessibility-rejection","state":state})
}

#[cfg(windows)]
pub(super) fn observe_after_accessibility_rejection(deadline: std::time::Instant) {
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
        || std::env::var("RUNNER_OS").as_deref() != Ok("Windows")
        || std::time::Instant::now() >= deadline
    {
        return;
    }
    let state = match inspect(deadline, b"Claude.exe") {
        Ok(true) => PostStopState::Present,
        Ok(false) => PostStopState::Absent,
        Err(_) => PostStopState::QueryFailed,
    };
    save_facts(post_stop_facts(state));
}

#[cfg(any(windows, test))]
fn wait_absent(
    mut query: impl FnMut(std::time::Instant) -> Result<bool, Reason>,
    mut now: impl FnMut() -> std::time::Instant,
    mut pause: impl FnMut(std::time::Duration),
    deadline: std::time::Instant,
) -> Result<(), Reason> {
    loop {
        if now() >= deadline {
            return Err(Reason::CleanupFailed);
        }
        let present = query(deadline)?;
        if now() >= deadline {
            return Err(Reason::CleanupFailed);
        }
        if !present {
            return Ok(());
        }
        pause(std::time::Duration::from_millis(50).min(deadline.saturating_duration_since(now())));
    }
}

#[cfg(any(windows, test))]
fn csv_presence(bytes: &[u8], inspector_pid: u32, image: &[u8]) -> Result<bool, Reason> {
    if bytes.is_empty() || bytes.len() > 65536 {
        return Err(Reason::DesktopUnavailable);
    }
    let mut found = false;
    let mut inspector = false;
    for line in bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let mut fields = Vec::new();
        let mut position = 0;
        while position < line.len() {
            if line[position] != b'"' {
                return Err(Reason::DesktopUnavailable);
            }
            position += 1;
            let start = position;
            while position < line.len() && line[position] != b'"' {
                position += 1;
            }
            if position == line.len() {
                return Err(Reason::DesktopUnavailable);
            }
            fields.push(&line[start..position]);
            position += 1;
            if position < line.len() {
                if line[position] != b',' || position + 1 == line.len() {
                    return Err(Reason::DesktopUnavailable);
                }
                position += 1;
            }
        }
        if fields.len() != 5 {
            return Err(Reason::DesktopUnavailable);
        }
        found |= fields[0].eq_ignore_ascii_case(image);
        inspector |= fields[0].eq_ignore_ascii_case(b"tasklist.exe")
            && fields[1] == inspector_pid.to_string().as_bytes();
    }
    if !inspector {
        return Err(Reason::DesktopUnavailable);
    }
    Ok(found)
}

#[cfg(any(windows, test))]
#[derive(Clone, Copy, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
enum InspectionStage {
    Deadline,
    SystemRoot,
    PrivateOutput,
    Spawn,
    Exit,
    Read,
    Schema,
    Oversize,
}

#[cfg(any(windows, test))]
fn failure_facts(image: &[u8], stage: InspectionStage) -> Option<serde_json::Value> {
    let app = match image {
        b"Claude.exe" => "claude-desktop",
        b"ChatGPT.exe" => "chatgpt-desktop",
        _ => return None,
    };
    Some(
        serde_json::json!({"schemaVersion":1,"mechanism":"windows-process-absence",
        "diagnosticsOnly":true,"app":app,"stage":stage}),
    )
}

#[cfg(windows)]
fn save_facts(value: serde_json::Value) {
    use std::io::Write as _;
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
        || std::env::var("RUNNER_OS").as_deref() != Ok("Windows")
    {
        return;
    }
    let Some(directory) = std::env::var_os("NANH_DESKTOP_QUALIFICATION_FACTS") else {
        return;
    };
    let directory = std::path::PathBuf::from(directory);
    if !directory.is_absolute()
        || !std::fs::symlink_metadata(&directory).is_ok_and(|metadata| metadata.is_dir())
    {
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
        "windows-process-absence-{}.json",
        u64::from_le_bytes(nonce)
    ));
    if let Ok(mut file) = nan_harness_private_fs::open_private_new(&path) {
        if serde_json::to_writer(&mut file, &value).is_ok() {
            let _ = file.flush();
        }
    }
}

#[cfg(windows)]
fn save_failure(image: &[u8], stage: InspectionStage) {
    if let Some(value) = failure_facts(image, stage) {
        save_facts(value);
    }
}

#[cfg(windows)]
fn inspect(deadline: std::time::Instant, image: &[u8]) -> Result<bool, Reason> {
    inspect_inner(deadline, image).map_err(|stage| {
        save_failure(image, stage);
        Reason::DesktopUnavailable
    })
}

#[cfg(windows)]
fn inspect_inner(deadline: std::time::Instant, image: &[u8]) -> Result<bool, InspectionStage> {
    use std::io::{Read as _, Seek as _};
    use std::os::windows::process::CommandExt as _;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    let unavailable = || InspectionStage::SystemRoot;
    let root = std::path::PathBuf::from(std::env::var_os("SystemRoot").ok_or_else(unavailable)?);
    let canonical_root = root.canonicalize().map_err(|_| unavailable())?;
    let path = root.join("System32/tasklist.exe");
    let executable = path.canonicalize().map_err(|_| unavailable())?;
    if !root.is_absolute()
        || !std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file())
        || !executable.starts_with(&canonical_root)
    {
        return Err(unavailable());
    }
    let mut output = tempfile::tempfile().map_err(|_| InspectionStage::PrivateOutput)?;
    if Instant::now() >= deadline {
        return Err(InspectionStage::Deadline);
    }
    let mut child = Command::new(&executable)
        .args(["/FO", "CSV", "/NH"])
        .env_clear()
        .env("SystemRoot", &root)
        .creation_flags(0x0800_0000)
        .stdin(Stdio::null())
        .stdout(
            output
                .try_clone()
                .map_err(|_| InspectionStage::PrivateOutput)?,
        )
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| InspectionStage::Spawn)?;
    let outcome = loop {
        if Instant::now() >= deadline {
            break Err(InspectionStage::Deadline);
        }
        match output.metadata() {
            Err(_) => break Err(InspectionStage::PrivateOutput),
            Ok(metadata) if metadata.len() > 65536 => break Err(InspectionStage::Oversize),
            Ok(_) => {}
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break Ok(()),
            Ok(Some(_)) | Err(_) => break Err(InspectionStage::Exit),
            Ok(None) => std::thread::sleep(
                Duration::from_millis(10).min(deadline.saturating_duration_since(Instant::now())),
            ),
        }
    };
    if outcome.is_err() {
        // Only this inspector Child is terminated; app termination belongs to
        // the pre-existing owned JobObject cleanup, never an enumerated PID.
        let _ = child.kill();
        let _ = child.wait();
    }
    outcome?;
    output.rewind().map_err(|_| InspectionStage::Read)?;
    let mut bytes = zeroize::Zeroizing::new(Vec::new());
    output
        .take(65537)
        .read_to_end(&mut bytes)
        .map_err(|_| InspectionStage::Read)?;
    if bytes.len() > 65536 {
        return Err(InspectionStage::Oversize);
    }
    if Instant::now() >= deadline {
        return Err(InspectionStage::Deadline);
    }
    csv_presence(&bytes, child.id(), image).map_err(|_| InspectionStage::Schema)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::Cell,
        time::{Duration, Instant},
    };
    const INSPECTOR: &[u8] = b"\"tasklist.exe\",\"123\",\"Console\",\"1\",\"1,000 K\"\r\n";
    #[test]
    fn independent_observation_only_claims_first_ax_presence_with_original_budget() {
        let now = Instant::now();
        let deadline = now + Duration::from_secs(5);
        let mut observed = false;
        assert!(!mark_rejection_observation(
            &mut observed,
            false,
            now,
            deadline
        ));
        assert!(!observed);
        assert!(!mark_rejection_observation(
            &mut observed,
            true,
            deadline,
            deadline
        ));
        assert!(!observed);
        assert!(mark_rejection_observation(
            &mut observed,
            true,
            now,
            deadline
        ));
        assert!(!mark_rejection_observation(
            &mut observed,
            true,
            now,
            deadline
        ));
        for (state, expected) in [
            (PostStopState::Present, "present"),
            (PostStopState::Absent, "absent"),
            (PostStopState::QueryFailed, "query-failed"),
        ] {
            let value = post_stop_facts(state);
            assert_eq!(value["state"], expected);
            assert_eq!(value["phase"], "first-accessibility-rejection");
            assert_eq!(value.as_object().unwrap().len(), 5);
            assert!(
                !serde_json::to_string(&value)
                    .unwrap()
                    .contains("PRIVATE_SENTINEL")
            );
        }
    }

    #[test]
    fn inspection_failures_serialize_only_closed_stages_and_selected_app() {
        let stages = [
            InspectionStage::Deadline,
            InspectionStage::SystemRoot,
            InspectionStage::PrivateOutput,
            InspectionStage::Spawn,
            InspectionStage::Exit,
            InspectionStage::Read,
            InspectionStage::Schema,
            InspectionStage::Oversize,
        ];
        let expected = [
            "deadline",
            "system-root",
            "private-output",
            "spawn",
            "exit",
            "read",
            "schema",
            "oversize",
        ];
        for (stage, expected) in stages.into_iter().zip(expected) {
            let value = failure_facts(b"Claude.exe", stage).unwrap();
            assert_eq!(value["stage"], expected);
            assert_eq!(value["app"], "claude-desktop");
            assert_eq!(value.as_object().unwrap().len(), 5);
            let encoded = serde_json::to_string(&value).unwrap();
            assert!(!encoded.contains("PRIVATE_SENTINEL"));
        }
        assert!(failure_facts(b"PRIVATE_SENTINEL.exe", InspectionStage::Exit).is_none());
        assert_eq!(
            failure_facts(b"ChatGPT.exe", InspectionStage::Spawn).unwrap()["app"],
            "chatgpt-desktop"
        );
    }

    #[test]
    fn exact_image_and_owned_inspector_are_required() {
        assert_eq!(csv_presence(INSPECTOR, 123, b"ChatGPT.exe"), Ok(false));
        let mut rows = INSPECTOR.to_vec();
        rows.extend_from_slice(b"\"cHaTgPt.exe\",\"456\",\"Console\",\"1\",\"100 K\"\r\n");
        assert_eq!(csv_presence(&rows, 123, b"ChatGPT.exe"), Ok(true));
        assert!(csv_presence(&rows, 124, b"ChatGPT.exe").is_err());
        let mut foreign = INSPECTOR.to_vec();
        foreign.extend_from_slice(
            b"\"ChatGPTHelper.exe\",\"456\",\"ChatGPT.exe\",\"1\",\"100 K\"\r\n",
        );
        assert_eq!(csv_presence(&foreign, 123, b"ChatGPT.exe"), Ok(false));
        for row in [
            b"".as_slice(),
            b"INFO: No tasks are running",
            b"INFORMATION: Keine Aufgaben",
            b"\"ChatGPT.exe\",\"456\"",
            b"\"broken",
        ] {
            assert!(csv_presence(row, 123, b"ChatGPT.exe").is_err());
        }
        assert!(csv_presence(&vec![b'x'; 65537], 123, b"ChatGPT.exe").is_err());
    }
    #[test]
    fn claude_exact_image_is_case_insensitive_and_helpers_do_not_match() {
        let mut rows = INSPECTOR.to_vec();
        rows.extend_from_slice(b"\"cLaUdE.exe\",\"456\",\"Console\",\"1\",\"100 K\"\r\n");
        assert_eq!(csv_presence(&rows, 123, b"Claude.exe"), Ok(true));
        assert_eq!(csv_presence(&rows, 123, b"ChatGPT.exe"), Ok(false));
        let mut helpers = INSPECTOR.to_vec();
        helpers
            .extend_from_slice(b"\"ClaudeHelper.exe\",\"456\",\"Claude.exe\",\"1\",\"100 K\"\r\n");
        assert_eq!(csv_presence(&helpers, 123, b"Claude.exe"), Ok(false));
    }

    #[test]
    fn polling_uses_one_deadline_and_never_queries_after_it() {
        let start = Instant::now();
        let clock = Cell::new(start);
        let calls = Cell::new(0);
        let deadline = start + Duration::from_millis(100);
        let result = wait_absent(
            |bound| {
                assert_eq!(bound, deadline);
                calls.set(calls.get() + 1);
                Ok(true)
            },
            || clock.get(),
            |duration| clock.set(clock.get() + duration),
            deadline,
        );
        assert_eq!(result, Err(Reason::CleanupFailed));
        assert_eq!(calls.get(), 2);
    }
    #[test]
    fn late_absence_cannot_pass_or_reset_deadline() {
        let start = Instant::now();
        let clock = Cell::new(start);
        let deadline = start + Duration::from_millis(100);
        assert_eq!(
            wait_absent(
                |bound| {
                    assert_eq!(bound, deadline);
                    clock.set(deadline);
                    Ok(false)
                },
                || clock.get(),
                |_| panic!("no retry"),
                deadline
            ),
            Err(Reason::CleanupFailed)
        );
    }
    #[test]
    fn delayed_absence_succeeds_but_query_failure_never_does() {
        let start = Instant::now();
        let clock = Cell::new(start);
        let calls = Cell::new(0);
        assert_eq!(
            wait_absent(
                |_| {
                    calls.set(calls.get() + 1);
                    Ok(calls.get() < 2)
                },
                || clock.get(),
                |duration| clock.set(clock.get() + duration),
                start + Duration::from_secs(2)
            ),
            Ok(())
        );
        assert_eq!(calls.get(), 2);
        assert_eq!(
            wait_absent(
                |_| Err(Reason::DesktopUnavailable),
                || start,
                |_| panic!("no retry"),
                start + Duration::from_secs(2)
            ),
            Err(Reason::DesktopUnavailable)
        );
    }
}
