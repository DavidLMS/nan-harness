//! Hosted renderer experiment with one input owner and no OCR fallback.

use super::Gui;
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

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct QualificationFacts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error_observed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    retry_control: Option<bool>,
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

fn read_facts(path: &Path, qualification: bool) -> Result<Facts, Reason> {
    const KEYS: &[&str] = &[
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
        "errorObserved",
        "retryControl",
    ];
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
    let count = if qualification { 26..=28 } else { 24..=26 };
    if !count.contains(&object.len()) || object.keys().any(|key| !KEYS.contains(&key.as_str())) {
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
        || !matches!(
            (
                qualification,
                facts.qualification.error_observed,
                facts.qualification.retry_control
            ),
            (false, None, None) | (true, Some(_), Some(_))
        )
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

impl Gui {
    pub(crate) fn qualify_dom_turn(
        &self,
        directory: &Path,
        owner: u32,
        turn: DomTurn<'_>,
        provider: &ProviderGate,
    ) -> Result<(), Reason> {
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
        let stem = format!("qualification-{owner}-{}", u64::from_le_bytes(nonce));
        let request_path = directory.join(format!("{stem}.private"));
        let output_path = directory.join(format!("{stem}.json"));
        let request = QualificationRequest {
            request: Request {
                connection_path: directory.join(format!("connection-{owner}.json")),
                owner_pid: owner,
                prompt: turn.prompt,
                expected_marker: turn.marker,
                timeout_ms: 30_000,
            },
            action: turn.action,
            purpose: turn.purpose,
        };
        let bytes = serde_json::to_vec(&request).map_err(|_| Reason::IsolationUnavailable)?;
        open_private_new(&request_path)
            .and_then(|mut file| file.write_all(&bytes))
            .map_err(|_| Reason::IsolationUnavailable)?;
        let outcome = self.run_dom_driver(&driver, &request_path, &output_path, true);
        std::fs::remove_file(&request_path).map_err(|_| Reason::IsolationUnavailable)?;
        outcome?;
        let mut facts = read_facts(&output_path, true)?;
        facts.response.provider_response_verified = provider.fixture_response_verified();
        facts.response.provider_generation_count =
            Some(provider.generation_count()).filter(|count| *count <= 4096);
        let bytes = serde_json::to_vec(&facts).map_err(|_| Reason::IsolationUnavailable)?;
        let final_path = output_path.with_extension("closed");
        open_private_new(&final_path)
            .and_then(|mut file| file.write_all(&bytes))
            .and_then(|()| std::fs::rename(final_path, &output_path))
            .map_err(|_| Reason::IsolationUnavailable)?;
        if !facts.input.input_submitted || facts.error_category.is_some() {
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

    pub(crate) fn probe_dom(
        &self,
        directory: &Path,
        owner: u32,
        marker: &str,
        result: &mut ProbeResult,
        provider: &ProviderGate,
    ) -> Result<(), Reason> {
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
        let deadline = Instant::now() + Duration::from_secs(35);
        let outcome = loop {
            match child.try_wait() {
                Ok(Some(_)) => break Ok(()),
                Err(_) => break Err(Reason::ActionUnsupported),
                Ok(None) => {}
            }
            if let Err(reason) = self.visual.guard() {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
        assert!(read(&qualifier, true).is_ok());
        assert!(read(&qualifier, false).is_err());
        qualifier["retryControl"] = serde_json::Value::Null;
        assert!(read(&qualifier, true).is_err());
    }
}
