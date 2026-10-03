//! Explicit hosted public navigation; one accessibility action and no conversation input.

use super::{ComposerFailure, Gui, map_error, require_foreground_pid};
use crate::report::Reason;
use nan_harness_private_fs::open_private_new;
use serde::Serialize;
use std::io::Write as _;
use std::path::Path;
use std::time::{Duration, Instant};
use xa11y::{App, AppExt as _, Element, ElementData};

#[derive(Default, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Phase {
    #[default]
    Preflight,
    Press,
    Postcondition,
    Completed,
}
#[derive(Default, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ActionStatus {
    #[default]
    NotAttempted,
    Completed,
    Uncertain,
}
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Facts {
    phase: Phase,
    action_status: ActionStatus,
    preconditions_verified: bool,
    chat_postcondition_verified: bool,
    native_guard_verified: bool,
}
impl Facts {
    fn value(&self) -> Option<serde_json::Value> {
        let mut value = serde_json::to_value(self).ok()?;
        value["pressAttempted"] =
            (!matches!(self.action_status, ActionStatus::NotAttempted)).into();
        Some(value)
    }
}
fn preflight(counts: &serde_json::Value) -> bool {
    [
        "modeGroupVisible",
        "modeChatVisible",
        "modeChatEnabled",
        "classicEditable",
        "startTaskVisible",
    ]
    .iter()
    .all(|key| counts[*key].as_u64() == Some(1))
}
fn postcondition(counts: &serde_json::Value) -> bool {
    counts["classicEditable"].as_u64() == Some(1)
        && counts["sendMessageVisible"].as_u64() == Some(1)
        && counts["startTaskVisible"].as_u64() == Some(0)
}
fn held_identity(before: &ElementData, after: &ElementData, pid: u32) -> bool {
    let bounded = before
        .bounds
        .is_some_and(|bounds| bounds.width > 0 && bounds.height > 0);
    before.pid == Some(pid)
        && after.pid == Some(pid)
        && bounded
        && before.stable_id.as_ref().is_some_and(|id| !id.is_empty())
        && before.stable_id == after.stable_id
        && before.bounds == after.bounds
    // xa11y Mac allocates a new cache handle per query; AXIdentifier is the
    // platform identity, while that handle only addresses a provider cache.
}
fn press_once(facts: &mut Facts, action: impl FnOnce() -> bool) -> Result<(), Reason> {
    if !matches!(facts.action_status, ActionStatus::NotAttempted) {
        return Err(Reason::ActionUnsupported);
    }
    facts.phase = Phase::Press;
    facts.action_status = ActionStatus::Uncertain;
    if action() {
        facts.action_status = ActionStatus::Completed;
    }
    Ok(())
}
fn within(deadline: Instant) -> Result<(), Reason> {
    if Instant::now() < deadline {
        Ok(())
    } else {
        Err(Reason::Timeout)
    }
}
impl Gui {
    fn navigation_guard(
        &self,
        deadline: Instant,
        observations: &mut Vec<ComposerFailure>,
    ) -> Result<(), Reason> {
        within(deadline)?;
        self.observe_hosted_startup(observations)?;
        within(deadline)?;
        let foreground = App::foreground(deadline.saturating_duration_since(Instant::now()))
            .map_err(map_error)
            .map(|app| if app.is_foreground() { app.pid } else { None });
        require_foreground_pid(foreground, self.visual.pid())?;
        within(deadline)
    }
    fn chat_control(&self) -> Option<Element> {
        let app = self.app.as_ref()?;
        let mut found = app
            .locator(&super::claude_native_probe::mode_button("Chat", true))
            .elements()
            .ok()?;
        if found.len() != 1 {
            return None;
        }
        found.pop()
    }
    pub(super) fn claude_chat_navigation(
        &self,
        directory: &Path,
        owner: u32,
        observations: &mut Vec<ComposerFailure>,
    ) -> Result<(), Reason> {
        if std::env::var("NANH_CLAUDE_MAC_CHAT_NAVIGATION").as_deref() != Ok("1") {
            return Ok(());
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        // The passive inventory enforces the exact hosted Mac/profile policy.
        if self.claude_composer_inventory().is_none() {
            return Ok(());
        }
        let mut facts = Facts::default();
        let outcome = self.navigate_chat(deadline, observations, &mut facts);
        record(directory, owner, &facts);
        outcome
    }
    fn navigate_chat(
        &self,
        deadline: Instant,
        observations: &mut Vec<ComposerFailure>,
        facts: &mut Facts,
    ) -> Result<(), Reason> {
        self.navigation_guard(deadline, observations)?;
        let Some(counts) = self.claude_composer_inventory() else {
            return Ok(());
        };
        within(deadline)?;
        if !preflight(&counts) {
            return Ok(());
        }
        let (Some(held), Some(pid)) = (
            self.chat_control(),
            self.app.as_ref().and_then(|app| app.pid),
        ) else {
            return Ok(());
        };
        within(deadline)?;
        let Some(fresh) = self.chat_control() else {
            return Ok(());
        };
        if !held_identity(held.data(), fresh.data(), pid) {
            return Ok(());
        }
        let Some(counts) = self.claude_composer_inventory() else {
            return Ok(());
        };
        if !preflight(&counts) {
            return Ok(());
        }
        self.navigation_guard(deadline, observations)?;
        let Some(final_control) = self.chat_control() else {
            return Ok(());
        };
        within(deadline)?;
        if !held_identity(held.data(), final_control.data(), pid) {
            return Ok(());
        }
        facts.preconditions_verified = true;
        facts.native_guard_verified = true;
        press_once(facts, || held.press().is_ok())?;
        // A failed/uncertain receipt never permits a second action.
        facts.phase = Phase::Postcondition;
        loop {
            self.navigation_guard(deadline, observations)
                .inspect_err(|_| facts.native_guard_verified = false)?;
            let counts = self.claude_composer_inventory();
            within(deadline).inspect_err(|_| facts.native_guard_verified = false)?;
            if counts.as_ref().is_some_and(postcondition) {
                self.navigation_guard(deadline, observations)
                    .inspect_err(|_| facts.native_guard_verified = false)?;
                facts.chat_postcondition_verified = true;
                facts.phase = Phase::Completed;
                return Ok(());
            }
            std::thread::sleep(
                Duration::from_millis(50).min(deadline.saturating_duration_since(Instant::now())),
            );
        }
    }
}
fn record(directory: &Path, owner: u32, facts: &Facts) {
    let Ok(metadata) = std::fs::symlink_metadata(directory) else {
        return;
    };
    if !metadata.is_dir() || directory.canonicalize().ok().as_deref() != Some(directory) {
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if metadata.permissions().mode() & 0o077 != 0 {
            return;
        }
    }
    let Some(mut value) = facts.value() else {
        return;
    };
    value["schemaVersion"] = 1.into();
    value["mechanism"] = "claude-chat-navigation".into();
    value["diagnosticsOnly"] = true.into();
    if let Ok(mut file) =
        open_private_new(&directory.join(format!("claude-chat-navigation-{owner}.json")))
    {
        let _ = file.write_all(value.to_string().as_bytes());
    }
}
#[cfg(test)]
mod tests {
    use super::{Facts, held_identity, postcondition, preflight, press_once, within};
    #[test]
    fn source_counts_require_unique_scoped_controls_and_chat_transition() {
        let mut counts = serde_json::json!({"modeGroupVisible":1,"modeChatVisible":1,"modeChatEnabled":1,"classicEditable":1,"startTaskVisible":1,"sendMessageVisible":0});
        assert!(preflight(&counts));
        assert!(!postcondition(&counts));
        for key in [
            "modeGroupVisible",
            "modeChatVisible",
            "modeChatEnabled",
            "classicEditable",
            "startTaskVisible",
        ] {
            let original = counts[key].clone();
            counts[key] = 2.into();
            assert!(!preflight(&counts));
            counts[key] = serde_json::Value::Null;
            assert!(!preflight(&counts));
            counts[key] = original;
        }
        counts["startTaskVisible"] = 0.into();
        counts["sendMessageVisible"] = 1.into();
        assert!(postcondition(&counts));
        assert!(!preflight(&counts));
    }
    #[test]
    fn expired_phase_and_default_receipt_do_not_claim_action_or_navigation() {
        assert!(within(std::time::Instant::now()).is_err());
        let value = Facts::default().value().unwrap();
        assert_eq!(value["actionStatus"], "not-attempted");
        assert_eq!(value["pressAttempted"], false);
        assert_eq!(value["chatPostconditionVerified"], false);
    }
    #[test]
    fn uncertain_press_is_not_replayed_and_post_readback_is_independent() {
        let mut facts = Facts::default();
        let mut actions = 0;
        press_once(&mut facts, || {
            actions += 1;
            false
        })
        .unwrap();
        assert!(
            press_once(&mut facts, || {
                actions += 1;
                true
            })
            .is_err()
        );
        assert_eq!(actions, 1);
        let value = facts.value().unwrap();
        assert_eq!(value["pressAttempted"], true);
        assert_eq!(value["actionStatus"], "uncertain");
        assert_eq!(value["chatPostconditionVerified"], false);
    }
    #[test]
    fn fresh_provider_cache_handles_are_not_platform_identity() {
        let mut before: xa11y::ElementData = serde_json::from_value(serde_json::json!({
            "role":"Button","name":null,"value":null,"description":null,
            "bounds":{"x":10,"y":20,"width":30,"height":40},"actions":[],
            "states":xa11y::StateSet::default(),"numeric_value":null,"min_value":null,"max_value":null,
            "stable_id":"private-control-id","pid":123,"raw":{}
        }))
        .unwrap();
        before.handle = 1;
        let mut after = before.clone();
        after.handle = 2;
        assert!(held_identity(&before, &after, 123));
        after.pid = Some(124);
        assert!(!held_identity(&before, &after, 123));
        after = before.clone();
        after.stable_id = Some("replaced-control".into());
        assert!(!held_identity(&before, &after, 123));
        after = before.clone();
        after.bounds.as_mut().unwrap().x += 1;
        assert!(!held_identity(&before, &after, 123));
        before.stable_id = None;
        after = before.clone();
        assert!(!held_identity(&before, &after, 123));
    }
}
