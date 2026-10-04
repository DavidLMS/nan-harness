//! Private causal authority: three submitted prompts and unique verified-turn contexts.
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use zeroize::Zeroizing;

#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum AuthorityRejection {
    ArmedUnobserved,
    ContextUnobserved,
    ContextChanged,
    PriorContextIncomplete,
    Policy,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FailureAuthorityObservation {
    status: AuthorityRejection,
    prepared_turns: u8,
    learned_turns: u8,
    rejected_stream: u16,
    rejected_history: u16,
    rejected_context: u16,
}
#[derive(Default)]
pub(super) struct FailureTurnAuthority {
    epoch: u64,
    prompts: Vec<Zeroizing<String>>,
    contexts: [Option<MainContext>; 2],
    context_ambiguous: bool,
    context_turns: u8,
    failure_armed: bool,
    fixture_tools: FixtureToolAuthority,
    observed: bool,
    rejected_stream: u16,
    rejected_history: u16,
    rejected_context: u16,
}
#[derive(Default, Clone, Copy)]
enum FixtureToolAuthority {
    #[default]
    Unobserved,
    Learned([u8; 32]),
    Ambiguous,
    Authorized([u8; 32]),
}
#[derive(Clone, Copy, PartialEq, Eq)]
struct MainContext {
    model: [u8; 32],
    instructions: [u8; 32],
}
impl FailureTurnAuthority {
    pub(super) fn prepare(
        &mut self,
        prompt: &str,
        failure: bool,
        live: bool,
    ) -> Result<Option<u64>, ()> {
        if live
            || prompt.is_empty()
            || prompt.len() > 1024
            || prompt.contains('\0')
            || self.failure_armed
            || self.prompts.len() >= 3
            || self
                .prompts
                .iter()
                .any(|earlier| earlier.as_str() == prompt)
            || failure != (self.prompts.len() == 2)
        {
            return Err(());
        }
        if failure
            && (self.contexts.iter().any(Option::is_none)
                || self.context_ambiguous
                || self.context_turns != 3)
        {
            return Err(());
        }
        if failure {
            self.epoch = self.epoch.checked_add(1).ok_or(())?;
        }
        self.prompts.push(Zeroizing::new(prompt.to_owned()));
        self.failure_armed = failure;
        self.observed = false;
        Ok(failure.then_some(self.epoch))
    }
    pub(super) fn rejection_observation(&self) -> FailureAuthorityObservation {
        FailureAuthorityObservation {
            status: if self.failure_armed && !self.observed {
                AuthorityRejection::ArmedUnobserved
            } else if self.contexts.iter().all(Option::is_none) {
                AuthorityRejection::ContextUnobserved
            } else if self.context_ambiguous {
                AuthorityRejection::ContextChanged
            } else if self.context_turns != 3 {
                AuthorityRejection::PriorContextIncomplete
            } else {
                AuthorityRejection::Policy
            },
            prepared_turns: u8::try_from(self.prompts.len()).unwrap_or(u8::MAX),
            learned_turns: u8::try_from(self.context_turns.count_ones()).unwrap_or(u8::MAX),
            rejected_stream: self.rejected_stream,
            rejected_history: self.rejected_history,
            rejected_context: self.rejected_context,
        }
    }
    pub(super) fn authorize_fixture_context(&mut self) -> Result<(), ()> {
        if self.prompts.len() != 2
            || self.failure_armed
            || self.observed
            || self.context_ambiguous
            || self.context_turns != 3
            || !matches!(self.fixture_tools, FixtureToolAuthority::Learned(_))
        {
            return Err(());
        }
        let FixtureToolAuthority::Learned(tools) = self.fixture_tools else {
            return Err(());
        };
        self.fixture_tools = FixtureToolAuthority::Authorized(tools);
        Ok(())
    }
    pub(super) fn armed(&self) -> bool {
        self.failure_armed
    }
    pub(super) fn learn_context(&mut self, body: &Value) {
        if self.failure_armed || self.prompts.is_empty() {
            return;
        }
        // Count only a request containing the exact current prepared user text.
        // Unrelated background/title traffic cannot alter these diagnostics.
        let relevant = current_user_candidate(body, &self.prompts);
        if !relevant {
            return;
        }
        if body.get("stream") != Some(&Value::Bool(true)) {
            self.rejected_stream = self.rejected_stream.saturating_add(1).min(4096);
            return;
        }
        if !matches_history(body, &self.prompts) {
            self.rejected_history = self.rejected_history.saturating_add(1).min(4096);
            return;
        }
        let Some(context) = main_context(body) else {
            self.context_ambiguous = true;
            self.rejected_context = self.rejected_context.saturating_add(1).min(4096);
            return;
        };
        if self.prompts.len() == 2 {
            self.fixture_tools = match (self.fixture_tools, fixture_tools(body)) {
                (FixtureToolAuthority::Unobserved, Some(tools)) => {
                    FixtureToolAuthority::Learned(tools)
                }
                (FixtureToolAuthority::Learned(held), Some(tools)) if held == tools => {
                    FixtureToolAuthority::Learned(held)
                }
                _ => FixtureToolAuthority::Ambiguous,
            };
        }
        let turn = self.prompts.len() - 1;
        self.context_turns |= 1 << turn;
        // Each verified turn must have one unambiguous instruction context and
        // the same routed model. A later turn can legitimately acquire new tool
        // instructions; failure must still match the most recent verified turn.
        if self.contexts[turn].is_some_and(|held| held != context)
            || self
                .contexts
                .iter()
                .flatten()
                .any(|held| held.model != context.model)
        {
            self.context_ambiguous = true;
            self.rejected_context = self.rejected_context.saturating_add(1).min(4096);
        } else {
            self.contexts[turn] = Some(context);
        }
    }
    pub(super) fn observe(&mut self, body: &Value) -> bool {
        if !self.failure_armed || self.observed || self.context_ambiguous {
            return false;
        }
        if !current_user_candidate(body, &self.prompts) {
            return false;
        }
        if body.get("stream") != Some(&Value::Bool(true)) {
            self.rejected_stream = self.rejected_stream.saturating_add(1).min(4096);
            return false;
        }
        if !matches_history(body, &self.prompts) {
            self.rejected_history = self.rejected_history.saturating_add(1).min(4096);
            return false;
        }
        let context_matches = if let FixtureToolAuthority::Authorized(tools) = self.fixture_tools {
            fixture_tools(body) == Some(tools)
                && main_context(body)
                    .zip(self.contexts[1])
                    .is_some_and(|(current, held)| current.model == held.model)
        } else {
            main_context(body) == self.contexts[1]
        };
        if !context_matches {
            self.rejected_context = self.rejected_context.saturating_add(1).min(4096);
            return false;
        }
        self.observed = true;
        true
    }
    pub(super) fn observed(&self, epoch: u64) -> bool {
        self.epoch == epoch && self.failure_armed && self.observed
    }
}
// Attribute rejection counts only to the bounded exact prepared user candidate.
// Its presence is diagnostic relevance, never authority to inject an error.
fn current_user_candidate(body: &Value, prompts: &[Zeroizing<String>]) -> bool {
    let Some(prompt) = prompts.last() else {
        return false;
    };
    body.get("messages")
        .and_then(Value::as_array)
        .is_some_and(|messages| {
            messages.len() <= 512
                && messages.iter().any(|message| {
                    message.get("role").and_then(Value::as_str) == Some("user")
                        && message.get("content").and_then(exact_text) == Some(prompt.as_str())
                })
        })
}
// Exact bounded OpenAI tool definitions from the already verified main turn.
// This fingerprint never authorizes a request without history/nonce/stream/model.
fn fixture_tools(body: &Value) -> Option<[u8; 32]> {
    let tools = body.get("tools")?.as_array()?;
    if tools.is_empty() || tools.len() > 128 {
        return None;
    }
    let mut names = std::collections::HashSet::new();
    let mut fixture = false;
    for tool in tools {
        let object = tool.as_object()?;
        if object.len() != 2 || tool.get("type")?.as_str()? != "function" {
            return None;
        }
        let function = tool.get("function")?.as_object()?;
        if function.keys().any(|key| {
            !matches!(
                key.as_str(),
                "name" | "description" | "parameters" | "strict"
            )
        }) {
            return None;
        }
        let name = function.get("name")?.as_str()?;
        if name.is_empty() || name.len() > 128 || !names.insert(name) {
            return None;
        }
        if function
            .get("description")
            .is_some_and(|value| value.as_str().is_none_or(|text| text.len() > 8192))
            || function
                .get("strict")
                .is_some_and(|value| !value.is_boolean())
        {
            return None;
        }
        let parameters = function.get("parameters")?.as_object()?;
        if parameters.get("type")?.as_str()? != "object" {
            return None;
        }
        if name == "mcp__nanh-read-fixture__read_file" {
            let properties = parameters.get("properties")?.as_object()?;
            if properties.len() != 1
                || properties.get("path")?.get("type")?.as_str()? != "string"
                || parameters.get("required")?.as_array()? != &[Value::String("path".into())]
            {
                return None;
            }
            fixture = true;
        }
    }
    let serialized = Zeroizing::new(serde_json::to_vec(tools).ok()?);
    if !fixture || serialized.len() > 131_072 {
        return None;
    }
    Some(Sha256::digest(serialized.as_slice()).into())
}
fn exact_text(value: &Value) -> Option<&str> {
    match value {
        Value::String(text) => Some(text),
        Value::Array(parts) if parts.len() == 1 => {
            let part = parts[0].as_object()?;
            (part.len() == 2 && part.get("type")?.as_str()? == "text")
                .then(|| part.get("text").and_then(Value::as_str))
                .flatten()
        }
        _ => None,
    }
}
fn contains_prompt(value: &Value, prompt: &str) -> Option<bool> {
    let mut stack = vec![(value, 0)];
    let mut count = 0;
    while let Some((node, depth)) = stack.pop() {
        count += 1;
        if count > 4096 || depth > 32 {
            return None;
        }
        match node {
            Value::String(text) if text.contains(prompt) => return Some(true),
            Value::Array(values) => stack.extend(values.iter().map(|value| (value, depth + 1))),
            Value::Object(values) => stack.extend(values.values().map(|value| (value, depth + 1))),
            _ => {}
        }
    }
    Some(false)
}
fn matches_history(body: &Value, prompts: &[Zeroizing<String>]) -> bool {
    if body.get("stream") != Some(&Value::Bool(true)) {
        return false;
    }
    let Some(messages) = body.get("messages").and_then(Value::as_array) else {
        return false;
    };
    if messages.is_empty() || messages.len() > 512 || prompts.is_empty() {
        return false;
    }
    let mut next = 0;
    for message in messages {
        let Some(role) = message.get("role").and_then(Value::as_str) else {
            return false;
        };
        if role == "user" {
            if next >= prompts.len()
                || message.get("content").and_then(exact_text) != Some(prompts[next].as_str())
            {
                return false;
            }
            next += 1;
        } else if !matches!(role, "system" | "developer" | "assistant" | "tool") {
            return false;
        }
        // The failure nonce may occur only as the exact final submitted user text.
        let Some(failure) = prompts.last() else {
            return false;
        };
        if (role != "user" || next != prompts.len())
            && contains_prompt(message, failure) != Some(false)
        {
            return false;
        }
    }
    next == prompts.len()
        && messages
            .last()
            .and_then(|message| message.get("role"))
            .and_then(Value::as_str)
            == Some("user")
}
fn main_context(body: &Value) -> Option<MainContext> {
    let model = body.get("model")?.as_str()?;
    if model.is_empty() || model.len() > 256 {
        return None;
    }
    let messages = body.get("messages")?.as_array()?;
    let context: Vec<&Value> = messages
        .iter()
        .filter(|message| {
            matches!(
                message.get("role").and_then(Value::as_str),
                Some("system" | "developer")
            )
        })
        .collect();
    if context.is_empty() || context.len() > 32 {
        return None;
    }
    let mut hash = Sha256::new();
    hash.update((context.len() as u64).to_le_bytes());
    for message in context {
        let text = Zeroizing::new(serde_json::to_string(message).ok()?);
        if text.len() > 65536 {
            return None;
        }
        hash.update((text.len() as u64).to_le_bytes());
        hash.update(text.as_bytes());
    }
    Some(MainContext {
        model: Sha256::digest(model.as_bytes()).into(),
        instructions: hash.finalize().into(),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn request(prompts: &[&str], system: &str) -> Value {
        let mut messages = vec![json!({"role":"system","content":system})];
        for prompt in prompts {
            messages.push(json!({"role":"user","content":prompt}));
        }
        json!({"stream":true,"model":"fixture","messages":messages})
    }
    fn prepared() -> (FailureTurnAuthority, u64) {
        let mut authority = FailureTurnAuthority::default();
        for (index, prompt) in ["response nonce1", "tool nonce2"].iter().enumerate() {
            assert_eq!(authority.prepare(prompt, false, false), Ok(None));
            authority.learn_context(&request(
                &["response nonce1", "tool nonce2"][..=index],
                "main instructions",
            ));
        }
        let epoch = authority
            .prepare("failure nonce3", true, false)
            .unwrap()
            .unwrap();
        (authority, epoch)
    }
    #[test]
    fn full_current_history_context_and_one_failure_epoch_are_required() {
        let (mut authority, epoch) = prepared();
        assert!(authority.armed());
        let valid = request(
            &["response nonce1", "tool nonce2", "failure nonce3"],
            "main instructions",
        );
        let mut wrong_stream = valid.clone();
        wrong_stream["stream"] = json!(false);
        let mut trailing_assistant = valid.clone();
        trailing_assistant["messages"]
            .as_array_mut()
            .unwrap()
            .push(json!({"role":"assistant","content":"tool"}));
        let mut quoted = valid.clone();
        quoted["messages"][3]["content"] = json!("Give a title for failure nonce3");
        let mut multi = valid.clone();
        multi["messages"][3]["content"] =
            json!([{"type":"text","text":"failure nonce3"},{"type":"image_url","image_url":{}}]);
        for other in [
            wrong_stream,
            trailing_assistant,
            quoted,
            multi,
            request(&["failure nonce3"], "main instructions"),
            request(
                &["response nonce1", "tool nonce2", "failure nonce3"],
                "Generate title",
            ),
            request(
                &["response nonce1", "tool nonce2", "failure nonce3", "other"],
                "main instructions",
            ),
            request(
                &["response nonce1", "tool nonce2", "failure nonce3"],
                "main instructions failure nonce3",
            ),
        ] {
            assert!(!authority.observe(&other));
            assert!(!authority.observed(epoch));
        }
        let mut singleton = valid.clone();
        singleton["messages"][3]["content"] = json!([{"type":"text","text":"failure nonce3"}]);
        let (mut other, other_epoch) = prepared();
        assert!(other.observe(&singleton));
        assert!(other.observed(other_epoch));
        assert!(authority.observe(&valid));
        assert!(authority.observed(epoch));
        assert!(!authority.observe(&valid));
        assert!(!authority.observed(epoch + 1));
    }
    #[test]
    fn ambiguous_prior_context_and_missing_main_requests_cannot_arm_failure() {
        let mut authority = FailureTurnAuthority::default();
        authority.prepare("first", false, false).unwrap();
        authority.learn_context(&request(&["first"], "main"));
        authority.learn_context(&request(&["first"], "title"));
        authority.prepare("second", false, false).unwrap();
        assert!(authority.prepare("third", true, false).is_err());
        let mut missing = FailureTurnAuthority::default();
        missing.prepare("first", false, false).unwrap();
        missing.prepare("second", false, false).unwrap();
        assert!(missing.prepare("third", true, false).is_err());
        let mut one_request = FailureTurnAuthority::default();
        one_request.prepare("first", false, false).unwrap();
        one_request.learn_context(&request(&["first"], "main"));
        one_request.prepare("second", false, false).unwrap();
        assert!(one_request.prepare("third", true, false).is_err());
    }
    #[test]
    fn rejection_diagnostics_ignore_background_and_distinguish_contracts() {
        let mut authority = FailureTurnAuthority::default();
        authority.prepare("private one", false, false).unwrap();
        authority.learn_context(&request(&["background"], "other"));
        assert_eq!(authority.rejection_observation().rejected_stream, 0);
        let mut current = request(&["private one"], "main");
        current["stream"] = json!(false);
        authority.learn_context(&current);
        assert_eq!(authority.rejection_observation().rejected_stream, 1);
        current["stream"] = json!(true);
        authority.learn_context(&current);
        assert_eq!(authority.rejection_observation().learned_turns, 1);
        authority.prepare("private two", false, false).unwrap();
        authority.learn_context(&request(&["private two"], "main"));
        assert_eq!(authority.rejection_observation().rejected_history, 1);
        authority.learn_context(&request(&["private one", "private two"], "changed"));
        authority.learn_context(&request(&["private one", "private two"], "conflicting"));
        let observation = authority.rejection_observation();
        assert_eq!(observation.status, AuthorityRejection::ContextChanged);
        assert_eq!(observation.rejected_context, 1);
        let wire = serde_json::to_string(&observation).unwrap();
        assert!(!wire.contains("private"));
        assert!(!wire.contains("\"changed\""));
        assert!(authority.prepare("private three", true, false).is_err());
    }

    fn add_fixture_tools(mut body: Value) -> Value {
        body["tools"] = json!([{"type":"function","function":{"name":"mcp__nanh-read-fixture__read_file","parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}}}]);
        body
    }
    #[test]
    fn fixture_context_requires_verified_tool_set_and_all_existing_causal_checks() {
        let mut authority = FailureTurnAuthority::default();
        authority.prepare("first", false, false).unwrap();
        authority.learn_context(&request(&["first"], "one"));
        authority.prepare("second", false, false).unwrap();
        let second = add_fixture_tools(request(&["first", "second"], "two"));
        authority.learn_context(&second);
        authority.authorize_fixture_context().unwrap();
        let epoch = authority.prepare("third", true, false).unwrap().unwrap();
        let valid = add_fixture_tools(request(&["first", "second", "third"], "three"));
        let mut different_tool = valid.clone();
        different_tool["tools"][0]["function"]["description"] = json!("different definition");
        let mut duplicate = valid.clone();
        duplicate["tools"]
            .as_array_mut()
            .unwrap()
            .push(valid["tools"][0].clone());
        let mut model = valid.clone();
        model["model"] = json!("other");
        let mut nonstream = valid.clone();
        nonstream["stream"] = json!(false);
        for bad in [
            request(&["first", "second", "third"], "three"),
            different_tool,
            duplicate,
            model,
            nonstream,
            add_fixture_tools(request(&["third"], "three")),
        ] {
            assert!(!authority.observe(&bad));
        }
        assert!(!authority.observed(epoch));
        assert!(authority.observe(&valid));
        assert!(authority.observed(epoch));
        assert!(!authority.observe(&valid));
        let mut ambiguous = FailureTurnAuthority::default();
        ambiguous.prepare("first", false, false).unwrap();
        ambiguous.learn_context(&request(&["first"], "one"));
        ambiguous.prepare("second", false, false).unwrap();
        ambiguous.learn_context(&second);
        let mut changed = second.clone();
        changed["tools"][0]["function"]["description"] = json!("changed");
        ambiguous.learn_context(&changed);
        assert!(ambiguous.authorize_fixture_context().is_err());
    }

    #[test]
    fn armed_rejections_are_private_and_only_count_current_candidates() {
        let mut authority = FailureTurnAuthority::default();
        authority.prepare("first", false, false).unwrap();
        authority.learn_context(&request(&["first"], "instructions"));
        authority.prepare("second", false, false).unwrap();
        authority.learn_context(&request(&["first", "second"], "instructions"));
        let epoch = authority.prepare("third", true, false).unwrap().unwrap();
        assert!(!authority.observe(&request(&["unrelated title"], "instructions")));
        let valid = request(&["first", "second", "third"], "instructions");
        let mut stream = valid.clone();
        stream["stream"] = json!(false);
        assert!(!authority.observe(&stream));
        assert!(!authority.observe(&request(&["third"], "instructions")));
        assert!(!authority.observe(&request(&["first", "second", "third"], "changed")));
        let observation = authority.rejection_observation();
        assert_eq!(observation.status, AuthorityRejection::ArmedUnobserved);
        assert_eq!(
            (
                observation.rejected_stream,
                observation.rejected_history,
                observation.rejected_context
            ),
            (1, 1, 1)
        );
        let wire = serde_json::to_string(&observation).unwrap();
        assert!(!wire.contains("third") && !wire.contains("instructions"));
        assert!(!authority.observed(epoch));
        assert!(authority.observe(&valid));
        assert!(authority.observed(epoch));
    }

    #[test]
    fn failure_requires_the_unique_latest_verified_context_and_unchanged_model() {
        let mut authority = FailureTurnAuthority::default();
        authority.prepare("first", false, false).unwrap();
        authority.learn_context(&request(&["first"], "initial instructions"));
        authority.prepare("second", false, false).unwrap();
        authority.learn_context(&request(&["first", "second"], "tool instructions"));
        let epoch = authority.prepare("third", true, false).unwrap().unwrap();
        assert!(!authority.observe(&request(
            &["first", "second", "third"],
            "initial instructions"
        )));
        assert!(!authority.observe(&request(&["first", "second", "third"], "new instructions")));
        assert!(authority.observe(&request(&["first", "second", "third"], "tool instructions")));
        assert!(authority.observed(epoch));

        let mut changed_model = FailureTurnAuthority::default();
        changed_model.prepare("first", false, false).unwrap();
        changed_model.learn_context(&request(&["first"], "initial instructions"));
        changed_model.prepare("second", false, false).unwrap();
        let mut request = request(&["first", "second"], "tool instructions");
        request["model"] = json!("different fixture model");
        changed_model.learn_context(&request);
        assert!(changed_model.prepare("third", true, false).is_err());
    }
}
