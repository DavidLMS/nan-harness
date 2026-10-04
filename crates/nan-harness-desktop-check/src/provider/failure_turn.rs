//! Private causal authority: three submitted prompts and one stable main context.
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use zeroize::Zeroizing;

#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum AuthorityRejection {
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
    context: Option<[u8; 32]>,
    context_ambiguous: bool,
    context_turns: u8,
    failure_armed: bool,
    observed: bool,
    rejected_stream: u16,
    rejected_history: u16,
    rejected_context: u16,
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
        if failure && (self.context.is_none() || self.context_ambiguous || self.context_turns != 3)
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
            status: if self.context.is_none() {
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
    pub(super) fn armed(&self) -> bool {
        self.failure_armed
    }
    pub(super) fn learn_context(&mut self, body: &Value) {
        if self.failure_armed || self.prompts.is_empty() {
            return;
        }
        // Count only a request containing the exact current prepared user text.
        // Unrelated background/title traffic cannot alter these diagnostics.
        let relevant = body
            .get("messages")
            .and_then(Value::as_array)
            .is_some_and(|messages| {
                messages.len() <= 512
                    && messages.iter().any(|message| {
                        message.get("role").and_then(Value::as_str) == Some("user")
                            && message.get("content").and_then(exact_text)
                                == self.prompts.last().map(|prompt| prompt.as_str())
                    })
            });
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
        self.context_turns |= 1 << (self.prompts.len() - 1);
        match self.context {
            Some(held) if held != context => {
                self.context_ambiguous = true;
                self.rejected_context = self.rejected_context.saturating_add(1).min(4096);
            }
            None => self.context = Some(context),
            _ => {}
        }
    }
    pub(super) fn observe(&mut self, body: &Value) -> bool {
        if !self.failure_armed
            || self.observed
            || self.context_ambiguous
            || !matches_history(body, &self.prompts)
            || main_context(body) != self.context
        {
            return false;
        }
        self.observed = true;
        true
    }
    pub(super) fn observed(&self, epoch: u64) -> bool {
        self.epoch == epoch && self.failure_armed && self.observed
    }
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
fn main_context(body: &Value) -> Option<[u8; 32]> {
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
    hash.update((model.len() as u64).to_le_bytes());
    hash.update(model.as_bytes());
    hash.update((context.len() as u64).to_le_bytes());
    for message in context {
        let text = Zeroizing::new(serde_json::to_string(message).ok()?);
        if text.len() > 65536 {
            return None;
        }
        hash.update((text.len() as u64).to_le_bytes());
        hash.update(text.as_bytes());
    }
    Some(hash.finalize().into())
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
        let observation = authority.rejection_observation();
        assert_eq!(observation.status, AuthorityRejection::ContextChanged);
        assert_eq!(observation.rejected_context, 1);
        let wire = serde_json::to_string(&observation).unwrap();
        assert!(!wire.contains("private"));
        assert!(!wire.contains("\"changed\""));
        assert!(authority.prepare("private three", true, false).is_err());
    }
}
