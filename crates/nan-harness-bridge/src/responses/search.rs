use crate::error::ApiError;
use crate::search_service::{self, SearchRequest};
use crate::upstream::NanClient;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Mutex;

const MAX_SESSIONS: usize = 256;
const MAX_REFERENCES_PER_SESSION: usize = 64;
const MAX_SESSION_ID_BYTES: usize = 256;
const MAX_STORED_URL_BYTES: usize = 8 * 1024;

#[derive(Debug, Default)]
pub(crate) struct SearchReferences {
    state: Mutex<SearchReferenceState>,
}

#[derive(Debug, Default)]
struct SearchReferenceState {
    sessions: BTreeMap<String, SessionReferences>,
    next_generation: u64,
}

#[derive(Debug, Default)]
struct SessionReferences {
    urls: BTreeMap<String, String>,
    last_used: u64,
}

pub(crate) async fn execute(
    client: &NanClient,
    references: &SearchReferences,
    request: Value,
) -> Result<Value, ApiError> {
    let session_id = request_session_id(&request);
    let query = search_query(&request, references);
    let count = result_count(&request);
    let allowed_domains = allowed_domains(&request);
    let results = search_service::execute(
        client,
        SearchRequest {
            query,
            max_results: count,
            allowed_domains,
            blocked_domains: Vec::new(),
        },
    )
    .await?;
    let structured = results
        .iter()
        .enumerate()
        .map(|(index, result)| {
            let reference = format!("turn0search{index}");
            if let Some(session_id) = session_id {
                references.insert(session_id, &reference, &result.url);
            }
            json!({
                "type": "text_result",
                "ref_id": reference,
                "url": result.url,
                "title": result.title,
                "snippet": result.snippet
            })
        })
        .collect::<Vec<_>>();
    let output = search_service::result_summary(&results);
    Ok(json!({
        "encrypted_output": null,
        "output": output,
        "results": structured
    }))
}

fn search_query(request: &Value, references: &SearchReferences) -> String {
    let commands = request.get("commands").unwrap_or(&Value::Null);
    for key in ["search_query", "image_query"] {
        let queries = commands
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|query| query.get("q").and_then(Value::as_str))
            .collect::<Vec<_>>();
        if !queries.is_empty() {
            return queries.join(" OR ");
        }
    }
    for key in ["open", "find", "click", "screenshot"] {
        if let Some(reference) = commands
            .get(key)
            .and_then(Value::as_array)
            .and_then(|operations| operations.first())
            .and_then(|operation| operation.get("ref_id"))
            .and_then(Value::as_str)
        {
            return references
                .resolve(request_session_id(request), reference)
                .unwrap_or_else(|| reference.to_owned());
        }
    }
    commands.to_string()
}

fn request_session_id(request: &Value) -> Option<&str> {
    request
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty() && id.len() <= MAX_SESSION_ID_BYTES)
}

impl SearchReferences {
    fn insert(&self, session_id: &str, reference: &str, url: &str) {
        if !valid_session_id(session_id) || url.len() > MAX_STORED_URL_BYTES {
            return;
        }

        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let generation = state.next_generation();
        if !state.sessions.contains_key(session_id)
            && state.sessions.len() >= MAX_SESSIONS
            && let Some(oldest) = state
                .sessions
                .iter()
                .min_by_key(|(_, session)| session.last_used)
                .map(|(id, _)| id.clone())
        {
            state.sessions.remove(&oldest);
        }

        let session = state.sessions.entry(session_id.to_owned()).or_default();
        session.last_used = generation;
        if session.urls.len() >= MAX_REFERENCES_PER_SESSION && !session.urls.contains_key(reference)
        {
            session.urls.pop_first();
        }
        session.urls.insert(reference.to_owned(), url.to_owned());
    }

    fn resolve(&self, session_id: Option<&str>, reference: &str) -> Option<String> {
        let session_id = session_id.filter(|id| valid_session_id(id))?;
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let generation = state.next_generation();
        let session = state.sessions.get_mut(session_id)?;
        session.last_used = generation;
        session.urls.get(reference).cloned()
    }
}

impl SearchReferenceState {
    fn next_generation(&mut self) -> u64 {
        self.next_generation = self.next_generation.saturating_add(1);
        self.next_generation
    }
}

fn valid_session_id(session_id: &str) -> bool {
    !session_id.is_empty() && session_id.len() <= MAX_SESSION_ID_BYTES
}

fn result_count(request: &Value) -> usize {
    match request
        .pointer("/commands/response_length")
        .and_then(Value::as_str)
    {
        Some("long") => 20,
        Some("medium") => 10,
        _ => 5,
    }
}

fn allowed_domains(request: &Value) -> Vec<String> {
    request
        .pointer("/settings/filters/allowed_domains")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{MAX_REFERENCES_PER_SESSION, MAX_SESSIONS, SearchReferences, search_query};
    use serde_json::json;
    use std::sync::{Arc, Barrier};

    #[test]
    fn references_are_scoped_by_request_id() {
        let references = SearchReferences::default();
        references.insert(
            "session-a",
            "turn0search0",
            "https://example.test/session-a",
        );
        references.insert(
            "session-b",
            "turn0search0",
            "https://example.test/session-b",
        );

        let open = |id| {
            json!({
                "id": id,
                "commands": {"open": [{"ref_id": "turn0search0"}]}
            })
        };
        assert_eq!(
            search_query(&open("session-a"), &references),
            "https://example.test/session-a"
        );
        assert_eq!(
            search_query(&open("session-b"), &references),
            "https://example.test/session-b"
        );
        assert_eq!(search_query(&open("unknown"), &references), "turn0search0");
        assert_eq!(
            search_query(
                &json!({"commands": {"open": [{"ref_id": "turn0search0"} ]}}),
                &references,
            ),
            "turn0search0"
        );
    }

    #[test]
    fn concurrent_sessions_do_not_overwrite_same_reference() {
        let references = Arc::new(SearchReferences::default());
        let barrier = Arc::new(Barrier::new(3));
        let first_references = Arc::clone(&references);
        let first_barrier = Arc::clone(&barrier);
        let first = std::thread::spawn(move || {
            first_barrier.wait();
            first_references.insert(
                "session-a",
                "turn0search0",
                "https://example.test/session-a",
            );
        });
        let second_references = Arc::clone(&references);
        let second_barrier = Arc::clone(&barrier);
        let second = std::thread::spawn(move || {
            second_barrier.wait();
            second_references.insert(
                "session-b",
                "turn0search0",
                "https://example.test/session-b",
            );
        });
        barrier.wait();
        first.join().expect("first session should finish");
        second.join().expect("second session should finish");

        assert_eq!(
            references
                .resolve(Some("session-a"), "turn0search0")
                .as_deref(),
            Some("https://example.test/session-a")
        );
        assert_eq!(
            references
                .resolve(Some("session-b"), "turn0search0")
                .as_deref(),
            Some("https://example.test/session-b")
        );
    }

    #[test]
    fn old_sessions_are_evicted_when_the_store_reaches_its_bound() {
        let references = SearchReferences::default();
        for index in 0..MAX_SESSIONS {
            references.insert(
                &format!("session-{index}"),
                "turn0search0",
                "https://example.test/result",
            );
        }
        assert!(
            references
                .resolve(Some("session-0"), "turn0search0")
                .is_some()
        );
        references.insert("session-new", "turn0search0", "https://example.test/new");
        assert!(
            references
                .resolve(Some("session-1"), "turn0search0")
                .is_none()
        );
        assert_eq!(
            references
                .resolve(Some("session-new"), "turn0search0")
                .as_deref(),
            Some("https://example.test/new")
        );
    }

    #[test]
    fn references_per_session_are_bounded() {
        let references = SearchReferences::default();
        for index in 0..MAX_REFERENCES_PER_SESSION {
            references.insert(
                "session-a",
                &format!("turn0search{index}"),
                "https://example.test/result",
            );
        }
        references.insert(
            "session-a",
            &format!("turn0search{MAX_REFERENCES_PER_SESSION}"),
            "https://example.test/new",
        );

        assert!(
            references
                .resolve(Some("session-a"), "turn0search0")
                .is_none()
        );
        assert_eq!(
            references
                .resolve(
                    Some("session-a"),
                    &format!("turn0search{MAX_REFERENCES_PER_SESSION}"),
                )
                .as_deref(),
            Some("https://example.test/new")
        );
    }
}
