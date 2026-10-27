//! A separate native experiment; none of these observations qualify a release.

use super::{PublishedConformanceRunner, inventory_process_evidence};
use crate::conformance::arguments::RunKind;
use crate::conformance::constants::INVENTORY_MARKER;
use crate::conformance::registry::harness_registration;
use crate::conformance::report::InventoryProcessEvidence;
use crate::scripted_provider::{ProviderScenario, ScriptedProvider};
use crate::workspace::ConformanceWorkspace;
use nan_harness_core::HarnessKind;
use serde::Serialize;
use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

const TRUNCATION_MARKER: &str = "\n[output truncated; head and tail retained]\n";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum MimoFormat {
    Json,
    Default,
}

impl MimoFormat {
    fn argument(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Default => "default",
        }
    }
}

pub(super) fn select_format(
    arguments: &mut [OsString],
    format: Option<MimoFormat>,
    harness: HarnessKind,
    kind: &RunKind,
) {
    let Some(format) = format else { return };
    if harness != HarnessKind::MimoCode || !matches!(kind, RunKind::Inventory) {
        return;
    }
    let flag = arguments
        .windows(2)
        .position(|pair| pair[0] == "--format" && pair[1] == "json")
        .expect("MiMo inventory must retain its documented format argument");
    arguments[flag + 1] = format.argument().into();
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InventoryRecord {
    index: usize,
    format: MimoFormat,
    process: Option<InventoryProcessEvidence>,
    provider: Option<ProviderEvidence>,
    capture: Option<CaptureEvidence>,
    setup_error: Option<SetupError>,
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
enum SetupError {
    Workspace,
    Provider,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderEvidence {
    request_count: usize,
    completed: bool,
    bounded: bool,
    shutdown: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CaptureEvidence {
    stdout: StreamEvidence,
    stderr: StreamEvidence,
    events: EventCounts,
    decoded_text_marker: bool,
    ordered_text_marker: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StreamEvidence {
    retained_bytes: usize,
    truncated: bool,
    marker: bool,
}

impl StreamEvidence {
    fn observe(output: &str) -> Self {
        Self {
            retained_bytes: output.len(),
            truncated: output.contains(TRUNCATION_MARKER),
            marker: output.contains(INVENTORY_MARKER),
        }
    }
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct EventCounts {
    text: usize,
    step_start: usize,
    step_finish: usize,
    tool_use: usize,
    error: usize,
    reasoning: usize,
    unknown: usize,
    invalid: usize,
    non_event: usize,
}

impl CaptureEvidence {
    fn observe(stdout: &str, stderr: &str) -> Self {
        let mut result = Self {
            stdout: StreamEvidence::observe(stdout),
            stderr: StreamEvidence::observe(stderr),
            events: EventCounts::default(),
            decoded_text_marker: false,
            ordered_text_marker: false,
        };
        let mut ordered_text = String::new();
        for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
            let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else {
                result.events.invalid += 1;
                continue;
            };
            match event.get("type").and_then(serde_json::Value::as_str) {
                Some("text") => {
                    result.events.text += 1;
                    // MiMo 0.1.16 emits a completed text part, not a text delta.
                    if let Some(text) = event
                        .pointer("/part/text")
                        .and_then(serde_json::Value::as_str)
                    {
                        result.decoded_text_marker |= text.contains(INVENTORY_MARKER);
                        ordered_text.push_str(text);
                    }
                }
                Some("step_start") => result.events.step_start += 1,
                Some("step_finish") => result.events.step_finish += 1,
                Some("tool_use") => result.events.tool_use += 1,
                Some("error") => result.events.error += 1,
                Some("reasoning") => result.events.reasoning += 1,
                Some(_) => result.events.unknown += 1,
                None => result.events.non_event += 1,
            }
        }
        // This diagnostic never substitutes for the original raw stdout marker check.
        result.ordered_text_marker = ordered_text.contains(INVENTORY_MARKER);
        result
    }
}

async fn observe_inventory(
    binary: &std::path::Path,
    index: usize,
    format: MimoFormat,
) -> InventoryRecord {
    let mut record = InventoryRecord {
        index,
        format,
        process: None,
        provider: None,
        capture: None,
        setup_error: None,
    };
    let Ok(workspace) = ConformanceWorkspace::create() else {
        record.setup_error = Some(SetupError::Workspace);
        return record;
    };
    let Ok(provider) = ScriptedProvider::start(ProviderScenario::inventory(INVENTORY_MARKER)).await
    else {
        record.setup_error = Some(SetupError::Provider);
        return record;
    };
    let registration =
        *harness_registration(HarnessKind::MimoCode).expect("MiMo registration must exist");
    let mut runner = PublishedConformanceRunner::new(binary, HarnessKind::MimoCode);
    runner.mimo_inventory_format = Some(format);
    let output = runner
        .run_process(
            registration,
            &workspace,
            &provider,
            RunKind::Inventory,
            INVENTORY_MARKER,
        )
        .await;
    record.process = inventory_process_evidence(&output);
    record.capture = output
        .as_ref()
        .ok()
        .map(|output| CaptureEvidence::observe(&output.stdout, &output.stderr));
    let request_count = provider.chat_requests().len();
    let completed = provider.completed();
    let bounded = provider.recording_bounded();
    let shutdown = provider.shutdown().await.is_ok();
    record.provider = Some(ProviderEvidence {
        request_count,
        completed,
        bounded,
        shutdown,
    });
    record
}

// Match the published canary's provider scheduling while measuring output loss.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires an explicitly staged release CLI and stock MiMo on PATH"]
async fn native_mimo_inventory_output() {
    let binary = PathBuf::from(
        std::env::var_os("NAN_MIMO_INVENTORY_BINARY")
            .expect("NAN_MIMO_INVENTORY_BINARY is required"),
    );
    let destination = PathBuf::from(
        std::env::var_os("NAN_MIMO_INVENTORY_OUTPUT")
            .expect("NAN_MIMO_INVENTORY_OUTPUT is required"),
    );
    assert!(
        binary.is_absolute() && binary.is_file(),
        "release CLI must be an absolute file"
    );
    assert!(
        destination.is_absolute(),
        "diagnostic destination must be absolute"
    );
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    drop(
        options
            .open(&destination)
            .expect("could not reserve new private diagnostic file"),
    );
    let mut records = Vec::new();
    write_checkpoint(&destination, &records);
    // Preplanned fresh cases; neither failure nor output changes this sequence.
    for (index, format) in [
        MimoFormat::Json,
        MimoFormat::Default,
        MimoFormat::Json,
        MimoFormat::Default,
    ]
    .into_iter()
    .enumerate()
    {
        records.push(observe_inventory(&binary, index, format).await);
        write_checkpoint(&destination, &records);
    }
    assert!(
        records.iter().all(|record| record.setup_error.is_none()),
        "inventory experiment setup failed"
    );
    assert!(
        records.iter().all(|record| record
            .provider
            .as_ref()
            .is_some_and(|provider| provider.shutdown)),
        "scripted provider shutdown failed"
    );
}

fn write_checkpoint(destination: &std::path::Path, records: &[InventoryRecord]) {
    let parent = destination
        .parent()
        .expect("absolute destination has a parent");
    let mut file = tempfile::NamedTempFile::new_in(parent)
        .expect("could not create private diagnostic checkpoint");
    let encoded = serde_json::to_vec(records).expect("closed diagnostic must serialize");
    file.write_all(&encoded)
        .expect("could not write closed diagnostic");
    // Replace only the destination reserved by this test, retaining a complete prefix
    // if the external experiment deadline interrupts a later case.
    assert!(
        file.persist(destination).is_ok(),
        "could not publish closed diagnostic checkpoint"
    );
}

#[test]
fn documented_completed_events_are_counted_without_serializing_payloads() {
    let private = "synthetic-private-path-token-prompt";
    let stdout = [
        serde_json::json!({"type":"step_start","private":private}),
        serde_json::json!({"type":"text","sessionID":private,"part":{"text":INVENTORY_MARKER,"id":private}}),
        serde_json::json!({"type":"step_finish"}),
        serde_json::json!({"type":"tool_use","part":{"output":private}}),
        serde_json::json!({"type":"error","error":private}),
        serde_json::json!({"type":"reasoning","part":{"text":private}}),
        serde_json::json!({"type":private}),
        serde_json::json!({"private":private}),
        serde_json::json!(17),
    ].map(|event| event.to_string()).join("\r\n") + "\r\nnot-json";
    let observation = CaptureEvidence::observe(&stdout, private);
    assert!(
        observation.stdout.marker
            && observation.decoded_text_marker
            && observation.ordered_text_marker
    );
    assert_eq!(observation.stdout.retained_bytes, stdout.len());
    assert_eq!(observation.stderr.retained_bytes, private.len());
    assert_eq!(
        serde_json::to_value(&observation.events).unwrap(),
        serde_json::json!({
            "text":1,"stepStart":1,"stepFinish":1,"toolUse":1,"error":1,"reasoning":1,"unknown":1,"invalid":1,"nonEvent":2
        })
    );
    let encoded = serde_json::to_string(&observation).unwrap();
    assert!(!encoded.contains(private));
    assert!(!encoded.contains(INVENTORY_MARKER));
    assert!(!encoded.contains("sessionID"));
}

#[test]
fn only_documented_text_payloads_contribute_to_decoded_marker_evidence() {
    let split = INVENTORY_MARKER.len() / 2;
    let stdout = [
        serde_json::json!({"type":"text","part":{"text":&INVENTORY_MARKER[..split]}}),
        serde_json::json!({"type":"text","part":{"text":&INVENTORY_MARKER[split..]}}),
        serde_json::json!({"type":"text","text":INVENTORY_MARKER}),
        serde_json::json!({"type":"unknown","part":{"text":INVENTORY_MARKER}}),
    ]
    .map(|event| event.to_string())
    .join("\n");
    let observation = CaptureEvidence::observe(&stdout, "");
    assert!(!observation.decoded_text_marker);
    assert!(observation.ordered_text_marker);
    assert_eq!(observation.events.text, 3);
    let unsupported = CaptureEvidence::observe(
        &serde_json::json!({"type":"text","text":INVENTORY_MARKER}).to_string(),
        "",
    );
    assert!(unsupported.stdout.marker);
    assert!(!unsupported.decoded_text_marker && !unsupported.ordered_text_marker);
}

#[test]
fn truncated_capture_is_explicit_and_never_reconstructs_missing_json() {
    let stdout = format!("{{\"type\":\"text\"{TRUNCATION_MARKER}truncated-tail");
    let observation = CaptureEvidence::observe(&stdout, TRUNCATION_MARKER);
    assert!(observation.stdout.truncated && observation.stderr.truncated);
    assert_eq!(observation.stdout.retained_bytes, stdout.len());
    assert_eq!(observation.events.invalid, 3);
    assert_eq!(observation.events.text, 0);
    assert!(!observation.decoded_text_marker && !observation.ordered_text_marker);
}

#[test]
fn format_override_is_explicit_and_inventory_only() {
    let original = super::headless_arguments(
        HarnessKind::MimoCode,
        &RunKind::Inventory,
        INVENTORY_MARKER,
        std::path::Path::new("."),
        false,
    );
    for (harness, kind, format) in [
        (HarnessKind::MimoCode, RunKind::Inventory, None),
        (
            HarnessKind::MimoCode,
            RunKind::Sentinel,
            Some(MimoFormat::Default),
        ),
        (
            HarnessKind::OpenCode,
            RunKind::Inventory,
            Some(MimoFormat::Default),
        ),
    ] {
        let mut arguments = original.clone();
        select_format(&mut arguments, format, harness, &kind);
        assert_eq!(arguments, original);
    }
    let mut arguments = original.clone();
    select_format(
        &mut arguments,
        Some(MimoFormat::Default),
        HarnessKind::MimoCode,
        &RunKind::Inventory,
    );
    assert_eq!(arguments[3], "default");
    arguments[3] = "json".into();
    assert_eq!(arguments, original);
}

#[test]
fn wrapped_inventory_arguments_preserve_provider_boundary_and_native_flags() {
    let registration = *harness_registration(HarnessKind::MimoCode).unwrap();
    let mut runner = PublishedConformanceRunner::new("synthetic-cli", HarnessKind::MimoCode);
    let expected: Vec<OsString> = [
        OsString::from("mimo"),
        "--provider-base-url".into(),
        "http://127.0.0.1:1234/v1".into(),
        "--".into(),
        "run".into(),
        "--pure".into(),
        "--format".into(),
        "json".into(),
        "--dangerously-skip-permissions".into(),
        format!("Reply exactly {INVENTORY_MARKER} without using tools.").into(),
    ]
    .into();
    let arguments = |runner: &PublishedConformanceRunner| {
        runner.process_arguments(
            registration,
            std::path::Path::new("synthetic-workspace"),
            "http://127.0.0.1:1234/v1",
            &RunKind::Inventory,
            INVENTORY_MARKER,
        )
    };
    assert_eq!(arguments(&runner), expected);
    runner.mimo_inventory_format = Some(MimoFormat::Json);
    assert_eq!(arguments(&runner), expected);
    runner.mimo_inventory_format = Some(MimoFormat::Default);
    let mut default = expected;
    default[7] = "default".into();
    assert_eq!(arguments(&runner), default);
}

#[test]
fn checkpoints_replace_only_closed_completed_case_prefixes() {
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("diagnostic.json");
    std::fs::write(&destination, "[]").unwrap();
    let mut records = Vec::new();
    write_checkpoint(&destination, &records);
    for (index, format) in [MimoFormat::Json, MimoFormat::Default]
        .into_iter()
        .enumerate()
    {
        records.push(InventoryRecord {
            index,
            format,
            process: None,
            provider: None,
            capture: None,
            setup_error: Some(SetupError::Workspace),
        });
        write_checkpoint(&destination, &records);
        let actual: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&destination).unwrap()).unwrap();
        assert_eq!(actual, serde_json::to_value(&records).unwrap());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(destination).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
