//! Hosted renderer experiment with one input owner and no OCR fallback.

use super::{ComposerFailure, Gui};
use crate::provider::ProviderGate;
use crate::report::{CheckStep, ProbeResult, Reason};
use nan_harness_private_fs::{open_private_new, open_private_read};
use serde::{Deserialize, Serialize};
use std::io::{Read as _, Write as _};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Request<'a> {
    connection_path: std::path::PathBuf,
    owner_pid: u32,
    prompt: &'a str,
    expected_marker: &'a str,
    timeout_ms: u32,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum DomAction {
    Ready,
    Submit,
    Retry,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum DomPurpose {
    Response,
    Failure,
}

#[derive(Clone, Copy)]
pub(crate) struct DomTurn<'a> {
    pub(crate) prompt: &'a str,
    pub(crate) marker: &'a str,
    pub(crate) action: DomAction,
    pub(crate) purpose: DomPurpose,
}

#[derive(Serialize)]
struct QualificationRequest<'a> {
    #[serde(flatten)]
    request: Request<'a>,
    action: DomAction,
    purpose: DomPurpose,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum DriverError {
    Unclassified,
    InvalidRequest,
    LauncherUnowned,
    EndpointUnowned,
    TargetAmbiguous,
    TargetInvalid,
    ComposerAmbiguous,
    SendUnavailable,
    StaleResponse,
    InputMismatch,
    ResponseTimeout,
    SubmitActionTimeout,
    SubmitActionIntercepted,
    SubmitActionDetached,
    SubmitActionFailed,
    ResponseObservationFailed,
    AttachmentOrActionFailed,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct EndpointFacts {
    endpoint_owned: bool,
    target_verified: bool,
    attached: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct InputFacts {
    unique_composer: bool,
    input_readback: bool,
    input_submitted: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SubmissionFacts {
    unique_send_control: bool,
    can_send: bool,
    input_cleared: bool,
    send_blocker: Option<SendBlocker>,
    send_mechanism: SendMechanism,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum SendMechanism {
    Pointer,
    SemanticKeyboard,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum SendBlocker {
    Modal,
    Menu,
    Tooltip,
    ComposerDragRegion,
    Other,
    Unmeasured,
    Focus,
    Disabled,
    Inert,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct TurnFacts {
    user_turn_observed: bool,
    assistant_turn_count: usize,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum RequestFailure {
    Aborted,
    Connection,
    Tls,
    Other,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct NetworkFacts {
    request_failed_count: usize,
    request_failure_category: Option<RequestFailure>,
    api_error_status: Option<u16>,
    api_error_response_count: usize,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ResponseFacts {
    response_verified: bool,
    synthetic_text_present: bool,
    #[serde(default)]
    provider_response_verified: bool,
    #[serde(default)]
    provider_generation_count: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum RetryHitTarget {
    #[serde(rename = "self")]
    Control,
    Composer,
    ErrorCard,
    Menu,
    Modal,
    Other,
    None,
    Unmeasured,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum RetryHitTag {
    Html,
    Body,
    Button,
    Div,
    Span,
    Svg,
    Other,
    None,
    Unmeasured,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum RetryHitRegion {
    ThreadViewport,
    PaneOverlay,
    PaneHost,
    NarrowOverlay,
    FloatingPane,
    TreeGroup,
    PanelHeader,
    PanelPageHeader,
    ZoneTabstrip,
    WindowDragHandle,
    GatewayConnecting,
    Onboarding,
    CommandBackdrop,
    DialogOverlay,
    ComposerRoot,
    ComposerDock,
    ComposerDragRegion,
    ComposerBounds,
    ComposerPortal,
    ParticleField,
    ChatDropOverlay,
    TitlebarDrag,
    Dialog,
    Popover,
    Tooltip,
    Other,
    None,
    Unmeasured,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RetryGeometryFacts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "retryRectInViewport")]
    rect_in_viewport: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "retryAncestorClipped")]
    ancestor_clipped: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "retryPointerEventsNone")]
    pointer_events_none: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "retryHitTag")]
    hit_tag: Option<RetryHitTag>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "retryHitRegion")]
    hit_region: Option<RetryHitRegion>,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RetryFocusFacts {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "retryFocusAfterAcquire"
    )]
    after_acquire: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "retryFocusBeforeAction"
    )]
    before_action: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "retryButtonConnected"
    )]
    button_connected: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "retryAncestorHidden"
    )]
    ancestor_hidden: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "retryAncestorInert"
    )]
    ancestor_inert: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "retryFieldsetDisabled"
    )]
    fieldset_disabled: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "retryDocumentFocused"
    )]
    document_focused: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "retryActiveTag"
    )]
    active_tag: Option<RetryHitTag>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "retryActiveRegion"
    )]
    active_region: Option<RetryHitRegion>,
}

impl RetryFocusFacts {
    fn complete(&self) -> bool {
        self.after_acquire.is_some()
            && self.before_action.is_some()
            && self.button_connected.is_some()
            && self.ancestor_hidden.is_some()
            && self.ancestor_inert.is_some()
            && self.fieldset_disabled.is_some()
            && self.document_focused.is_some()
            && self.active_tag.is_some()
            && self.active_region.is_some()
    }

    fn verified(&self) -> bool {
        self.after_acquire == Some(true)
            && self.before_action == Some(true)
            && self.button_connected == Some(true)
            && self.ancestor_hidden == Some(false)
            && self.ancestor_inert == Some(false)
            && self.fieldset_disabled == Some(false)
    }
}

#[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum RetrySampleStatus {
    Unmeasured,
    NativeControlInvalid,
    Detached,
    Hidden,
    Disabled,
    Inert,
    ForeignDocument,
    Clipped,
    PointerEventsNone,
    Transformed,
    OutsideViewport,
    NoOwnedPoint,
    Owned,
}

#[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum RetryReveal {
    None,
    CommandDismissed,
    OnboardingSkipped,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RetrySampleFacts {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "retryReveal"
    )]
    reveal: Option<RetryReveal>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "retrySampleStatus"
    )]
    status: Option<RetrySampleStatus>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "retryHitAncestor"
    )]
    hit_ancestor: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "retryHitSharesTurnPair"
    )]
    hit_shares_turn_pair: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "retryHitContainsComposer"
    )]
    hit_contains_composer: Option<bool>,
}

impl RetrySampleFacts {
    fn complete(&self) -> bool {
        self.reveal.is_some()
            && self.status.is_some()
            && self.hit_ancestor.is_some()
            && self.hit_shares_turn_pair.is_some()
            && self.hit_contains_composer.is_some()
    }
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct QualificationFacts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error_observed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    retry_control: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    retry_hit_owned: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    retry_hit_owned_points: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    retry_point_stable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    retry_hit_target: Option<RetryHitTarget>,
    #[serde(flatten)]
    geometry: RetryGeometryFacts,
    #[serde(flatten)]
    focus: RetryFocusFacts,
    #[serde(flatten)]
    sample: RetrySampleFacts,
}

impl QualificationFacts {
    fn pointer_verified(&self) -> bool {
        self.sample.status == Some(RetrySampleStatus::Owned)
            && self.retry_hit_owned == Some(true)
            && self.retry_hit_target == Some(RetryHitTarget::Control)
            && self
                .retry_hit_owned_points
                .is_some_and(|count| (1..=9).contains(&count))
            && self.retry_point_stable == Some(true)
            && self.geometry.rect_in_viewport == Some(true)
            && self.geometry.ancestor_clipped == Some(false)
            && self.geometry.pointer_events_none == Some(false)
    }

    fn retry_verified(&self, mechanism: &SendMechanism) -> bool {
        match mechanism {
            SendMechanism::Pointer => self.pointer_verified(),
            SendMechanism::SemanticKeyboard => self.focus.verified(),
        }
    }

    fn valid_action(
        &self,
        qualification: bool,
        input: &InputFacts,
        submission: &SubmissionFacts,
    ) -> bool {
        matches!(
            (qualification, self.error_observed, self.retry_control),
            (false, None, None) | (true, Some(_), Some(_))
        ) && (!qualification
            || self.retry_hit_owned.is_some()
                && self.retry_hit_owned_points.is_some_and(|count| count <= 9)
                && self.retry_point_stable.is_some()
                && self.geometry.rect_in_viewport.is_some()
                && self.geometry.ancestor_clipped.is_some()
                && self.geometry.pointer_events_none.is_some()
                && self.focus.complete()
                && self.sample.complete())
            && (self.retry_hit_owned != Some(true)
                || self.retry_hit_target == Some(RetryHitTarget::Control))
            && (!qualification
                || !input.input_submitted
                || !matches!(submission.send_mechanism, SendMechanism::Pointer)
                || self.pointer_verified())
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Facts {
    schema_version: u8,
    mechanism: String,
    #[serde(flatten)]
    endpoint: EndpointFacts,
    #[serde(flatten)]
    input: InputFacts,
    #[serde(flatten)]
    submission: SubmissionFacts,
    #[serde(flatten)]
    turns: TurnFacts,
    #[serde(flatten)]
    network: NetworkFacts,
    #[serde(flatten)]
    response: ResponseFacts,
    #[serde(flatten)]
    qualification: QualificationFacts,
    error_category: Option<DriverError>,
    playwright_version: Option<String>,
    observed_runtime_version: Option<String>,
}

const DOM_FACT_KEYS: &[&str] = &[
    "schemaVersion",
    "mechanism",
    "endpointOwned",
    "targetVerified",
    "attached",
    "uniqueComposer",
    "inputReadback",
    "inputSubmitted",
    "responseVerified",
    "syntheticTextPresent",
    "errorCategory",
    "playwrightVersion",
    "observedRuntimeVersion",
    "providerResponseVerified",
    "providerGenerationCount",
    "inputCleared",
    "userTurnObserved",
    "assistantTurnCount",
    "uniqueSendControl",
    "canSend",
    "sendBlocker",
    "sendMechanism",
    "requestFailedCount",
    "requestFailureCategory",
    "apiErrorStatus",
    "apiErrorResponseCount",
];

const DOM_QUALIFICATION_KEYS: &[&str] = &[
    "errorObserved",
    "retryControl",
    "retryHitOwned",
    "retryHitOwnedPoints",
    "retryPointStable",
    "retrySampleStatus",
    "retryReveal",
    "retryHitAncestor",
    "retryHitSharesTurnPair",
    "retryHitContainsComposer",
    "retryHitTarget",
    "retryRectInViewport",
    "retryAncestorClipped",
    "retryPointerEventsNone",
    "retryHitTag",
    "retryHitRegion",
    "retryFocusAfterAcquire",
    "retryFocusBeforeAction",
    "retryButtonConnected",
    "retryAncestorHidden",
    "retryAncestorInert",
    "retryFieldsetDisabled",
    "retryDocumentFocused",
    "retryActiveTag",
    "retryActiveRegion",
];

fn read_facts(path: &Path, qualification: bool) -> Result<Facts, Reason> {
    let mut bytes = Vec::new();
    open_private_read(path)
        .and_then(|(file, _)| file.take(8193).read_to_end(&mut bytes))
        .map_err(|_| Reason::IsolationUnavailable)?;
    if bytes.len() > 8192 {
        return Err(Reason::IsolationUnavailable);
    }
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| Reason::IsolationUnavailable)?;
    let object = value.as_object().ok_or(Reason::IsolationUnavailable)?;
    let extra = if qualification {
        DOM_QUALIFICATION_KEYS.len()
    } else {
        0
    };
    let count = (24 + extra)..=(26 + extra);
    if !count.contains(&object.len())
        || object.keys().any(|key| {
            !DOM_FACT_KEYS.contains(&key.as_str())
                && !DOM_QUALIFICATION_KEYS.contains(&key.as_str())
        })
    {
        return Err(Reason::IsolationUnavailable);
    }
    if DOM_QUALIFICATION_KEYS
        .iter()
        .any(|key| qualification != object.contains_key(*key))
    {
        return Err(Reason::IsolationUnavailable);
    }
    let facts: Facts = serde_json::from_value(value).map_err(|_| Reason::IsolationUnavailable)?;
    let version = |value: &Option<String>| {
        value.as_ref().is_none_or(|value| {
            !value.is_empty()
                && value.len() <= 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b".- ".contains(&byte))
        })
    };
    if facts.schema_version != 1
        || facts.mechanism
            != if qualification {
                "hermes-renderer-qualification"
            } else {
                "hermes-playwright-dom"
            }
        || !facts
            .qualification
            .valid_action(qualification, &facts.input, &facts.submission)
        || !version(&facts.playwright_version)
        || !version(&facts.observed_runtime_version)
        || facts.turns.assistant_turn_count > 4096
        || facts.network.request_failed_count > 4096
        || facts.network.api_error_response_count > 4096
        || facts
            .network
            .api_error_status
            .is_some_and(|status| !(400..=599).contains(&status))
        || facts.input.input_submitted
            && !(facts.endpoint.endpoint_owned
                && facts.endpoint.target_verified
                && facts.endpoint.attached
                && facts.input.unique_composer
                && facts.input.input_readback
                && ((facts.submission.unique_send_control && facts.submission.can_send)
                    || qualification && facts.qualification.retry_control == Some(true))
                && facts.submission.send_blocker.is_none())
        || facts.response.response_verified
            && !(facts.endpoint.endpoint_owned
                && facts.endpoint.target_verified
                && facts.endpoint.attached
                && facts.input.unique_composer
                && facts.input.input_readback
                && facts.input.input_submitted
                && facts.response.synthetic_text_present
                && facts.error_category.is_none())
    {
        return Err(Reason::IsolationUnavailable);
    }
    Ok(facts)
}

pub(crate) struct RendererSession<'a> {
    process: &'a mut crate::process::ProbeProcess,
    directory: &'a Path,
    owner: u32,
    readiness_deadline: Instant,
}

fn renderer_guard(
    process: &mut impl crate::process::Observation,
    owner: u32,
) -> Result<(), Reason> {
    if process.id() != Some(owner) {
        return Err(Reason::IsolationUnavailable);
    }
    if process
        .try_wait()
        .map_err(|_| Reason::ActionUnsupported)?
        .is_some()
    {
        return Err(Reason::ApplicationExited);
    }
    Ok(())
}

impl<'a> RendererSession<'a> {
    pub(crate) fn new(
        process: &'a mut crate::process::ProbeProcess,
        directory: &'a Path,
    ) -> Result<Self, Reason> {
        let owner = process.id().ok_or(Reason::ApplicationExited)?;
        renderer_guard(process, owner)?;
        Ok(Self {
            process,
            directory,
            owner,
            readiness_deadline: Instant::now() + Duration::from_secs(125),
        })
    }

    pub(crate) fn prepare_hermes_profile(&mut self, workspace: &Path) -> Result<(), Reason> {
        crate::probe::hermes_readiness::prepare(workspace, self.readiness_deadline, || {
            renderer_guard(self.process, self.owner)
        })
    }

    pub(crate) fn inventory(&mut self) -> Result<(), Reason> {
        let directory = self.directory;
        let owner = self.owner;
        renderer_guard(self.process, owner)?;
        let driver = std::env::var_os("NANH_DESKTOP_RENDERER_DRIVER")
            .map(std::path::PathBuf::from)
            .ok_or(Reason::IsolationUnavailable)?;
        if !driver.is_absolute() || !driver.is_file() || driver.is_symlink() {
            return Err(Reason::IsolationUnavailable);
        }
        let request_path = directory.join(format!("renderer-inventory-{owner}.private"));
        let output_path = directory.join(format!("renderer-inventory-{owner}.json"));
        let request = serde_json::json!({"ownerPid": owner, "connectionPath": directory.join(format!("connection-{owner}.json"))});
        open_private_new(&request_path)
            .and_then(|mut file| file.write_all(request.to_string().as_bytes()))
            .map_err(|_| Reason::IsolationUnavailable)?;
        let outcome = run_driver(
            &driver,
            &request_path,
            &output_path,
            false,
            inventory_driver_limit(),
            || renderer_guard(self.process, owner),
        );
        std::fs::remove_file(request_path).map_err(|_| Reason::IsolationUnavailable)?;
        outcome?;
        let mut bytes = Vec::new();
        open_private_read(&output_path)
            .and_then(|(file, _)| file.take(8193).read_to_end(&mut bytes))
            .map_err(|_| Reason::IsolationUnavailable)?;
        if bytes.len() > 8192 {
            return Err(Reason::IsolationUnavailable);
        }
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| Reason::IsolationUnavailable)?;
        if value["schemaVersion"] != 1
            || value["mechanism"] != "renderer-inventory"
            || value["endpointOwned"] != true
            || value["launcherOwned"] != true
            || value["attached"] != true
            || (value["pageCount"] != 1
                && !(std::env::var("NANH_DESKTOP_RENDERER_APP").as_deref()
                    == Ok("chatgpt-desktop")
                    && value["pageCount"] == 2
                    && value["codexSession"]["pageCount"] == 2
                    && value["codexSession"]["bindingVerified"] == true
                    && value["codexSession"]["auxiliaryInert"] == true
                    && value["codexSession"]["codingComposerReady"] == true))
        {
            return Err(Reason::ActionUnsupported);
        }
        Ok(())
    }

    pub(crate) fn turn(
        &mut self,
        turn: DomTurn<'_>,
        provider: &ProviderGate,
    ) -> Result<(), Reason> {
        renderer_guard(self.process, self.owner)?;
        let directory = self.directory;
        let owner = self.owner;
        let driver = std::env::var_os("FEASIBILITY_HERMES_DOM_DRIVER")
            .map(std::path::PathBuf::from)
            .ok_or(Reason::IsolationUnavailable)?;
        if !driver.is_absolute()
            || !std::fs::symlink_metadata(&driver)
                .is_ok_and(|metadata| metadata.file_type().is_file())
        {
            return Err(Reason::IsolationUnavailable);
        }
        let mut nonce = [0u8; 8];
        getrandom::fill(&mut nonce).map_err(|_| Reason::IsolationUnavailable)?;
        let stem = format!("qualification-{owner}-{}", u64::from_le_bytes(nonce));
        let request_path = directory.join(format!("{stem}.private"));
        let output_path = directory.join(format!("{stem}.json"));
        let (request_ms, process_limit) = qualification_timing(
            matches!(turn.action, DomAction::Ready) && crate::probe::hermes_readiness::enabled(),
            self.readiness_deadline,
        )?;
        let request = QualificationRequest {
            request: Request {
                connection_path: directory.join(format!("connection-{owner}.json")),
                owner_pid: owner,
                prompt: turn.prompt,
                expected_marker: turn.marker,
                timeout_ms: request_ms,
            },
            action: turn.action,
            purpose: turn.purpose,
        };
        let bytes = serde_json::to_vec(&request).map_err(|_| Reason::IsolationUnavailable)?;
        open_private_new(&request_path)
            .and_then(|mut file| file.write_all(&bytes))
            .map_err(|_| Reason::IsolationUnavailable)?;
        let outcome = run_driver(
            &driver,
            &request_path,
            &output_path,
            true,
            process_limit,
            || renderer_guard(self.process, owner),
        );
        std::fs::remove_file(&request_path).map_err(|_| Reason::IsolationUnavailable)?;
        // Failed actions still retain independent provider evidence. This is
        // observation only and never grants a second submission.
        let mut facts = match read_facts(&output_path, true) {
            Ok(facts) => facts,
            Err(reason) => return Err(outcome.err().unwrap_or(reason)),
        };
        facts.response.provider_response_verified = provider.fixture_response_verified();
        facts.response.provider_generation_count =
            Some(provider.generation_count()).filter(|count| *count <= 4096);
        let bytes = serde_json::to_vec(&facts).map_err(|_| Reason::IsolationUnavailable)?;
        let final_path = output_path.with_extension("closed");
        open_private_new(&final_path)
            .and_then(|mut file| file.write_all(&bytes))
            .and_then(|()| std::fs::rename(final_path, &output_path))
            .map_err(|_| Reason::IsolationUnavailable)?;
        outcome?;
        if matches!(turn.action, DomAction::Ready) {
            return if facts.endpoint.endpoint_owned
                && facts.endpoint.target_verified
                && facts.endpoint.attached
                && facts.error_category.is_none()
                && !facts.input.input_submitted
            {
                Ok(())
            } else {
                Err(Reason::IsolationUnavailable)
            };
        }
        if !facts.input.input_submitted
            || facts.error_category.is_some()
            || matches!(turn.action, DomAction::Retry)
                && !facts
                    .qualification
                    .retry_verified(&facts.submission.send_mechanism)
        {
            return Err(Reason::ResponseMismatch);
        }
        match turn.purpose {
            DomPurpose::Failure
                if facts.qualification.error_observed == Some(true)
                    && facts.qualification.retry_control == Some(true)
                    && provider.failure_observed() =>
            {
                Ok(())
            }
            DomPurpose::Response
                if facts.response.response_verified && provider.fixture_response_verified() =>
            {
                Ok(())
            }
            _ => Err(Reason::ResponseMismatch),
        }
    }
}

fn inventory_driver_limit() -> Duration {
    let public_setup_trial = cfg!(windows)
        && std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true")
        && std::env::var("RUNNER_ENVIRONMENT").as_deref() == Ok("github-hosted")
        && std::env::var("RUNNER_OS").as_deref() == Ok("Windows")
        && std::env::var("NANH_DESKTOP_RENDERER_APP").as_deref() == Ok("chatgpt-desktop")
        && std::env::var("NANH_CODEX_PUBLIC_ONBOARDING").as_deref() == Ok("engineering");
    // Trial: 35s startup + at most 25s public setup, with room for the initial
    // native ownership query and driver teardown. The worker remains capped.
    Duration::from_secs(if public_setup_trial { 75 } else { 35 })
}

impl Gui {
    pub(crate) fn inventory_renderer(
        &self,
        directory: &Path,
        owner: u32,
        composer_observations: &mut Vec<ComposerFailure>,
    ) -> Result<(), Reason> {
        self.observe_hosted_startup(composer_observations)?;
        if std::env::var("NANH_DESKTOP_QUALIFICATION_MODE").as_deref() == Ok("startup-baseline") {
            let count = |selector: &str| {
                self.app.as_ref().and_then(|app| {
                    app.locator(selector)
                        .elements()
                        .ok()
                        .and_then(|elements| (elements.len() <= 4096).then_some(elements.len()))
                })
            };
            let inventory = serde_json::json!({"appPresent": self.app.is_some(),
                "editableCount": count("text_area[visible=\"true\"][editable=\"true\"], text_field[visible=\"true\"][editable=\"true\"]"),
                "retryCount": count("button[visible=\"true\"][name=\"Retry\"], button[visible=\"true\"][name=\"Try again\"]"),
                "loginCount": count("button[visible=\"true\"][name=\"Sign in\"], button[visible=\"true\"][name=\"Log in\"]")});
            let composer_inventory = self.claude_composer_inventory();
            // Only role/known-control counts leave memory; this never sends input.
            self.observe_hosted_startup(composer_observations)?;
            if let Some(counts) = composer_inventory {
                super::claude_native_probe::record(directory, owner, &counts);
            }
            #[cfg(target_os = "macos")]
            self.claude_chat_navigation(directory, owner, composer_observations)?;
            let value = serde_json::json!({"schemaVersion":1, "mechanism":"renderer-startup-baseline", "diagnosticsOnly":true, "windowAcquired":true, "rendererInstrumented":false, "accessibilityInventory": inventory});
            open_private_new(&directory.join(format!("baseline-{owner}.json")))
                .and_then(|mut file| file.write_all(value.to_string().as_bytes()))
                .map_err(|_| Reason::IsolationUnavailable)?;
            return Err(Reason::ActionUnsupported);
        }
        let driver = std::env::var_os("NANH_DESKTOP_RENDERER_DRIVER")
            .map(std::path::PathBuf::from)
            .ok_or(Reason::IsolationUnavailable)?;
        if !driver.is_absolute() || !driver.is_file() || driver.is_symlink() {
            return Err(Reason::IsolationUnavailable);
        }
        let request_path = directory.join(format!("renderer-inventory-{owner}.private"));
        let output_path = directory.join(format!("renderer-inventory-{owner}.json"));
        let request = serde_json::json!({"ownerPid": owner,
            "connectionPath": directory.join(format!("connection-{owner}.json"))});
        open_private_new(&request_path)
            .and_then(|mut file| file.write_all(request.to_string().as_bytes()))
            .map_err(|_| Reason::IsolationUnavailable)?;
        let outcome = self.run_dom_driver(&driver, &request_path, &output_path, false);
        std::fs::remove_file(request_path).map_err(|_| Reason::IsolationUnavailable)?;
        outcome?;
        // An inventory can never satisfy input, response, tool or recovery acceptance.
        Err(Reason::ActionUnsupported)
    }

    pub(crate) fn probe_dom(
        &self,
        directory: &Path,
        owner: u32,
        marker: &str,
        result: &mut ProbeResult,
        provider: &ProviderGate,
    ) -> Result<(), Reason> {
        if self.kind == nan_harness_core::DesktopHarnessKind::ChatGpt {
            return self.probe_codex_dom(directory, owner, marker, result, provider);
        }
        // DOM targets are owned through native window and socket ancestry proofs.
        // Requiring an AX foreground provider would defeat this renderer adapter.
        self.visual.guard()?;
        let driver = std::env::var_os("FEASIBILITY_HERMES_DOM_DRIVER")
            .map(std::path::PathBuf::from)
            .ok_or(Reason::IsolationUnavailable)?;
        if !driver.is_absolute()
            || !std::fs::symlink_metadata(&driver)
                .is_ok_and(|metadata| metadata.file_type().is_file())
        {
            return Err(Reason::IsolationUnavailable);
        }
        let mut nonce = [0u8; 8];
        getrandom::fill(&mut nonce).map_err(|_| Reason::IsolationUnavailable)?;
        let stem = format!("{owner}-{}", u64::from_le_bytes(nonce));
        let request_path = directory.join(format!("request-{stem}.private"));
        let output_path = directory.join(format!("dom-{stem}.json"));
        let request = Request {
            connection_path: directory.join(format!("connection-{owner}.json")),
            owner_pid: owner,
            prompt: "Check this connection",
            expected_marker: marker,
            timeout_ms: 30_000,
        };
        let bytes = serde_json::to_vec(&request).map_err(|_| Reason::IsolationUnavailable)?;
        open_private_new(&request_path)
            .and_then(|mut file| file.write_all(&bytes))
            .map_err(|_| Reason::IsolationUnavailable)?;
        let outcome = self.run_dom_driver(&driver, &request_path, &output_path, false);
        std::fs::remove_file(&request_path).map_err(|_| Reason::IsolationUnavailable)?;
        outcome?;
        let mut facts = read_facts(&output_path, false)?;
        facts.response.provider_response_verified = provider.fixture_response_verified();
        facts.response.provider_generation_count =
            Some(provider.generation_count()).filter(|count| *count <= 4096);
        let bytes = serde_json::to_vec(&facts).map_err(|_| Reason::IsolationUnavailable)?;
        let final_path = output_path.with_extension("closed");
        open_private_new(&final_path)
            .and_then(|mut file| file.write_all(&bytes))
            .and_then(|()| std::fs::rename(final_path, &output_path))
            .map_err(|_| Reason::IsolationUnavailable)?;
        if facts.input.input_submitted {
            result.steps.push(CheckStep::InputSubmitted);
        }
        if !facts.response.response_verified {
            return Err(Reason::ResponseMismatch);
        }
        if !facts.response.provider_response_verified {
            return Err(Reason::ProviderFailed);
        }
        result.steps.push(CheckStep::ResponseVerified);
        Ok(())
    }

    fn run_dom_driver(
        &self,
        driver: &Path,
        request: &Path,
        output: &Path,
        qualification: bool,
    ) -> Result<(), Reason> {
        run_driver(
            driver,
            request,
            output,
            qualification,
            Duration::from_secs(if qualification { 65 } else { 35 }),
            || self.visual.guard(),
        )
    }
}

// Profile preparation and ready observation retain one original watch;
// the outer worker also governs all subsequent turns and cleanup.
fn qualification_timing(windows_ready: bool, deadline: Instant) -> Result<(u32, Duration), Reason> {
    if !windows_ready {
        return Ok((45_000, Duration::from_secs(65)));
    }
    let remaining = deadline.saturating_duration_since(Instant::now());
    let request = remaining
        .checked_sub(Duration::from_secs(5))
        .ok_or(Reason::Timeout)?;
    if request.is_zero() {
        return Err(Reason::Timeout);
    }
    let milliseconds =
        u32::try_from(request.as_millis().min(120_000)).map_err(|_| Reason::Timeout)?;
    if milliseconds == 0 {
        return Err(Reason::Timeout);
    }
    Ok((milliseconds, remaining))
}

fn run_driver(
    driver: &Path,
    request: &Path,
    output: &Path,
    qualification: bool,
    limit: Duration,
    mut guard: impl FnMut() -> Result<(), Reason>,
) -> Result<(), Reason> {
    guard()?;
    let mut child = Command::new("node")
        .arg(driver)
        .arg(if qualification {
            "--qualify"
        } else {
            "--drive"
        })
        .arg(request)
        .arg(output)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| Reason::ActionUnsupported)?;
    let deadline = Instant::now() + limit;
    let outcome = loop {
        match child.try_wait() {
            Ok(Some(_)) => break Ok(()),
            Err(_) => break Err(Reason::ActionUnsupported),
            Ok(None) => {}
        }
        if let Err(reason) = guard() {
            break Err(reason);
        }
        if Instant::now() >= deadline {
            break Err(Reason::Timeout);
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    if outcome.is_err() {
        let _ = child.kill();
        child.wait().map_err(|_| Reason::IsolationUnavailable)?;
    }
    outcome
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cold_readiness_retains_its_watch_and_leaves_driver_cleanup_time() {
        let deadline = Instant::now() + Duration::from_secs(20);
        let (request_ms, limit) = qualification_timing(true, deadline).unwrap();
        assert!(limit <= Duration::from_secs(20));
        assert!(Duration::from_millis(u64::from(request_ms)) + Duration::from_secs(5) <= limit);
        assert_eq!(
            qualification_timing(true, Instant::now()),
            Err(Reason::Timeout)
        );
        assert_eq!(
            qualification_timing(true, Instant::now() + Duration::from_secs(4)),
            Err(Reason::Timeout)
        );
        assert_eq!(
            qualification_timing(false, Instant::now()),
            Ok((45_000, Duration::from_secs(65)))
        );
    }

    use serde_json::json;

    struct ProcessObservation {
        owner: Option<u32>,
        exited: bool,
        unavailable: bool,
    }

    impl crate::process::Observation for ProcessObservation {
        fn id(&self) -> Option<u32> {
            self.owner
        }
        fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
            if self.unavailable {
                return Err(std::io::Error::other("unavailable"));
            }
            if !self.exited {
                return Ok(None);
            }
            #[cfg(unix)]
            {
                use std::os::unix::process::ExitStatusExt;
                Ok(Some(std::process::ExitStatus::from_raw(0)))
            }
            #[cfg(windows)]
            {
                use std::os::windows::process::ExitStatusExt;
                Ok(Some(std::process::ExitStatus::from_raw(0)))
            }
        }
    }

    #[test]
    fn renderer_session_rejects_changed_dead_or_unobservable_owner() {
        for (owner, exited, unavailable, expected) in [
            (Some(42), false, false, Ok(())),
            (Some(43), false, false, Err(Reason::IsolationUnavailable)),
            (None, false, false, Err(Reason::IsolationUnavailable)),
            (Some(42), true, false, Err(Reason::ApplicationExited)),
            (Some(42), false, true, Err(Reason::ActionUnsupported)),
        ] {
            let mut observation = ProcessObservation {
                owner,
                exited,
                unavailable,
            };
            assert_eq!(renderer_guard(&mut observation, 42), expected);
        }
    }

    fn base_facts() -> serde_json::Value {
        json!({"schemaVersion":1,"mechanism":"hermes-playwright-dom","endpointOwned":true,"targetVerified":true,"attached":true,"uniqueComposer":true,"inputReadback":true,"inputSubmitted":false,"responseVerified":false,"syntheticTextPresent":false,"errorCategory":null,"playwrightVersion":"1.61.0","observedRuntimeVersion":"22.0.0","inputCleared":false,"userTurnObserved":false,"assistantTurnCount":0,"uniqueSendControl":false,"canSend":false,"sendBlocker":null,"sendMechanism":"semantic-keyboard","requestFailedCount":0,"requestFailureCategory":null,"apiErrorStatus":null,"apiErrorResponseCount":0})
    }

    fn read(value: &serde_json::Value, qualification: bool) -> Result<Facts, Reason> {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("facts.json");
        open_private_new(&path)
            .unwrap()
            .write_all(&serde_json::to_vec(value).unwrap())
            .unwrap();
        read_facts(&path, qualification)
    }

    fn focus_facts(value: &mut serde_json::Value) {
        for key in [
            "retryFocusAfterAcquire",
            "retryFocusBeforeAction",
            "retryButtonConnected",
            "retryAncestorHidden",
            "retryAncestorInert",
            "retryFieldsetDisabled",
            "retryDocumentFocused",
        ] {
            value[key] = json!(false);
        }
        value["retryHitOwnedPoints"] = json!(0);
        value["retryPointStable"] = json!(false);
        value["retrySampleStatus"] = json!("unmeasured");
        value["retryReveal"] = json!("none");
        value["retryHitAncestor"] = json!(false);
        value["retryHitSharesTurnPair"] = json!(false);
        value["retryHitContainsComposer"] = json!(false);
        value["retryActiveTag"] = json!("unmeasured");
        value["retryActiveRegion"] = json!("unmeasured");
    }

    #[test]
    fn optional_qualification_facts_are_an_atomic_pair() {
        let basic = base_facts();
        assert!(read(&basic, false).is_ok());
        for key in ["errorObserved", "retryControl"] {
            let mut partial = basic.clone();
            partial[key] = json!(false);
            assert!(read(&partial, false).is_err());
            partial["mechanism"] = json!("hermes-renderer-qualification");
            assert!(read(&partial, true).is_err());
        }
        let mut qualifier = basic;
        qualifier["mechanism"] = json!("hermes-renderer-qualification");
        qualifier["errorObserved"] = json!(false);
        qualifier["retryControl"] = json!(false);
        qualifier["retryHitOwned"] = json!(false);
        qualifier["retryRectInViewport"] = json!(false);
        qualifier["retryAncestorClipped"] = json!(false);
        qualifier["retryPointerEventsNone"] = json!(false);
        qualifier["retryHitTag"] = json!("unmeasured");
        qualifier["retryHitRegion"] = json!("unmeasured");
        qualifier["retryHitTarget"] = json!("unmeasured");
        focus_facts(&mut qualifier);
        assert!(read(&qualifier, true).is_ok());
        assert!(read(&qualifier, false).is_err());
        qualifier["retryControl"] = serde_json::Value::Null;
        assert!(read(&qualifier, true).is_err());
    }

    #[test]
    fn pointer_retry_requires_the_owned_control_at_the_hit_point() {
        let mut value = base_facts();
        value["mechanism"] = json!("hermes-renderer-qualification");
        value["errorObserved"] = json!(true);
        value["retryControl"] = json!(true);
        value["retryHitOwned"] = json!(true);
        value["retryRectInViewport"] = json!(false);
        value["retryAncestorClipped"] = json!(false);
        value["retryPointerEventsNone"] = json!(false);
        value["retryHitTag"] = json!("unmeasured");
        value["retryHitRegion"] = json!("unmeasured");
        value["retryHitTarget"] = json!("self");
        value["inputSubmitted"] = json!(true);
        value["sendMechanism"] = json!("pointer");
        focus_facts(&mut value);
        value["retryHitOwnedPoints"] = json!(1);
        value["retryPointStable"] = json!(true);
        value["retrySampleStatus"] = json!("owned");
        value["retryRectInViewport"] = json!(true);
        assert!(read(&value, true).is_ok());
        for key in ["retryPointStable", "retryRectInViewport"] {
            value[key] = json!(false);
            assert!(read(&value, true).is_err());
            value[key] = json!(true);
        }
        for status in ["no-owned-point", "hidden", "unmeasured", "PRIVATE"] {
            value["retrySampleStatus"] = json!(status);
            assert!(read(&value, true).is_err());
        }
        value["retrySampleStatus"] = json!("owned");
        for count in [0, 10] {
            value["retryHitOwnedPoints"] = json!(count);
            assert!(read(&value, true).is_err());
        }
        value["retryHitOwnedPoints"] = json!(1);
        for target in ["composer", "error-card", "other", "unmeasured", "PRIVATE"] {
            value["retryHitTarget"] = json!(target);
            assert!(read(&value, true).is_err());
        }
        value["retryHitTarget"] = json!("self");
        value["retryHitOwned"] = json!(false);
        assert!(read(&value, true).is_err());
    }

    #[test]
    fn retry_focus_proof_requires_both_observations_and_closed_categories() {
        let mut value = base_facts();
        focus_facts(&mut value);
        assert!(read(&value, false).is_err());
        let mut focus: RetryFocusFacts = serde_json::from_value(value.clone()).unwrap();
        assert!(focus.complete());
        assert!(!focus.verified());
        focus.after_acquire = Some(true);
        assert!(!focus.verified());
        focus.before_action = Some(true);
        focus.button_connected = Some(true);
        assert!(focus.verified());
        focus.ancestor_hidden = Some(true);
        assert!(!focus.verified());
        value["retryActiveRegion"] = json!("PRIVATE");
        assert!(serde_json::from_value::<RetryFocusFacts>(value).is_err());
    }
}
