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
#[derive(Debug, Clone, Copy, serde::Serialize)]
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
#[derive(Default)]
struct ProcessScan {
    present: bool,
    own_seen: bool,
    count: usize,
}

#[cfg(any(windows, test))]
impl ProcessScan {
    fn observe(
        &mut self,
        pid: u32,
        name: &[u16],
        own: u32,
        image: &[u8],
    ) -> Result<(), InspectionStage> {
        self.count += 1;
        if self.count > 65536 {
            return Err(InspectionStage::Oversize);
        }
        let end = name
            .iter()
            .position(|unit| *unit == 0)
            .ok_or(InspectionStage::Schema)?;
        let name = String::from_utf16(&name[..end]).map_err(|_| InspectionStage::Schema)?;
        if name.is_empty() || name.contains(['/', '\\']) {
            return Err(InspectionStage::Schema);
        }
        self.own_seen |= pid == own;
        self.present |= name.as_bytes().eq_ignore_ascii_case(image);
        Ok(())
    }
    fn finish(self, complete: bool) -> Result<bool, InspectionStage> {
        if !complete || !self.own_seen {
            return Err(InspectionStage::Schema);
        }
        Ok(self.present)
    }
}

#[cfg(any(windows, test))]
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
enum InspectionStage {
    Deadline,
    Snapshot,
    First,
    Next,
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
    use windows_sys::Win32::{
        Foundation::{
            CloseHandle, ERROR_NO_MORE_FILES, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
        },
        System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
    };
    struct Snapshot(HANDLE);
    impl Drop for Snapshot {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    let check = || {
        if std::time::Instant::now() >= deadline {
            Err(InspectionStage::Deadline)
        } else {
            Ok(())
        }
    };
    check()?;
    // The read-only snapshot is the only handle opened; enumerated PIDs are never opened.
    let handle = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if handle == INVALID_HANDLE_VALUE {
        return Err(InspectionStage::Snapshot);
    }
    let snapshot = Snapshot(handle);
    check()?;
    let mut entry = PROCESSENTRY32W {
        dwSize: u32::try_from(std::mem::size_of::<PROCESSENTRY32W>())
            .map_err(|_| InspectionStage::Schema)?,
        ..Default::default()
    };
    let mut valid = unsafe { Process32FirstW(snapshot.0, &mut entry) };
    if valid == 0 {
        return Err(InspectionStage::First);
    }
    let mut scan = ProcessScan::default();
    loop {
        check()?;
        scan.observe(
            entry.th32ProcessID,
            &entry.szExeFile,
            std::process::id(),
            image,
        )?;
        check()?;
        valid = unsafe { Process32NextW(snapshot.0, &mut entry) };
        let end_error = if valid == 0 {
            unsafe { GetLastError() }
        } else {
            0
        };
        check()?;
        if valid == 0 {
            if end_error != ERROR_NO_MORE_FILES {
                return Err(InspectionStage::Next);
            }
            return scan.finish(true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::Cell,
        time::{Duration, Instant},
    };
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
            InspectionStage::Snapshot,
            InspectionStage::First,
            InspectionStage::Next,
            InspectionStage::Schema,
            InspectionStage::Oversize,
        ];
        let expected = [
            "deadline", "snapshot", "first", "next", "schema", "oversize",
        ];
        for (stage, expected) in stages.into_iter().zip(expected) {
            let value = failure_facts(b"Claude.exe", stage).unwrap();
            assert_eq!(value["stage"], expected);
            assert_eq!(value["app"], "claude-desktop");
            assert_eq!(value.as_object().unwrap().len(), 5);
            let encoded = serde_json::to_string(&value).unwrap();
            assert!(!encoded.contains("PRIVATE_SENTINEL"));
        }
        assert!(failure_facts(b"PRIVATE_SENTINEL.exe", InspectionStage::Next).is_none());
        assert_eq!(
            failure_facts(b"ChatGPT.exe", InspectionStage::Snapshot).unwrap()["app"],
            "chatgpt-desktop"
        );
    }

    #[test]
    fn complete_utf16_scan_requires_self_and_exact_basename() {
        let name = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<_>>();
        let mut scan = ProcessScan::default();
        scan.observe(42, &name("checker.exe"), 42, b"Claude.exe")
            .unwrap();
        scan.observe(99, &name("cLaUdE.exe"), 42, b"Claude.exe")
            .unwrap();
        assert!(scan.finish(true).unwrap());
        let mut scan = ProcessScan::default();
        scan.observe(42, &name("checker.exe"), 42, b"Claude.exe")
            .unwrap();
        scan.observe(99, &name("ClaudeHelper.exe"), 42, b"Claude.exe")
            .unwrap();
        assert!(!scan.finish(true).unwrap());
        assert!(ProcessScan::default().finish(true).is_err());
        assert!(ProcessScan::default().finish(false).is_err());
        for invalid in [
            vec![0xd800, 0],
            vec![65; 260],
            name("C:/Claude.exe"),
            name(""),
        ] {
            assert!(
                ProcessScan::default()
                    .observe(42, &invalid, 42, b"Claude.exe")
                    .is_err()
            );
        }
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
