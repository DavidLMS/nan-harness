//! Hosted-only semantic experiment. No visual fallback or raw tree persistence.

use super::{
    Gui, InputAccessibilityObservation, WAIT, map_error, primary_modifier, response_selector,
};
use crate::provider::ProviderGate;
use crate::report::{CheckStep, InputMode, ProbeResult, Reason, ResponseVerification};
use nan_harness_private_fs::open_private_new;
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::Path;
use std::time::{Duration, Instant};
use xa11y::{App, Locator, Role};

const MAX_ELEMENTS: usize = 4096;
const COMPOSER: &str = "text_area[name=\"Message Editor\"], text_field[name=\"Message Editor\"], text_area[name=\"Message\"], text_field[name=\"Message\"], text_area[name=\"Ask anything\"], text_field[name=\"Ask anything\"], text_area[description=\"Message Editor\"], text_field[description=\"Message Editor\"], text_area[description=\"Message\"], text_field[description=\"Message\"], text_area[description=\"Ask anything\"], text_field[description=\"Ask anything\"]";
const EDITABLE: &str = "text_area[editable=\"true\"], text_field[editable=\"true\"]";
const PROMPT: &str = "Check this connection";

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Stage {
    InitialInventory,
    TrustControl,
    AgentPanel,
    AfterPanel,
    BeforeKeyboard,
    AfterKeyboard,
    Response,
    Completed,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Readback {
    NoMatchingControl,
    ReadableEmptyValue,
    ReadableNonmatchingValue,
    ValueReadUnavailable,
    QueryFailed,
    ValueMatches,
}

fn input_observation(element: Option<&xa11y::ElementData>) -> Readback {
    match super::input_readback_observation(element, PROMPT) {
        Ok(()) => Readback::ValueMatches,
        Err(InputAccessibilityObservation::NoMatchingControl) => Readback::NoMatchingControl,
        Err(InputAccessibilityObservation::ReadableEmptyValue) => Readback::ReadableEmptyValue,
        Err(InputAccessibilityObservation::ReadableNonmatchingValue) => {
            Readback::ReadableNonmatchingValue
        }
        Err(InputAccessibilityObservation::ValueReadUnavailable) => Readback::ValueReadUnavailable,
        Err(InputAccessibilityObservation::QueryFailed) => Readback::QueryFailed,
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Inventory {
    stage: Stage,
    role_counts: Option<BTreeMap<&'static str, u32>>,
    roles_error: Option<Reason>,
    named_composer_count: Option<u32>,
    named_composer_error: Option<Reason>,
    editable_count: Option<u32>,
    editable_error: Option<Reason>,
    value_readback: Readback,
    response_matches: Option<u32>,
    response_error: Option<Reason>,
}

#[derive(Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct InputFacts {
    keyboard_entered: bool,
    semantic_input_verified: bool,
}

#[derive(Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct ResponseFacts {
    semantic_response_verified: bool,
    provider_response_verified: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Facts {
    schema_version: u8,
    mechanism: &'static str,
    experiment_only: bool,
    no_ocr_qualification: bool,
    app_by_pid: bool,
    app_error: Option<Reason>,
    stage: Stage,
    trust_control_count: Option<u32>,
    trust_control_error: Option<Reason>,
    blocker: Option<Reason>,
    #[serde(flatten)]
    input: InputFacts,
    #[serde(flatten)]
    response: ResponseFacts,
    inventories: Vec<Inventory>,
}

fn bounded_count(count: usize) -> Result<u32, Reason> {
    if count > MAX_ELEMENTS {
        return Err(Reason::ActionUnsupported);
    }
    u32::try_from(count).map_err(|_| Reason::ActionUnsupported)
}

fn query_count(app: &App, selector: &str) -> (Option<u32>, Option<Reason>) {
    match app
        .locator(selector)
        .count()
        .map_err(map_error)
        .and_then(bounded_count)
    {
        Ok(count) => (Some(count), None),
        Err(reason) => (None, Some(reason)),
    }
}

fn role_counts(
    roles: impl IntoIterator<Item = Role>,
) -> Result<BTreeMap<&'static str, u32>, Reason> {
    let mut counts = BTreeMap::new();
    for (index, role) in roles.into_iter().enumerate() {
        bounded_count(index + 1)?;
        *counts.entry(role.to_snake_case()).or_insert(0) += 1;
    }
    Ok(counts)
}

fn inventory(app: &App, stage: Stage, marker: &str) -> Inventory {
    let roles = app
        .locator("*")
        .elements()
        .map_err(map_error)
        .and_then(|elements| role_counts(elements.iter().map(|element| element.role)));
    let (role_counts, roles_error) = match roles {
        Ok(counts) => (Some(counts), None),
        Err(reason) => (None, Some(reason)),
    };
    let (named_composer_count, named_composer_error) = query_count(app, COMPOSER);
    let (editable_count, editable_error) = query_count(app, EDITABLE);
    let (response_matches, response_error) = match response_selector(marker) {
        Ok(selector) => query_count(app, &selector),
        Err(reason) => (None, Some(reason)),
    };
    let field = app.locator(EDITABLE);
    let value_readback = if editable_count == Some(1) {
        match field.element() {
            Ok(element) => input_observation(Some(element.data())),
            Err(_) => Readback::QueryFailed,
        }
    } else {
        Readback::NoMatchingControl
    };
    Inventory {
        stage,
        role_counts,
        roles_error,
        named_composer_count,
        named_composer_error,
        editable_count,
        editable_error,
        value_readback,
        response_matches,
        response_error,
    }
}

impl Gui {
    /// The normal compatibility suite must not treat this partial experiment as a pass.
    pub(crate) fn probe_accessibility(
        &self,
        directory: &Path,
        marker: &str,
        result: &mut ProbeResult,
        provider: &ProviderGate,
    ) -> Result<(), Reason> {
        let mut facts = Facts {
            schema_version: 1,
            mechanism: "zed-native-accessibility",
            experiment_only: true,
            no_ocr_qualification: false,
            app_by_pid: self.app.is_some(),
            app_error: self.app_error,
            stage: Stage::InitialInventory,
            trust_control_count: None,
            trust_control_error: None,
            blocker: None,
            input: InputFacts::default(),
            response: ResponseFacts::default(),
            inventories: Vec::new(),
        };
        let outcome = self.run_accessibility_probe(&mut facts, marker, result);
        facts.blocker = outcome.err();
        facts.response.provider_response_verified = provider.response_verified();
        let bytes = serde_json::to_vec(&facts).map_err(|_| Reason::IsolationUnavailable)?;
        let mut nonce = [0u8; 8];
        getrandom::fill(&mut nonce).map_err(|_| Reason::IsolationUnavailable)?;
        let name = format!("{}-{}.json", std::process::id(), u64::from_le_bytes(nonce));
        open_private_new(&directory.join(name))
            .and_then(|mut file| file.write_all(&bytes).and_then(|()| file.sync_all()))
            .map_err(|_| Reason::IsolationUnavailable)?;
        outcome
    }

    fn settled_accessible_input(&self) -> Result<Locator, Reason> {
        let deadline = Instant::now() + WAIT;
        loop {
            self.visual.guard()?;
            match self.input() {
                Ok(field) => return Ok(field),
                Err(failure) if failure.reason == Reason::SelectorNotMatched => {
                    if Instant::now() >= deadline {
                        return Err(failure.reason);
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(failure) => return Err(failure.reason),
            }
        }
    }

    fn run_accessibility_probe(
        &self,
        facts: &mut Facts,
        marker: &str,
        result: &mut ProbeResult,
    ) -> Result<(), Reason> {
        let app = self
            .app
            .as_ref()
            .ok_or(self.app_error.unwrap_or(Reason::SelectorNotMatched))?;
        facts
            .inventories
            .push(inventory(app, Stage::InitialInventory, marker));
        facts.stage = Stage::TrustControl;
        // Never confirm an unidentified modal. The panel chord cannot confirm it.
        let trust = app.locator("button[name=\"Trust and Continue\"]");
        let (count, error) = query_count(app, "button[name=\"Trust and Continue\"]");
        facts.trust_control_count = count;
        facts.trust_control_error = error;
        match count {
            Some(1) => {
                self.visual.guard()?;
                trust.press().map_err(map_error)?;
                trust.wait_hidden(WAIT).map_err(map_error)?;
            }
            Some(0) => {}
            Some(_) => return Err(Reason::SelectorNotMatched),
            None => return Err(error.unwrap_or(Reason::ActionUnsupported)),
        }
        facts.stage = Stage::AgentPanel;
        self.visual.guard()?;
        self.require_owned_foreground()?;
        xa11y::input_sim()
            .map_err(map_error)?
            .keyboard()
            .chord(
                xa11y::Key::Char('/'),
                &[primary_modifier(), xa11y::Key::Shift],
            )
            .map_err(map_error)?;
        let field = self.settled_accessible_input();
        facts.stage = Stage::AfterPanel;
        facts
            .inventories
            .push(inventory(app, Stage::AfterPanel, marker));
        let field = field?;
        facts.stage = Stage::BeforeKeyboard;
        facts
            .inventories
            .push(inventory(app, Stage::BeforeKeyboard, marker));
        field.wait_visible(WAIT).map_err(map_error)?;
        self.keyboard_fill(&field, PROMPT)
            .map_err(|(_, reason, _)| reason)?;
        facts.input.keyboard_entered = true;
        facts.stage = Stage::AfterKeyboard;
        let observation = readback(&field);
        facts
            .inventories
            .push(inventory(app, Stage::AfterKeyboard, marker));
        facts.input.semantic_input_verified = observation == Readback::ValueMatches;
        if !facts.input.semantic_input_verified {
            return Err(Reason::InputMismatch);
        }
        result.record_input(InputMode::AccessibilityAndKeyboard);
        self.send(&field).map_err(|(_, reason, _)| reason)?;
        result.steps.push(CheckStep::InputSubmitted);
        facts.stage = Stage::Response;
        let selector = response_selector(marker)?;
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            self.visual.guard()?;
            if app.locator(&selector).count().map_err(map_error)? > 0 {
                facts.response.semantic_response_verified = true;
                result.record_response(ResponseVerification::Accessibility);
                result.steps.push(CheckStep::ResponseVerified);
                break;
            }
            if Instant::now() >= deadline {
                facts
                    .inventories
                    .push(inventory(app, Stage::Response, marker));
                return Err(Reason::ResponseMismatch);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        facts
            .inventories
            .push(inventory(app, Stage::Response, marker));
        facts.stage = Stage::Completed;
        Ok(())
    }
}

fn readback(field: &Locator) -> Readback {
    let observation = std::cell::Cell::new(Readback::QueryFailed);
    let _ = field.wait_until(
        |element| {
            observation.set(input_observation(element));
            observation.get() == Readback::ValueMatches
        },
        WAIT,
    );
    observation.get()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inventories_aggregate_roles_without_text_or_platform_attributes() {
        let counts =
            role_counts([Role::Window, Role::TextArea, Role::TextArea, Role::Unknown]).unwrap();
        let value = serde_json::to_value(counts).unwrap();
        assert_eq!(
            value,
            serde_json::json!({"window":1,"text_area":2,"unknown":1})
        );
    }

    #[test]
    fn inventory_limits_and_query_failure_are_distinct_from_zero_matches() {
        assert_eq!(bounded_count(0), Ok(0));
        assert_eq!(
            bounded_count(MAX_ELEMENTS + 1),
            Err(Reason::ActionUnsupported)
        );
        assert_eq!(
            role_counts(std::iter::repeat_n(Role::Button, MAX_ELEMENTS + 1)),
            Err(Reason::ActionUnsupported)
        );
    }
}
