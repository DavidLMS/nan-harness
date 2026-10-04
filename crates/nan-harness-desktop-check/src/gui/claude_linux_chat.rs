//! Supervised source-bound Linux Chat input and assistant clipboard observations.
use super::{Gui, clipboard};
use crate::provider::ProviderGate;
use crate::report::Reason;
use nan_harness_core::DesktopHarnessKind;
use nan_harness_private_fs::open_private_new;
use num_traits::ToPrimitive as _;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use xa11y::Role;
use zeroize::Zeroizing;

pub(crate) fn policy() -> bool {
    [
        ("GITHUB_ACTIONS", "true"),
        ("RUNNER_ENVIRONMENT", "github-hosted"),
        ("RUNNER_OS", "Linux"),
        ("NANH_CLAUDE_LINUX_SOURCE_POLICY", "official-2.9939.4"),
        ("NANH_CLAUDE_LINUX_NATIVE_CHAT", "first-turn"),
        ("NANH_DESKTOP_QUALIFICATION_MODE", "startup-baseline"),
    ]
    .into_iter()
    .all(|(key, value)| std::env::var(key).as_deref() == Ok(value))
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum FailureBoundary {
    Request,
    Policy,
    NativeWindow,
    SourceOwner,
    Tree,
    TreeCycle,
    TreeDepth,
    TreeLimit,
    TreeIdentity,
    TreeChildren,
    ResponseHeading,
    ResponseRow,
    ResponseRowRoleLimit,
    ResponseRowCopyAbsent,
    ResponseRowCopyAmbiguous,
    ResponseRowHeadings,
    ResponseRowAttachment,
    State,
    Frame,
    FrameActive,
    FrameCount,
    FrameClient,
    Client,
    Mode,
    Focus,
    Input,
    InputMappingState,
    InputMappingChanged,
    InputEmptyState,
    InputEmptyWitness,
    Clipboard,
    Action,
    ActionCount,
    ActionName,
    ActionHit,
    Response,
    Transport,
    TransportSpawn,
    TransportIo,
    TransportWait,
    TransportStatus,
    TransportSize,
    TransportDecode,
    TransportDeadline,
}
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
enum EmptyInputField {
    WitnessPresent,
    RootSame,
    TextSame,
    RecordKeysSame,
    StateSame,
    AttributesSame,
    TextRecordsSame,
    OtherValuesSame,
    FocusOnlyStateChange,
    FocusAttempted,
}
type EmptyInputDrift = std::collections::BTreeMap<EmptyInputField, bool>;
fn native_tree_observation(facts: &Value) -> Option<Value> {
    let value = facts.get("nativeTreeObservation")?;
    if value.as_object()?.len() != 6 + usize::from(value.get("transportReason").is_some())
        || !["owner", "children", "identity"].contains(&value["operation"].as_str()?)
        || ![
            "wrong-owner",
            "query-unavailable",
            "deadline",
            "null-reference",
            "non-list",
            "limit",
            "duplicate",
        ]
        .contains(&value["reason"].as_str()?)
        || !["frame", "editor", "other"].contains(&value["nodeScope"].as_str()?)
        || !value["foreignBus"].is_boolean()
        || [("childCount", 1025), ("visitedCount", 1024)]
            .iter()
            .any(|(key, limit)| {
                value.get(*key).is_none()
                    || !value[*key].is_null() && value[*key].as_u64().is_none_or(|n| n > *limit)
            })
    {
        return None;
    }
    if let Some(transport) = value.get("transportReason")
        && (!["query-unavailable", "deadline", "null-reference"]
            .contains(&value["reason"].as_str()?)
            || ![
                "name-unowned",
                "service-unknown",
                "object-unknown",
                "interface-unknown",
                "method-unknown",
                "no-reply",
                "timeout",
                "disconnected",
                "failed",
                "other",
            ]
            .contains(&transport.as_str()?))
    {
        return None;
    }
    Some(value.clone())
}

fn retry_candidate_observation(facts: &Value) -> Option<Value> {
    let value = facts.get("retryCandidateObservation")?;
    let fields = value.as_object()?;
    let counts = [
        "pendingUserCount",
        "conversationHeadingCount",
        "tryAgainCount",
        "retryCount",
        "candidateRowHeadingCount",
    ];
    let flags = [
        "historyMatched",
        "candidateEnabled",
        "candidateActionUnique",
        "candidateHitMatched",
        "candidateUserAncestorMatched",
    ];
    if fields.len() != 11
        || counts
            .iter()
            .any(|key| value[*key].as_u64().is_none_or(|n| n > 32))
        || flags.iter().any(|key| !value[*key].is_boolean())
    {
        return None;
    }
    let total = value["tryAgainCount"].as_u64()? + value["retryCount"].as_u64()?;
    let label = match (
        value["tryAgainCount"].as_u64()?,
        value["retryCount"].as_u64()?,
    ) {
        (0, 0) => "none",
        (1, 0) => "try-again",
        (0, 1) => "retry",
        _ => "ambiguous",
    };
    if total > 32
        || value["candidateLabel"] != label
        || total != 1
            && (flags[1..].iter().any(|key| value[*key] != false)
                || value["candidateRowHeadingCount"] != 0)
        || value["candidateUserAncestorMatched"] == true && value["pendingUserCount"] != 1
    {
        return None;
    }
    Some(value.clone())
}

fn retry_candidate_ready(facts: &Value) -> bool {
    retry_candidate_observation(facts).is_some_and(|candidate| {
        candidate["pendingUserCount"] == 1
            && candidate["tryAgainCount"].as_u64().unwrap_or(0)
                + candidate["retryCount"].as_u64().unwrap_or(0)
                == 1
            && [
                "historyMatched",
                "candidateEnabled",
                "candidateActionUnique",
                "candidateHitMatched",
                "candidateUserAncestorMatched",
            ]
            .iter()
            .all(|key| candidate[*key] == true)
    })
}

fn empty_input_drift(facts: &Value) -> Option<EmptyInputDrift> {
    let value: EmptyInputDrift =
        serde_json::from_value(facts.get("emptyInputDrift")?.clone()).ok()?;
    (value.len() == 10).then_some(value)
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum SendActionClass {
    Click,
    Press,
    None,
    Multiple,
    Other,
}
fn send_action_class(facts: &Value) -> Option<SendActionClass> {
    serde_json::from_value(facts.get("sendActionClass")?.clone()).ok()
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ActivationClass {
    Click,
    Press,
    None,
    Ambiguous,
}
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SendActionObservation {
    action_count: u8,
    activation_match_count: u8,
    selected_index: Option<u8>,
    activation_class: ActivationClass,
}
fn send_action_observation(facts: &Value) -> Option<SendActionObservation> {
    let fields = facts.get("sendActionObservation")?;
    if fields.as_object()?.len() != 4 {
        return None;
    }
    let value: SendActionObservation = serde_json::from_value(fields.clone()).ok()?;
    let valid = value.action_count <= 8
        && value.activation_match_count <= value.action_count
        && match (
            value.activation_match_count,
            value.selected_index,
            value.activation_class,
        ) {
            (0, None, ActivationClass::None) | (2..=8, None, ActivationClass::Ambiguous) => true,
            (1, Some(index), ActivationClass::Click | ActivationClass::Press) => {
                index < value.action_count
            }
            _ => false,
        };
    valid.then_some(value)
}

fn failure_boundary(facts: &Value) -> Option<FailureBoundary> {
    serde_json::from_value(facts.get("failureBoundary")?.clone()).ok()
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InputShape {
    char_count: u16,
    only_line_breaks: bool,
    only_whitespace: bool,
    only_zero_width_markers: bool,
    #[serde(
        default,
        deserialize_with = "optional_shape_flag",
        skip_serializing_if = "Option::is_none"
    )]
    only_object_replacement: Option<bool>,
}
fn optional_shape_flag<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<bool>, D::Error> {
    bool::deserialize(deserializer).map(Some)
}
impl InputShape {
    fn valid(self) -> bool {
        if !(1..=4096).contains(&self.char_count) || self.only_line_breaks && !self.only_whitespace
        {
            return false;
        }
        if self.only_object_replacement == Some(true) {
            return !self.only_whitespace && !self.only_zero_width_markers;
        }
        !self.only_zero_width_markers || !self.only_whitespace
    }
}
fn input_shape(facts: &Value) -> Option<InputShape> {
    let shape: InputShape = serde_json::from_value(facts.get("inputShape")?.clone()).ok()?;
    shape.valid().then_some(shape)
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EmbeddedTextObservation {
    #[serde(rename = "nodeCount")]
    nodes: u8,
    #[serde(rename = "paragraphCount")]
    paragraphs: u8,
    #[serde(rename = "literalLfLeafCount")]
    literal_lf_leaves: u8,
    #[serde(rename = "brLfLeafCount")]
    br_lf_leaves: u8,
    #[serde(rename = "exactFillerLfLeafCount")]
    exact_filler_lf_leaves: u8,
}
fn embedded_text_observation(facts: &Value) -> Option<EmbeddedTextObservation> {
    let shape: EmbeddedTextObservation =
        serde_json::from_value(facts.get("embeddedTextObservation")?.clone()).ok()?;
    ((1..=64).contains(&shape.nodes)
        && shape.paragraphs <= shape.nodes
        && shape.literal_lf_leaves <= shape.nodes
        && shape.br_lf_leaves <= shape.literal_lf_leaves
        && shape.exact_filler_lf_leaves <= shape.br_lf_leaves)
        .then_some(shape)
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OwnedInputSourceShape {
    #[serde(rename = "paragraphTagPCount")]
    paragraph_tag_p: u8,
    #[serde(rename = "paragraphEmptyClassPairCount")]
    paragraph_empty_class_pair: u8,
    #[serde(rename = "paragraphDataPlaceholderCount")]
    paragraph_data_placeholder: u8,
    #[serde(rename = "unresolvedTextLeafCount")]
    unresolved_text_leaf: u8,
    #[serde(rename = "unresolvedOtherRoleCount")]
    unresolved_other_role: u8,
    #[serde(rename = "unresolvedEmptyTextCount")]
    unresolved_empty_text: u8,
    #[serde(rename = "unresolvedLfTextCount")]
    unresolved_lf_text: u8,
    #[serde(rename = "unresolvedExactResultCount")]
    unresolved_exact_result: u8,
    #[serde(rename = "unresolvedOtherTextCount")]
    unresolved_other_text: u8,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "Independent native diagnostic bits preserve the closed wire contract"
)]
struct OwnedInputObservation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_shape: Option<OwnedInputSourceShape>,
    node_count: u8,
    resolved_node_count: u8,
    paragraph_count: u8,
    root_child_count: u8,
    text_leaf_count: u8,
    other_role_count: u8,
    object_link_count: u8,
    complete_text_coverage: bool,
    root_single_paragraph: bool,
    root_only_objects: bool,
    placeholder_attribute_match: bool,
    placeholder_attribute_lf_match: bool,
    known_prompt_match_count: u8,
    latest_prompt_matches: bool,
}
fn owned_input_observation(facts: &Value) -> Option<OwnedInputObservation> {
    let shape: OwnedInputObservation =
        serde_json::from_value(facts.get("ownedInputObservation")?.clone()).ok()?;
    ((1..=64).contains(&shape.node_count)
        && [
            shape.resolved_node_count,
            shape.paragraph_count,
            shape.root_child_count,
            shape.text_leaf_count,
            shape.other_role_count,
        ]
        .into_iter()
        .all(|n| n <= shape.node_count)
        && shape.source_shape.is_none_or(|source| {
            [
                source.paragraph_tag_p,
                source.paragraph_empty_class_pair,
                source.paragraph_data_placeholder,
            ]
            .into_iter()
            .all(|n| n <= shape.paragraph_count)
                && u16::from(source.unresolved_text_leaf) + u16::from(source.unresolved_other_role)
                    == u16::from(shape.node_count - shape.resolved_node_count)
                && [
                    source.unresolved_empty_text,
                    source.unresolved_lf_text,
                    source.unresolved_exact_result,
                    source.unresolved_other_text,
                ]
                .into_iter()
                .map(u16::from)
                .sum::<u16>()
                    == u16::from(source.unresolved_text_leaf)
        })
        && shape.resolved_node_count > 0
        && shape.object_link_count < shape.node_count
        && shape.complete_text_coverage == (shape.resolved_node_count == shape.node_count)
        && (!shape.root_single_paragraph
            || shape.root_child_count == 1 && shape.paragraph_count > 0)
        && (!shape.root_only_objects || shape.object_link_count > 0)
        && shape.known_prompt_match_count <= 1
        && (!shape.latest_prompt_matches || shape.known_prompt_match_count == 1))
        .then_some(shape)
}

const FLAGS: [&str; 7] = [
    "inputVerified",
    "pasteAttempted",
    "sendAttempted",
    "sendForwarded",
    "responseVerified",
    "toolVerified",
    "recoveryVerified",
];
fn valid_binding(fields: &serde_json::Map<String, Value>) -> bool {
    if fields.len() != 6 {
        return false;
    }
    for key in ["editor", "frame"] {
        let Some(parts) = fields.get(key).and_then(Value::as_array) else {
            return false;
        };
        if parts.len() != 2
            || parts
                .iter()
                .any(|part| part.as_str().is_none_or(|s| s.is_empty() || s.len() > 1024))
        {
            return false;
        }
    }
    for key in ["editorIdentity", "frameIdentity"] {
        let Some(parts) = fields.get(key).and_then(Value::as_array) else {
            return false;
        };
        if parts.len() != 3
            || parts[0].as_u64().is_none_or(|role| role > 255)
            || parts[1..]
                .iter()
                .any(|part| part.as_str().is_none_or(|s| s.len() > 4096))
        {
            return false;
        }
    }
    for key in ["editorBounds", "frameBounds"] {
        let Some(parts) = fields.get(key).and_then(Value::as_array) else {
            return false;
        };
        if parts.len() != 4
            || parts.iter().any(|part| part.as_i64().is_none())
            || parts[2].as_i64().is_none_or(|n| n <= 0)
            || parts[3].as_i64().is_none_or(|n| n <= 0)
        {
            return false;
        }
    }
    true
}

// Optional diagnostics have narrower stage/attempt contracts than the base
// packet. Validate them together so no field can widen input authority.
fn validate_diagnostics(facts: &Value) -> Option<()> {
    let fields = facts.as_object()?;
    if fields.contains_key("nativeTreeObservation") && native_tree_observation(facts).is_none() {
        return None;
    }
    if fields.contains_key("retryCandidateObservation")
        && (retry_candidate_observation(facts).is_none() || facts["stage"] != "retry-diagnostic")
    {
        return None;
    }
    if fields.contains_key("emptyInputDrift")
        && (empty_input_drift(facts).is_none()
            || !matches!(
                facts["failureBoundary"].as_str(),
                Some("input-empty-state" | "input-empty-witness")
            ))
    {
        return None;
    }
    if fields.contains_key("sendActionObservation")
        && (send_action_observation(facts).is_none()
            || facts["inputVerified"] != true
            || !fields.contains_key("sendActionClass"))
    {
        return None;
    }
    if fields.contains_key("sendActionClass")
        && (!facts["inputVerified"].as_bool()?
            || !["click", "press", "none", "multiple", "other"]
                .contains(&facts["sendActionClass"].as_str()?))
    {
        return None;
    }
    if fields.contains_key("ownedInputObservation")
        && (owned_input_observation(facts).is_none()
            || facts["stage"] != "input-not-empty"
            || !fields.contains_key("embeddedTextObservation")
            || facts["ownedInputObservation"]["nodeCount"]
                != facts["embeddedTextObservation"]["nodeCount"]
            || facts["ownedInputObservation"]["paragraphCount"]
                != facts["embeddedTextObservation"]["paragraphCount"]
            || FLAGS.iter().any(|key| facts[*key] != false))
    {
        return None;
    }
    if fields.contains_key("embeddedTextObservation")
        && (embedded_text_observation(facts).is_none()
            || facts["stage"] != "input-not-empty"
            || !fields.contains_key("inputShape")
            || FLAGS.iter().any(|key| facts[*key] != false))
    {
        return None;
    }
    if fields.contains_key("inputShape")
        && (input_shape(facts).is_none()
            || !["input-not-empty", "clipboard-cleanup"].contains(&facts["stage"].as_str()?)
            || FLAGS.iter().any(|key| facts[*key] != false))
    {
        return None;
    }
    if fields.contains_key("failureBoundary")
        && (failure_boundary(facts).is_none()
            || ![
                "blocked",
                "action-uncertain",
                "deadline",
                "clipboard-cleanup",
                "input-not-empty",
                "response-mismatch",
            ]
            .contains(&facts["stage"].as_str()?))
    {
        return None;
    }
    Some(())
}

fn decode(bytes: &[u8]) -> Option<(Value, Option<Value>)> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    let object = value.as_object()?;
    if object.len() != 2 || !object.contains_key("facts") || !object.contains_key("binding") {
        return None;
    }
    let facts = value["facts"].as_object()?;
    if !(11..=22).contains(&facts.len())
        || facts.keys().any(|key| {
            !FLAGS.contains(&key.as_str())
                && ![
                    "schemaVersion",
                    "mechanism",
                    "diagnosticsOnly",
                    "stage",
                    "failureBoundary",
                    "inputShape",
                    "embeddedTextObservation",
                    "ownedInputObservation",
                    "sendActionClass",
                    "sendActionObservation",
                    "emptyInputDrift",
                    "retryCandidateObservation",
                    "retryAttempted",
                    "retryForwarded",
                    "nativeTreeObservation",
                ]
                .contains(&key.as_str())
        })
        || value["facts"]["schemaVersion"] != 1
        || value["facts"]["diagnosticsOnly"] != true
        || value["facts"]["mechanism"] != "claude-linux-native-chat"
        || FLAGS.iter().any(|key| !value["facts"][*key].is_boolean())
        || value["facts"]["toolVerified"] != false
        || value["facts"]["recoveryVerified"] != false
        || ![
            "source",
            "focus",
            "paste",
            "readback",
            "send",
            "input-not-empty",
            "blocked",
            "action-uncertain",
            "deadline",
            "clipboard-cleanup",
            "sent",
            "response-pending",
            "response-mismatch",
            "copied",
            "retry-diagnostic",
            "retry-forwarded",
        ]
        .contains(&value["facts"]["stage"].as_str()?)
        || value["facts"]["sendForwarded"] == true && value["facts"]["sendAttempted"] != true
        || value["facts"]["sendAttempted"] == true && value["facts"]["inputVerified"] != true
        || value["facts"]["responseVerified"] == true && value["facts"]["stage"] != "copied"
    {
        return None;
    }
    if facts.contains_key("retryAttempted") || facts.contains_key("retryForwarded") {
        if !facts["retryAttempted"].is_boolean()
            || !facts["retryForwarded"].is_boolean()
            || facts["retryForwarded"] == true && facts["retryAttempted"] != true
            || facts["retryForwarded"] == true
                && facts["stage"] != "retry-forwarded"
                && facts["stage"] != "action-uncertain"
            || facts["stage"] == "retry-forwarded" && facts["retryForwarded"] != true
        {
            return None;
        }
    } else if facts["stage"] == "retry-forwarded" {
        return None;
    }
    validate_diagnostics(&value["facts"])?;
    let binding = match &value["binding"] {
        Value::Null => None,
        Value::Object(fields) if valid_binding(fields) => Some(value["binding"].clone()),
        _ => return None,
    };
    Some((value["facts"].clone(), binding))
}

fn supervise(
    driver: &Path,
    request: Zeroizing<String>,
    deadline: Instant,
    failure: &mut FailureBoundary,
) -> Result<(Value, Option<Value>), Reason> {
    let mut command = Command::new("/usr/bin/python3");
    command.arg(driver).env_clear();
    for key in [
        "DISPLAY",
        "XAUTHORITY",
        "DBUS_SESSION_BUS_ADDRESS",
        "GITHUB_ACTIONS",
        "RUNNER_ENVIRONMENT",
        "RUNNER_OS",
        "NANH_CLAUDE_LINUX_SOURCE_POLICY",
        "NANH_CLAUDE_LINUX_NATIVE_CHAT",
        "NANH_DESKTOP_QUALIFICATION_MODE",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| {
            *failure = FailureBoundary::TransportSpawn;
            Reason::ActionUnsupported
        })?;
    *failure = FailureBoundary::TransportIo;
    let outcome = std::thread::scope(|scope| {
        let mut input = child.stdin.take()?;
        let output = child.stdout.take()?;
        let writer = scope.spawn(move || input.write_all(request.as_bytes()));
        let reader = scope.spawn(move || {
            let mut bytes = Zeroizing::new(Vec::new());
            output.take(65537).read_to_end(&mut bytes).ok()?;
            Some(bytes)
        });
        let status = loop {
            if Instant::now() >= deadline {
                *failure = FailureBoundary::TransportDeadline;
                break None;
            }
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) => std::thread::sleep(
                    Duration::from_millis(10)
                        .min(deadline.saturating_duration_since(Instant::now())),
                ),
                Err(_) => {
                    *failure = FailureBoundary::TransportWait;
                    break None;
                }
            }
        };
        if status.is_none() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let written = writer.join().ok()?.ok();
        let bytes = reader.join().ok()??;
        written?;
        if Instant::now() >= deadline {
            *failure = FailureBoundary::TransportDeadline;
            return None;
        }
        if bytes.len() > 65536 {
            *failure = FailureBoundary::TransportSize;
            return None;
        }
        if !status.is_some_and(|status| status.success()) {
            if !matches!(*failure, FailureBoundary::TransportWait) {
                *failure = FailureBoundary::TransportStatus;
            }
            return None;
        }
        *failure = FailureBoundary::TransportDecode;
        decode(&bytes)
    });
    if outcome.is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
    outcome.ok_or(if Instant::now() >= deadline {
        Reason::Timeout
    } else {
        Reason::ActionUnsupported
    })
}

struct KnownTurn {
    prompt: Zeroizing<String>,
    marker: Zeroizing<String>,
}
#[derive(Default)]
struct TurnHistory {
    // Only previous turns needed for the second/third input; never public facts.
    turns: Vec<KnownTurn>,
}
impl TurnHistory {
    fn record(&mut self, prompt: Zeroizing<String>, marker: &str) -> Result<(), Reason> {
        if self.turns.len() >= 2
            || prompt.is_empty()
            || prompt.len() > 4096
            || marker.is_empty()
            || marker.len() > 4096
            || self.turns.iter().any(|turn| {
                turn.prompt.as_str() == prompt.as_str() || turn.marker.as_str() == marker
            })
        {
            return Err(Reason::ActionUnsupported);
        }
        self.turns.push(KnownTurn {
            prompt,
            marker: Zeroizing::new(marker.to_owned()),
        });
        Ok(())
    }
    fn private_request(&self) -> Value {
        json!(
            self.turns
                .iter()
                .map(|turn| json!({"prompt":turn.prompt.as_str(),
            "marker":turn.marker.as_str()}))
                .collect::<Vec<_>>()
        )
    }
}

// Dispatch consumes the sole recovery attempt, including an uncertain transport.
enum RetryState {
    NotStarted,
    Waiting(Instant),
    DispatchUncertain,
    RejectedBeforeAction,
    Forwarded,
}
impl RetryState {
    fn attempted(&self) -> bool {
        matches!(self, Self::DispatchUncertain | Self::Forwarded)
    }
}

pub(crate) struct ClaudeLinuxChatSession<'a> {
    gui: &'a Gui,
    profile: &'a crate::probe::FreshClaudeLinuxProfile,
    replacement_consumed: bool,
    history: TurnHistory,
    pending_prompt: Option<Zeroizing<String>>,
    directory: PathBuf,
    driver: PathBuf,
    binding: Option<Value>,
    stage: String,
    failure_boundary: Option<FailureBoundary>,
    send_action_class: Option<SendActionClass>,
    send_action_observation: Option<SendActionObservation>,
    input_shape: Option<InputShape>,
    embedded_text_observation: Option<EmbeddedTextObservation>,
    owned_input_observation: Option<OwnedInputObservation>,
    empty_input_drift: Option<EmptyInputDrift>,
    retry_candidate: Option<Value>,
    retry: RetryState,
    native_tree: Option<Value>,
    submitted: u8,
    verified: u8,
    copied: u8,
    input_in_flight: bool,
    copy_in_flight: bool,
}
impl Gui {
    pub(crate) fn claude_linux_chat_session<'a>(
        &'a self,
        directory: &Path,
        profile: &'a crate::probe::FreshClaudeLinuxProfile,
    ) -> Result<ClaudeLinuxChatSession<'a>, Reason> {
        let driver = std::env::var_os("FEASIBILITY_CLAUDE_CHAT_DRIVER")
            .map(PathBuf::from)
            .ok_or(Reason::IsolationUnavailable)?;
        if self.kind != DesktopHarnessKind::Claude
            || !policy()
            || !driver.is_absolute()
            || driver.is_symlink()
            || !driver.is_file()
        {
            return Err(Reason::IsolationUnavailable);
        }
        Ok(ClaudeLinuxChatSession {
            gui: self,
            profile,
            replacement_consumed: false,
            history: TurnHistory::default(),
            pending_prompt: None,
            directory: directory.to_owned(),
            driver,
            binding: None,
            stage: "source".into(),
            failure_boundary: None,
            send_action_class: None,
            send_action_observation: None,
            input_shape: None,
            embedded_text_observation: None,
            owned_input_observation: None,
            empty_input_drift: None,
            retry_candidate: None,
            retry: RetryState::NotStarted,
            native_tree: None,
            submitted: 0,
            verified: 0,
            copied: 0,
            input_in_flight: false,
            copy_in_flight: false,
        })
    }
}
impl ClaudeLinuxChatSession<'_> {
    fn operation(&mut self, mode: &str, value: &str, deadline: Instant) -> Result<Value, Reason> {
        if !self.profile.verifies_owned(deadline) {
            return Err(Reason::IsolationUnavailable);
        }
        self.stage = "blocked".into();
        self.failure_boundary = Some(FailureBoundary::NativeWindow);
        if Instant::now() >= deadline
            || !self.gui.visual.linux_passive_composer_guard_until(deadline)
        {
            return Err(Reason::FocusChanged);
        }
        self.failure_boundary = Some(FailureBoundary::SourceOwner);
        let data = &self.gui.app.as_ref().ok_or(Reason::ActionUnsupported)?.data;
        if data.role != Role::Application || data.pid != Some(self.gui.visual.pid()) {
            return Err(Reason::FocusChanged);
        }
        let bus = data
            .raw
            .get("bus_name")
            .and_then(Value::as_str)
            .ok_or(Reason::ActionUnsupported)?;
        let path = data.stable_id.as_deref().ok_or(Reason::ActionUnsupported)?;
        let ticks = nix::time::clock_gettime(nix::time::ClockId::CLOCK_MONOTONIC)
            .map_err(|_| Reason::ActionUnsupported)?;
        let absolute = ticks.tv_sec().to_f64().ok_or(Reason::ActionUnsupported)?
            + ticks.tv_nsec().to_f64().ok_or(Reason::ActionUnsupported)? / 1_000_000_000.0
            + deadline
                .saturating_duration_since(Instant::now())
                .as_secs_f64();
        let mut request = self.gui.visual.linux_chat_window_request();
        request["bus"] = json!(bus);
        request["path"] = json!(path);
        request["checkerPid"] = json!(std::process::id());
        request["deadline"] = json!(absolute);
        request["mode"] = json!(mode);
        request["value"] = json!(value);
        request["binding"] = json!(self.binding);
        request["profileAuthority"] = self.profile.private_request(deadline)?;
        request["history"] = if matches!(
            mode,
            "input-next-correlated" | "input-next-empty-class" | "retry-ready" | "retry"
        ) {
            self.history.private_request()
        } else {
            json!([])
        };
        let payload = Zeroizing::new(request.to_string());
        self.failure_boundary = Some(FailureBoundary::Transport);
        let mut transport = FailureBoundary::Transport;
        let outcome = supervise(&self.driver, payload, deadline, &mut transport);
        if outcome.is_err() {
            self.failure_boundary = Some(transport);
        }
        let (facts, binding) = outcome?;
        self.failure_boundary = failure_boundary(&facts);
        self.send_action_class = send_action_class(&facts);
        self.send_action_observation = send_action_observation(&facts);
        self.input_shape = input_shape(&facts);
        self.embedded_text_observation = embedded_text_observation(&facts);
        self.owned_input_observation = owned_input_observation(&facts);
        self.empty_input_drift = empty_input_drift(&facts);
        self.retry_candidate = retry_candidate_observation(&facts);
        self.native_tree = native_tree_observation(&facts);
        facts["stage"]
            .as_str()
            .ok_or(Reason::ActionUnsupported)?
            .clone_into(&mut self.stage);
        if self.stage == "deadline" || Instant::now() >= deadline {
            // Preserve an exhausted helper budget before another native-window
            // query obscures it as a foreground failure. No later input is allowed.
            self.stage = "deadline".into();
            return Err(Reason::Timeout);
        }
        if !self.gui.visual.linux_passive_composer_guard_until(deadline) {
            self.stage = "blocked".into();
            self.failure_boundary = Some(FailureBoundary::NativeWindow);
            return Err(Reason::FocusChanged);
        }
        if Instant::now() >= deadline {
            return Err(Reason::Timeout);
        }
        if let Some(binding) = binding {
            self.binding = Some(binding);
        }
        Ok(facts)
    }
    pub(crate) fn new_turn(&mut self, prompt: &str) -> Result<(), Reason> {
        if self.input_in_flight
            || self.copy_in_flight
            || self.copied != self.submitted
            || self.submitted >= 3
            || self.history.turns.len() != usize::from(self.copied)
            || self.pending_prompt.is_some()
        {
            return Err(Reason::ActionUnsupported);
        }
        if prompt.is_empty()
            || prompt.len() > 4096
            || self
                .history
                .turns
                .iter()
                .any(|turn| turn.prompt.as_str() == prompt)
        {
            return Err(Reason::ActionUnsupported);
        }
        self.input_in_flight = true;
        let mode = if self.submitted == 0 && !self.replacement_consumed {
            self.replacement_consumed = true;
            "input-first-owned"
        } else if self.submitted > 0 {
            "input-next-empty-class"
        } else {
            return Err(Reason::ActionUnsupported);
        };
        let facts = self.operation(mode, prompt, Instant::now() + Duration::from_secs(15))?;
        if facts["inputVerified"] == true {
            self.verified += 1;
        }
        if facts["sendForwarded"] != true {
            return Err(if self.stage == "deadline" {
                Reason::Timeout
            } else {
                Reason::ActionUnsupported
            });
        }
        self.pending_prompt = Some(Zeroizing::new(prompt.to_owned()));
        self.submitted += 1;
        self.input_in_flight = false;
        Ok(())
    }
    pub(crate) fn wait_response(
        &mut self,
        marker: &str,
        timeout: Duration,
        gate: &ProviderGate,
    ) -> Result<(), Reason> {
        if self.copy_in_flight
            || self.input_in_flight
            || self.copied >= self.submitted
            || self.pending_prompt.is_none()
            || marker.is_empty()
            || marker.len() > 4096
        {
            return Err(Reason::ActionUnsupported);
        }
        let deadline = Instant::now() + timeout;
        loop {
            self.copy_in_flight = true;
            let facts = self.operation(
                "copy",
                marker,
                deadline.min(Instant::now() + Duration::from_secs(15)),
            )?;
            if facts["responseVerified"] == true {
                if clipboard::read()?.as_str() != marker || !gate.fixture_response_verified() {
                    return Err(Reason::ResponseMismatch);
                }
                let prompt = self
                    .pending_prompt
                    .take()
                    .ok_or(Reason::ActionUnsupported)?;
                if self.copied < 2 {
                    self.history.record(prompt, marker)?;
                }
                self.copied += 1;
                self.copy_in_flight = false;
                return Ok(());
            }
            if self.stage != "response-pending" {
                return Err(Reason::ActionUnsupported);
            }
            self.copy_in_flight = false;
            if Instant::now() >= deadline {
                return Err(Reason::Timeout);
            }
            std::thread::sleep(
                Duration::from_millis(100).min(deadline.saturating_duration_since(Instant::now())),
            );
        }
    }
    pub(crate) fn wait_retry(
        &mut self,
        timeout: Duration,
        gate: &ProviderGate,
    ) -> Result<(), Reason> {
        if self.submitted != 3 || self.copied != 2 || !matches!(self.retry, RetryState::NotStarted)
        {
            return Err(Reason::ActionUnsupported);
        }
        let prompt = self
            .pending_prompt
            .clone()
            .ok_or(Reason::ActionUnsupported)?;
        let deadline = Instant::now() + timeout;
        self.retry = RetryState::Waiting(deadline);
        while !gate.failure_observed() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(Reason::Timeout);
            }
            std::thread::sleep(Duration::from_millis(100).min(remaining));
        }
        loop {
            let facts = self.operation(
                "retry-ready",
                &prompt,
                deadline.min(Instant::now() + Duration::from_secs(15)),
            )?;
            if retry_candidate_ready(&facts) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(Reason::Timeout);
            }
            std::thread::sleep(
                Duration::from_millis(100).min(deadline.saturating_duration_since(Instant::now())),
            );
        }
    }
    pub(crate) fn retry_once(&mut self) -> Result<(), Reason> {
        let RetryState::Waiting(deadline) = self.retry else {
            return Err(Reason::ActionUnsupported);
        };
        if self.stage != "retry-diagnostic"
            || !self.retry_candidate.as_ref().is_some_and(|candidate| {
                retry_candidate_ready(&json!({"retryCandidateObservation":candidate}))
            })
            || self.submitted != 3
            || self.copied != 2
            || Instant::now() >= deadline
        {
            return Err(Reason::ActionUnsupported);
        }
        let prompt = self
            .pending_prompt
            .clone()
            .ok_or(Reason::ActionUnsupported)?;
        // Consumed before dispatch: uncertain transport/action never permits a fallback.
        self.retry = RetryState::DispatchUncertain;
        let facts = self.operation(
            "retry",
            &prompt,
            deadline.min(Instant::now() + Duration::from_secs(15)),
        )?;
        self.retry = if facts["retryAttempted"] != true {
            RetryState::RejectedBeforeAction
        } else if facts["retryForwarded"] == true {
            RetryState::Forwarded
        } else {
            RetryState::DispatchUncertain
        };
        if facts["retryForwarded"] == true && self.stage == "retry-forwarded" {
            Ok(())
        } else {
            Err(Reason::ActionUnsupported)
        }
    }
    pub(crate) fn finish(
        mut self,
        _gate: &ProviderGate,
        outcome: Result<(), Reason>,
    ) -> Result<(), Reason> {
        let cleared =
            clipboard::write("").is_ok() && clipboard::read().is_ok_and(|value| value.is_empty());
        if !cleared {
            self.stage = "clipboard-cleanup".into();
            self.failure_boundary = Some(FailureBoundary::Clipboard);
        }
        let mut facts = json!({"schemaVersion":1,"mechanism":"claude-linux-native-chat","diagnosticsOnly":true,
            "stage":self.stage,"submittedTurns":self.submitted,"inputVerifiedTurns":self.verified,
            "copiedResponses":self.copied,"retryAttempted":self.retry.attempted(),"clipboardCleared":cleared});
        if let Some(tree) = self.native_tree {
            facts["nativeTreeObservation"] = tree;
        }
        if let Some(boundary) = self.failure_boundary {
            facts["failureBoundary"] = json!(boundary);
        }
        if self.stage == "retry-diagnostic"
            && let Some(candidate) = self.retry_candidate
        {
            facts["retryCandidateObservation"] = candidate;
        }
        if let Some(drift) = self.empty_input_drift {
            facts["emptyInputDrift"] = json!(drift);
        }
        if let Some(observation) = self.send_action_observation {
            facts["sendActionObservation"] = json!(observation);
        }
        if let Some(class) = self.send_action_class {
            facts["sendActionClass"] = json!(class);
        }
        if let Some(shape) = self.input_shape {
            facts["inputShape"] = json!(shape);
        }
        if self.stage == "input-not-empty"
            && let Some(shape) = self.embedded_text_observation
        {
            facts["embeddedTextObservation"] = json!(shape);
        }
        if self.stage == "input-not-empty"
            && let Some(shape) = self.owned_input_observation
        {
            facts["ownedInputObservation"] = json!(shape);
        }
        let recorded = open_private_new(&self.directory.join(format!(
            "claude-linux-native-chat-{}.json",
            self.gui.visual.pid()
        )))
        .and_then(|mut file| file.write_all(facts.to_string().as_bytes()));
        if !cleared {
            return Err(Reason::CleanupFailed);
        }
        recorded.map_err(|_| Reason::IsolationUnavailable)?;
        outcome
    }
}

#[cfg(test)]
mod input_shape_tests {
    use super::{decode, input_shape, retry_candidate_ready};
    use serde_json::{Value, json};

    #[test]
    fn retry_requires_exact_failed_turn_and_preserves_uncertain_attempt_packets() {
        let candidate = json!({"pendingUserCount":1,"historyMatched":true,
            "conversationHeadingCount":6,"tryAgainCount":1,"retryCount":0,
            "candidateLabel":"try-again","candidateEnabled":true,"candidateActionUnique":true,
            "candidateHitMatched":true,"candidateUserAncestorMatched":true,"candidateRowHeadingCount":2});
        let mut facts = json!({"schemaVersion":1,"mechanism":"claude-linux-native-chat",
            "diagnosticsOnly":true,"stage":"retry-diagnostic","inputVerified":false,
            "pasteAttempted":false,"sendAttempted":false,"sendForwarded":false,
            "responseVerified":false,"toolVerified":false,"recoveryVerified":false,
            "retryCandidateObservation":candidate});
        assert!(retry_candidate_ready(&facts));
        for key in [
            "historyMatched",
            "candidateEnabled",
            "candidateActionUnique",
            "candidateHitMatched",
            "candidateUserAncestorMatched",
        ] {
            facts["retryCandidateObservation"][key] = json!(false);
            assert!(!retry_candidate_ready(&facts));
            facts["retryCandidateObservation"][key] = json!(true);
        }
        facts
            .as_object_mut()
            .unwrap()
            .remove("retryCandidateObservation");
        facts["retryAttempted"] = json!(true);
        facts["retryForwarded"] = json!(false);
        facts["stage"] = json!("action-uncertain");
        let packet =
            |facts: &Value| serde_json::to_vec(&json!({"facts":facts,"binding":null})).unwrap();
        assert!(decode(&packet(&facts)).is_some());
        facts["retryForwarded"] = json!(true);
        facts["stage"] = json!("retry-forwarded");
        assert!(decode(&packet(&facts)).is_some());
        assert_eq!(facts["sendAttempted"], false);
        facts["retryAttempted"] = json!(false);
        assert!(decode(&packet(&facts)).is_none());
        facts["retryAttempted"] = json!("private-value");
        assert!(decode(&packet(&facts)).is_none());
    }

    #[test]
    fn input_shape_is_closed_bounded_and_diagnostic_only() {
        let valid = json!({"inputShape":{"charCount":1,"onlyLineBreaks":true,
            "onlyWhitespace":true,"onlyZeroWidthMarkers":false}});
        assert!(input_shape(&valid).is_some());
        assert!(
            input_shape(&json!({"inputShape":{"charCount":1,"onlyLineBreaks":false,
            "onlyWhitespace":false,"onlyZeroWidthMarkers":false,"onlyObjectReplacement":true}}))
            .is_some()
        );
        for shape in [
            json!({"charCount":1,"onlyLineBreaks":false,"onlyWhitespace":false,"onlyZeroWidthMarkers":false,"onlyObjectReplacement":null}),
            json!({"charCount":1,"onlyLineBreaks":false,"onlyWhitespace":true,"onlyZeroWidthMarkers":false,"onlyObjectReplacement":true}),
            json!({"charCount":1,"onlyLineBreaks":false,"onlyWhitespace":false,"onlyZeroWidthMarkers":false,"onlyObjectReplacement":"PRIVATE"}),
            json!({"charCount":0,"onlyLineBreaks":false,"onlyWhitespace":false,"onlyZeroWidthMarkers":false}),
            json!({"charCount":4097,"onlyLineBreaks":false,"onlyWhitespace":false,"onlyZeroWidthMarkers":false}),
            json!({"charCount":true,"onlyLineBreaks":false,"onlyWhitespace":false,"onlyZeroWidthMarkers":false}),
            json!({"charCount":1,"onlyLineBreaks":true,"onlyWhitespace":false,"onlyZeroWidthMarkers":false}),
            json!({"charCount":1,"onlyLineBreaks":false,"onlyWhitespace":true,"onlyZeroWidthMarkers":true}),
            json!({"charCount":1,"onlyLineBreaks":false,"onlyWhitespace":false,"onlyZeroWidthMarkers":false,"value":"PRIVATE"}),
        ] {
            assert!(input_shape(&json!({"inputShape":shape})).is_none());
        }
    }
}

#[cfg(test)]
mod embedded_text_tests {
    use super::embedded_text_observation;
    use serde_json::json;
    #[test]
    fn rejects_unknown_private_fields_and_inconsistent_leaf_counts() {
        let valid = json!({"nodeCount":3,"paragraphCount":1,"literalLfLeafCount":1,
            "brLfLeafCount":1,"exactFillerLfLeafCount":1});
        assert!(embedded_text_observation(&json!({"embeddedTextObservation":valid})).is_some());
        for (key, value) in [
            ("nodeCount", json!(0)),
            ("nodeCount", json!(65)),
            ("nodeCount", json!(true)),
            ("paragraphCount", json!(4)),
            ("brLfLeafCount", json!(2)),
            ("exactFillerLfLeafCount", json!(2)),
            ("attributes", json!("PRIVATE")),
        ] {
            let mut changed = valid.clone();
            changed[key] = value;
            assert!(
                embedded_text_observation(&json!({"embeddedTextObservation":changed})).is_none()
            );
        }
        assert!(embedded_text_observation(&json!({"embeddedTextObservation":null})).is_none());
    }
}

#[cfg(test)]
mod helper_packet_tests {
    use super::decode;
    use serde_json::json;
    #[test]
    fn tree_rejections_remain_closed_without_exporting_native_nodes() {
        for boundary in [
            "tree-cycle",
            "tree-depth",
            "tree-limit",
            "tree-identity",
            "tree-children",
            "response-heading",
            "response-row",
            "response-row-role-limit",
            "response-row-copy-absent",
            "response-row-copy-ambiguous",
            "response-row-headings",
            "response-row-attachment",
        ] {
            let packet = json!({"facts":{"schemaVersion":1,"mechanism":"claude-linux-native-chat",
                "diagnosticsOnly":true,"stage":"blocked","failureBoundary":boundary,
                "inputVerified":false,"pasteAttempted":false,"sendAttempted":false,
                "sendForwarded":false,"responseVerified":false,"toolVerified":false,
                "recoveryVerified":false},"binding":null});
            assert!(decode(&serde_json::to_vec(&packet).unwrap()).is_some());
            let mut changed = packet.clone();
            changed["facts"]["nativePath"] = json!("PRIVATE");
            assert!(decode(&serde_json::to_vec(&changed).unwrap()).is_none());
        }
    }
    #[test]
    fn multi_action_packet_checks_exact_cardinality_and_private_fields() {
        let packet = json!({"facts":{"schemaVersion":1,"mechanism":"claude-linux-native-chat",
            "diagnosticsOnly":true,"stage":"sent","inputVerified":true,"pasteAttempted":true,
            "sendAttempted":true,"sendForwarded":true,"responseVerified":false,"toolVerified":false,
            "recoveryVerified":false,"sendActionClass":"multiple","sendActionObservation":{
                "actionCount":3,"activationMatchCount":1,"selectedIndex":1,"activationClass":"press"}},"binding":null});
        assert!(decode(&serde_json::to_vec(&packet).unwrap()).is_some());
        for (key, value) in [
            ("actionCount", json!(true)),
            ("actionCount", json!(9)),
            ("activationMatchCount", json!(2)),
            ("selectedIndex", json!(3)),
            ("activationClass", json!("PRIVATE")),
            ("text", json!("PRIVATE")),
        ] {
            let mut changed = packet.clone();
            changed["facts"]["sendActionObservation"][key] = value;
            assert!(decode(&serde_json::to_vec(&changed).unwrap()).is_none());
        }
    }
    #[test]
    fn send_action_packet_is_closed_and_requires_verified_input() {
        let packet = json!({"facts":{"schemaVersion":1,"mechanism":"claude-linux-native-chat",
            "diagnosticsOnly":true,"stage":"blocked","failureBoundary":"action-name",
            "inputVerified":true,"pasteAttempted":true,"sendAttempted":false,"sendForwarded":false,
            "responseVerified":false,"toolVerified":false,"recoveryVerified":false,"sendActionClass":"press"},
            "binding":null});
        assert!(decode(&serde_json::to_vec(&packet).unwrap()).is_some());
        for (key, value) in [
            ("sendActionClass", json!("PRIVATE")),
            ("sendActionClass", json!(true)),
            ("inputVerified", json!(false)),
            ("failureBoundary", json!("PRIVATE")),
        ] {
            let mut changed = packet.clone();
            changed["facts"][key] = value;
            assert!(decode(&serde_json::to_vec(&changed).unwrap()).is_none());
        }
    }
    #[test]
    fn full_nonempty_helper_packet_accepts_all_reproved_optional_diagnostics() {
        let packet = json!({"facts":{
            "schemaVersion":1,"mechanism":"claude-linux-native-chat","diagnosticsOnly":true,
            "stage":"input-not-empty","failureBoundary":"input",
            "inputVerified":false,"pasteAttempted":false,"sendAttempted":false,
            "sendForwarded":false,"responseVerified":false,"toolVerified":false,"recoveryVerified":false,
            "inputShape":{"charCount":1,"onlyLineBreaks":true,"onlyWhitespace":true,
                "onlyZeroWidthMarkers":false,"onlyObjectReplacement":false},
            "embeddedTextObservation":{"nodeCount":3,"paragraphCount":1,"literalLfLeafCount":1,
                "brLfLeafCount":1,"exactFillerLfLeafCount":1}},
            "binding":{"editor":["synthetic-owned","/editor"],"frame":["synthetic-owned","/frame"],
                "editorIdentity":[61,"synthetic-composer",""],"frameIdentity":[69,"synthetic-frame",""],
                "editorBounds":[10,10,100,30],"frameBounds":[0,0,300,200]}});
        let bytes = serde_json::to_vec(&packet).unwrap();
        let (facts, binding) = decode(&bytes).expect("valid complete diagnostic packet rejected");
        assert_eq!(facts, packet["facts"]);
        assert_eq!(binding.as_ref(), Some(&packet["binding"]));
        for (key, value) in [
            ("PRIVATE", json!("payload")),
            ("sendAttempted", json!(true)),
            ("embeddedTextObservation", json!(null)),
            ("inputShape", json!({"text":"PRIVATE"})),
        ] {
            let mut changed = packet.clone();
            changed["facts"][key] = value;
            assert!(decode(&serde_json::to_vec(&changed).unwrap()).is_none());
        }
        let mut changed = packet.clone();
        changed["binding"]["editorBounds"] = json!([10, 10, 0, 30]);
        assert!(decode(&serde_json::to_vec(&changed).unwrap()).is_none());
    }
}

#[cfg(test)]
mod turn_history_tests {
    use super::TurnHistory;
    use zeroize::Zeroizing;
    #[test]
    fn history_is_private_bounded_and_rejects_duplicate_pairs() {
        let mut history = TurnHistory::default();
        history
            .record(
                Zeroizing::new("owned prompt one".into()),
                "copied nonce one",
            )
            .unwrap();
        assert!(
            history
                .record(Zeroizing::new("owned prompt one".into()), "different nonce")
                .is_err()
        );
        assert!(
            history
                .record(
                    Zeroizing::new("different prompt".into()),
                    "copied nonce one"
                )
                .is_err()
        );
        assert!(
            history
                .record(Zeroizing::new(String::new()), "copied nonce two")
                .is_err()
        );
        history
            .record(
                Zeroizing::new("owned prompt two".into()),
                "copied nonce two",
            )
            .unwrap();
        assert_eq!(history.private_request().as_array().unwrap().len(), 2);
        assert!(
            history
                .record(
                    Zeroizing::new("owned prompt three".into()),
                    "copied nonce three"
                )
                .is_err()
        );
    }
}

#[cfg(test)]
mod owned_input_tests {
    use super::owned_input_observation;
    use serde_json::json;
    #[test]
    fn diagnostic_relation_and_coverage_are_closed_without_authority() {
        let valid = json!({"nodeCount":5,"resolvedNodeCount":2,"paragraphCount":1,
            "rootChildCount":1,"textLeafCount":3,"otherRoleCount":0,"objectLinkCount":1,
            "completeTextCoverage":false,"rootSingleParagraph":true,"rootOnlyObjects":true,
            "placeholderAttributeMatch":false,"placeholderAttributeLfMatch":false,
            "knownPromptMatchCount":0,"latestPromptMatches":false});
        assert!(owned_input_observation(&json!({"ownedInputObservation":valid})).is_some());
        for (key, value) in [
            ("nodeCount", json!(0)),
            ("nodeCount", json!(true)),
            ("resolvedNodeCount", json!(0)),
            ("resolvedNodeCount", json!(6)),
            ("completeTextCoverage", json!(true)),
            ("rootChildCount", json!(2)),
            ("objectLinkCount", json!(5)),
            ("knownPromptMatchCount", json!(2)),
            ("latestPromptMatches", json!(true)),
            ("rawValue", json!("private")),
        ] {
            let mut changed = valid.clone();
            changed[key] = value;
            assert!(owned_input_observation(&json!({"ownedInputObservation":changed})).is_none());
        }
    }
}

#[cfg(test)]
mod main_mode_packet_tests {
    use super::decode;
    #[test]
    fn actual_synthetic_main_packets_obey_decoder_contract() {
        assert!(decode(br#"{"facts":{"schemaVersion":1,"mechanism":"claude-linux-native-chat","diagnosticsOnly":true,"stage":"sent","inputVerified":true,"pasteAttempted":true,"sendAttempted":true,"sendForwarded":true,"responseVerified":false,"toolVerified":false,"recoveryVerified":false,"sendActionClass":"click","sendActionObservation":{"actionCount":1,"activationMatchCount":1,"selectedIndex":0,"activationClass":"click"}},"binding":{"editor":["r","editor2"],"frame":["r","frame"],"editorIdentity":[61,"Write your prompt to Claude",""],"editorBounds":[20,20,300,80],"frameIdentity":[23,"Claude",""],"frameBounds":[0,0,800,600]}}"#).is_some());
        assert!(decode(br#"{"facts":{"schemaVersion":1,"mechanism":"claude-linux-native-chat","diagnosticsOnly":true,"stage":"input-not-empty","inputVerified":false,"pasteAttempted":false,"sendAttempted":false,"sendForwarded":false,"responseVerified":false,"toolVerified":false,"recoveryVerified":false,"inputShape":{"charCount":21,"onlyLineBreaks":false,"onlyWhitespace":false,"onlyZeroWidthMarkers":false,"onlyObjectReplacement":false},"failureBoundary":"input"},"binding":null}"#).is_some());
        assert!(decode(br#"{"facts":{"schemaVersion":1,"mechanism":"claude-linux-native-chat","diagnosticsOnly":true,"stage":"blocked","inputVerified":false,"pasteAttempted":false,"sendAttempted":false,"sendForwarded":false,"responseVerified":false,"toolVerified":false,"recoveryVerified":false,"failureBoundary":"request"},"binding":null}"#).is_some());
    }
}

#[cfg(test)]
mod transport_cause_tests {
    use super::{FailureBoundary, supervise};
    use std::{
        fs,
        time::{Duration, Instant},
    };
    use zeroize::Zeroizing;
    #[test]
    fn bounded_synthetic_driver_causes_remain_distinct() {
        for (source, expected, budget) in [
            (
                "import sys; sys.stdout.write('invalid')",
                "transport-decode",
                2,
            ),
            ("import sys; sys.exit(7)", "transport-status", 2),
            (
                "import sys; sys.stdout.write('x'*70000)",
                "transport-size",
                2,
            ),
            ("import time; time.sleep(1)", "transport-deadline", 0),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let driver = directory.path().join("synthetic.py");
            fs::write(&driver, source).unwrap();
            let mut cause = FailureBoundary::Transport;
            let timeout = if budget == 0 {
                Duration::from_millis(20)
            } else {
                Duration::from_secs(budget)
            };
            assert!(
                supervise(
                    &driver,
                    Zeroizing::new(String::new()),
                    Instant::now() + timeout,
                    &mut cause
                )
                .is_err()
            );
            assert_eq!(serde_json::to_value(cause).unwrap(), expected);
        }
    }
}

#[cfg(test)]
mod empty_drift_tests {
    use super::empty_input_drift;
    use serde_json::json;

    #[test]
    fn empty_witness_diagnostics_reject_private_or_incomplete_packets() {
        let drift = json!({"witnessPresent":true,"rootSame":true,"textSame":true,
            "recordKeysSame":true,"stateSame":false,"attributesSame":true,
            "textRecordsSame":true,"otherValuesSame":true,"focusOnlyStateChange":true,
            "focusAttempted":true});
        assert!(empty_input_drift(&json!({"emptyInputDrift":drift})).is_some());
        for (key, value) in [("rawText", json!("PRIVATE")), ("rootSame", json!(1))] {
            let mut changed = drift.clone();
            changed[key] = value;
            assert!(empty_input_drift(&json!({"emptyInputDrift":changed})).is_none());
        }
        let mut missing = drift;
        missing.as_object_mut().unwrap().remove("focusAttempted");
        assert!(empty_input_drift(&json!({"emptyInputDrift":missing})).is_none());
    }
}

#[cfg(test)]
mod native_tree_tests {
    use super::native_tree_observation;
    use serde_json::json;

    #[test]
    fn nullable_counts_cannot_be_replaced_with_private_fields() {
        let tree = json!({"operation":"children","reason":"query-unavailable",
            "nodeScope":"editor","foreignBus":false,"childCount":null,"visitedCount":null});
        assert!(native_tree_observation(&json!({"nativeTreeObservation":tree})).is_some());
        let mut transport = tree.clone();
        transport["operation"] = json!("identity");
        transport["transportReason"] = json!("object-unknown");
        assert!(native_tree_observation(&json!({"nativeTreeObservation":transport})).is_some());
        for invalid in [json!("PRIVATE"), json!(true), json!(null)] {
            transport["transportReason"] = invalid;
            assert!(native_tree_observation(&json!({"nativeTreeObservation":transport})).is_none());
        }
        let mut changed = tree;
        changed.as_object_mut().unwrap().remove("childCount");
        changed["rawObjectPath"] = json!("PRIVATE");
        assert!(native_tree_observation(&json!({"nativeTreeObservation":changed})).is_none());
    }
}
