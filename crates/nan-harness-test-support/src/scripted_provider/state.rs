use serde_json::Value;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::atomic::{AtomicBool, AtomicUsize};

use super::ProviderScenario;

pub(super) const CONFORMANCE_TOOL_CALL_ID_PREFIX: &str = "call_nan_harness_conformance";
pub(super) const STRUCTURED_HELPER_TOOL_CALL_ID: &str = "call_nan_harness_structured_helper";
const MAX_RECORDED_REQUESTS: usize = 128;
const MAX_RECORDED_REQUEST_BYTES: usize = 256 * 1024;

#[derive(Debug)]
pub(super) struct ProviderState {
    scenario: ProviderScenario,
    fixture_url: String,
    chat_requests: Mutex<Vec<Value>>,
    search_requests: Mutex<Vec<Value>>,
    model_requests: AtomicUsize,
    progress: Mutex<ScriptProgress>,
    recording_overflow: AtomicBool,
}

impl ProviderState {
    pub(super) fn new(scenario: ProviderScenario, fixture_url: String) -> Self {
        Self {
            scenario,
            fixture_url,
            chat_requests: Mutex::new(Vec::new()),
            search_requests: Mutex::new(Vec::new()),
            model_requests: AtomicUsize::new(0),
            progress: Mutex::new(ScriptProgress::default()),
            recording_overflow: AtomicBool::new(false),
        }
    }

    pub(super) fn scenario(&self) -> &ProviderScenario {
        &self.scenario
    }

    pub(super) fn fixture_url(&self) -> &str {
        &self.fixture_url
    }

    pub(super) fn chat_requests(&self) -> Vec<Value> {
        self.chat_requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub(super) fn search_requests(&self) -> Vec<Value> {
        self.search_requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub(super) fn model_requests(&self) -> usize {
        self.model_requests
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub(super) fn completed(&self) -> bool {
        self.progress().completed
    }

    pub(super) fn recording_bounded(&self) -> bool {
        !self
            .recording_overflow
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub(super) fn progress(&self) -> MutexGuard<'_, ScriptProgress> {
        self.progress
            .lock()
            .expect("scripted provider progress lock should not be poisoned")
    }

    pub(super) fn record_chat_request(&self, body: &Value) {
        let request_is_bounded = body.to_string().len() <= MAX_RECORDED_REQUEST_BYTES;
        let mut requests = self
            .chat_requests
            .lock()
            .expect("scripted provider chat request lock should not be poisoned");
        if requests.len() >= MAX_RECORDED_REQUESTS || !request_is_bounded {
            self.recording_overflow
                .store(true, std::sync::atomic::Ordering::Relaxed);
        } else {
            requests.push(body.clone());
        }
    }

    pub(super) fn record_search_request(&self, body: Value) {
        let mut requests = self
            .search_requests
            .lock()
            .expect("scripted provider search request lock should not be poisoned");
        if requests.len() >= MAX_RECORDED_REQUESTS
            || body.to_string().len() > MAX_RECORDED_REQUEST_BYTES
        {
            self.recording_overflow
                .store(true, std::sync::atomic::Ordering::Relaxed);
        } else {
            requests.push(body);
        }
    }

    pub(super) fn record_model_request(&self) {
        self.model_requests
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

#[derive(Debug, Default)]
pub(super) struct ScriptProgress {
    pub(super) index: usize,
    pub(super) emitted: bool,
    pub(super) result_identifiers: Vec<String>,
    pub(super) completed: bool,
}
