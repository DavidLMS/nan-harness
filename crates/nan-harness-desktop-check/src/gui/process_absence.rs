//! Read-only process absence supplements window absence before restoration.
#[cfg(any(windows, test))]
use crate::report::Reason;
#[cfg(windows)]
use nan_harness_core::DesktopHarnessKind;

#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
enum SettlementState {
    NotQueried,
    Present,
    Absent,
    QueryFailed,
}

#[derive(Default)]
pub(super) struct ProcessSettlement {
    #[cfg(any(windows, test))]
    first: Option<SettlementState>,
    #[cfg(any(windows, test))]
    last: Option<SettlementState>,
    #[cfg(any(windows, test))]
    count: u16,
}

#[cfg(any(windows, test))]
impl ProcessSettlement {
    fn observe(&mut self, result: Result<bool, Reason>) {
        let state = match result {
            Ok(true) => SettlementState::Present,
            Ok(false) => SettlementState::Absent,
            Err(_) => SettlementState::QueryFailed,
        };
        self.first.get_or_insert(state);
        self.last = Some(state);
        self.count = self.count.saturating_add(1);
    }

    fn facts(&self) -> serde_json::Value {
        serde_json::json!({"schemaVersion":1, "mechanism":"windows-process-settlement",
            "diagnosticsOnly":true,
            "firstState":self.first.unwrap_or(SettlementState::NotQueried),
            "lastState":self.last.unwrap_or(SettlementState::NotQueried),
            "queryCount":(self.count <= 128).then_some(self.count)})
    }

    #[cfg(windows)]
    pub(super) fn record(&self) {
        save_facts(self.facts());
    }
}

#[cfg(any(windows, test))]
fn observed_query(
    query: impl FnOnce() -> Result<bool, Reason>,
    observer: Option<&mut ProcessSettlement>,
) -> Result<bool, Reason> {
    let result = query();
    if let Some(observer) = observer {
        observer.observe(result);
    }
    result
}

#[cfg(windows)]
pub(super) fn ensure_absent(
    kind: DesktopHarnessKind,
    native: Option<&crate::native::Native>,
    before_launch: bool,
) -> Result<(), Reason> {
    use std::time::{Duration, Instant};
    let image: &[u8] = match kind {
        DesktopHarnessKind::ChatGpt => b"ChatGPT.exe",
        DesktopHarnessKind::Claude => b"Claude.exe",
        _ => return Ok(()),
    };
    let deadline = Instant::now() + Duration::from_secs(2);
    let prepared;
    let native = match native {
        Some(native) => native,
        None => {
            prepared = crate::native::Native::new()?;
            &prepared
        }
    };
    let mut observed = false;
    wait_absent(
        |deadline| {
            inspect_with_baseline(
                || inspect(native, deadline, image),
                &mut observed,
                before_launch && kind == DesktopHarnessKind::Claude,
                |state| save_facts(before_launch_facts(state)),
            )
        },
        Instant::now,
        std::thread::sleep,
        deadline,
    )
}

#[cfg(any(windows, test))]
fn inspect_with_baseline(
    mut query: impl FnMut() -> Result<bool, Reason>,
    observed: &mut bool,
    before_launch: bool,
    mut record: impl FnMut(PostStopState),
) -> Result<bool, Reason> {
    let result = query();
    if before_launch && !*observed {
        *observed = true;
        record(match result {
            Ok(true) => PostStopState::Present,
            Ok(false) => PostStopState::Absent,
            Err(_) => PostStopState::QueryFailed,
        });
    }
    result
}

#[cfg(windows)]
pub(super) fn inspect_absent(
    kind: DesktopHarnessKind,
    deadline: std::time::Instant,
    native: Option<&crate::native::Native>,
    observer: Option<&mut ProcessSettlement>,
) -> Result<(), Reason> {
    let image: &[u8] = match kind {
        DesktopHarnessKind::ChatGpt => b"ChatGPT.exe",
        DesktopHarnessKind::Claude => b"Claude.exe",
        _ => return Ok(()),
    };
    if std::time::Instant::now() >= deadline {
        save_failure(image, InspectionStage::Deadline);
        return Err(Reason::DesktopUnavailable);
    }
    let prepared;
    let native = match native {
        Some(native) => native,
        None => {
            prepared = crate::native::Native::new()?;
            &prepared
        }
    };
    if observed_query(|| inspect(native, deadline, image), observer)? {
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

#[cfg(any(windows, test))]
fn before_launch_facts(state: PostStopState) -> serde_json::Value {
    serde_json::json!({"schemaVersion":1,"mechanism":"windows-process-baseline",
        "diagnosticsOnly":true,"app":"claude-desktop","phase":"before-launch","state":state})
}

#[cfg(windows)]
pub(super) fn observe_after_accessibility_rejection(
    deadline: std::time::Instant,
    native: Option<&crate::native::Native>,
    observer: Option<&mut ProcessSettlement>,
) {
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
        || std::env::var("RUNNER_OS").as_deref() != Ok("Windows")
        || std::time::Instant::now() >= deadline
    {
        return;
    }
    let prepared;
    let native = match native {
        Some(native) => Some(native),
        None => {
            prepared = crate::native::Native::new().ok();
            prepared.as_ref()
        }
    };
    let result = native
        .ok_or(Reason::DesktopUnavailable)
        .and_then(|native| observed_query(|| inspect(native, deadline, b"Claude.exe"), observer));
    let state = match result {
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
fn inspect(
    native: &crate::native::Native,
    deadline: std::time::Instant,
    image: &[u8],
) -> Result<bool, Reason> {
    let result = native
        .process_presence_until(image == b"Claude.exe", deadline)
        .map_err(|error| {
            if error == crate::native::FailureCategory::Timeout {
                InspectionStage::Deadline
            } else {
                InspectionStage::Schema
            }
        })
        .and_then(|output| parse_presence(&output));
    result.map_err(|stage| {
        save_failure(image, stage);
        Reason::DesktopUnavailable
    })
}

#[cfg(any(windows, test))]
fn parse_presence(output: &str) -> Result<bool, InspectionStage> {
    match output {
        "present\n" => Ok(true),
        "absent\n" => Ok(false),
        "error snapshot\n" => Err(InspectionStage::Snapshot),
        "error first\n" => Err(InspectionStage::First),
        "error next\n" => Err(InspectionStage::Next),
        "error oversize\n" => Err(InspectionStage::Oversize),
        _ => Err(InspectionStage::Schema),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn settlement_preserves_actual_results_without_repeating_queries() {
        let mut observer = ProcessSettlement::default();
        assert_eq!(observer.facts()["firstState"], "not-queried");
        assert_eq!(observer.facts()["queryCount"], 0);
        let mut queries = 0;
        for expected in [
            Ok(true),
            Ok(true),
            Err(Reason::DesktopUnavailable),
            Ok(false),
        ] {
            assert_eq!(
                observed_query(
                    || {
                        queries += 1;
                        expected
                    },
                    Some(&mut observer)
                ),
                expected
            );
        }
        assert_eq!(queries, 4);
        let facts = observer.facts();
        assert_eq!(facts.as_object().unwrap().len(), 6);
        assert_eq!(facts["firstState"], "present");
        assert_eq!(facts["lastState"], "absent");
        assert_eq!(facts["queryCount"], 4);
        assert_eq!(
            observed_query(|| Err(Reason::AlreadyRunning), None),
            Err(Reason::AlreadyRunning)
        );
    }

    #[test]
    fn settlement_overflow_is_explicit_and_retains_first_and_latest_states() {
        let mut observer = ProcessSettlement::default();
        for _ in 0..128 {
            observer.observe(Ok(true));
        }
        assert_eq!(observer.facts()["queryCount"], 128);
        observer.observe(Err(Reason::DesktopUnavailable));
        let facts = observer.facts();
        assert!(facts["queryCount"].is_null());
        assert_eq!(facts["firstState"], "present");
        assert_eq!(facts["lastState"], "query-failed");
    }

    #[test]
    fn settlement_deadline_records_only_queries_that_were_attempted() {
        use std::cell::Cell;
        use std::time::{Duration, Instant};
        let start = Instant::now();
        let deadline = start + Duration::from_millis(100);
        let clock = Cell::new(start);
        let mut observer = ProcessSettlement::default();
        let outcome = wait_absent(
            |bound| {
                observed_query(
                    || {
                        assert_eq!(bound, deadline);
                        clock.set(deadline);
                        Ok(false)
                    },
                    Some(&mut observer),
                )
            },
            || clock.get(),
            |_| panic!("no pause after expiry"),
            deadline,
        );
        assert_eq!(outcome, Err(Reason::CleanupFailed));
        assert_eq!(observer.facts()["lastState"], "absent");
        assert_eq!(observer.facts()["queryCount"], 1);
        let mut unqueried = ProcessSettlement::default();
        let outcome = wait_absent(
            |_| observed_query(|| panic!("expired query"), Some(&mut unqueried)),
            || deadline,
            |_| panic!("expired pause"),
            deadline,
        );
        assert_eq!(outcome, Err(Reason::CleanupFailed));
        assert_eq!(unqueried.facts()["queryCount"], 0);
    }

    use super::*;
    use std::{
        cell::Cell,
        time::{Duration, Instant},
    };
    #[test]
    fn prelaunch_guard_reuses_each_query_and_records_only_first_result() {
        let mut observed = false;
        let calls = Cell::new(0);
        let mut records = Vec::new();
        for present in [true, false] {
            assert_eq!(
                inspect_with_baseline(
                    || {
                        calls.set(calls.get() + 1);
                        Ok(present)
                    },
                    &mut observed,
                    true,
                    |state| records.push(before_launch_facts(state))
                ),
                Ok(present)
            );
        }
        assert_eq!(calls.get(), 2);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["state"], "present");
        let mut omitted = false;
        assert_eq!(
            inspect_with_baseline(
                || Err(Reason::DesktopUnavailable),
                &mut omitted,
                false,
                |_| panic!("restore cannot emit prelaunch receipt")
            ),
            Err(Reason::DesktopUnavailable)
        );
        assert!(!omitted);
    }

    #[test]
    fn prelaunch_observation_is_closed_and_cannot_be_confused_with_post_stop() {
        for (state, expected) in [
            (PostStopState::Present, "present"),
            (PostStopState::Absent, "absent"),
            (PostStopState::QueryFailed, "query-failed"),
        ] {
            let facts = before_launch_facts(state);
            assert_eq!(facts["state"], expected);
            assert_eq!(facts["phase"], "before-launch");
            assert_eq!(facts["app"], "claude-desktop");
            assert_eq!(facts.as_object().unwrap().len(), 6);
            assert_ne!(facts["mechanism"], post_stop_facts(state)["mechanism"]);
        }
    }

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
    fn native_presence_protocol_rejects_partial_or_private_payloads() {
        assert!(parse_presence("present\n").unwrap());
        assert!(!parse_presence("absent\n").unwrap());
        for rejected in [
            "present",
            "absent\nPRIVATE_SENTINEL",
            "present\nabsent\n",
            "error schema\n",
            "error next\n",
        ] {
            assert!(parse_presence(rejected).is_err());
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
