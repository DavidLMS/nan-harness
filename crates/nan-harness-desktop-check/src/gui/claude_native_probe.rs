//! Passive source-labelled controls; these counts never authorize input or acceptance.

use super::Gui;
use nan_harness_core::DesktopHarnessKind;
use nan_harness_private_fs::open_private_new;
use std::io::Write as _;
use std::path::Path;

pub(super) fn mode_button(label: &str, enabled: bool) -> String {
    let enabled = if enabled { "[enabled=\"true\"]" } else { "" };
    ["name", "description"].into_iter().flat_map(|group_name| {
        ["name", "description"].map(move |button_name| {
            format!("group[visible=\"true\"][{group_name}=\"Mode\"] button[visible=\"true\"]{enabled}[{button_name}=\"{label}\"]")
        })
    }).collect::<Vec<_>>().join(", ")
}

fn counts(mut query: impl FnMut(&str) -> Option<usize>) -> serde_json::Value {
    let mut count = |selector: &str| query(selector).filter(|value| *value <= 4096);
    let labelled = |role: &str, label: &str, editable: bool| {
        let state = if editable { "[editable=\"true\"]" } else { "" };
        format!(
            "{role}[visible=\"true\"]{state}[name=\"{label}\"], {role}[visible=\"true\"]{state}[description=\"{label}\"]"
        )
    };
    let editor = |label: &str, editable: bool| {
        format!(
            "{}, {}",
            labelled("text_area", label, editable),
            labelled("text_field", label, editable)
        )
    };
    serde_json::json!({
        "classicEditable":count(&editor("Write your prompt to Claude", true)),
        "classicVisible":count(&editor("Write your prompt to Claude", false)),
        "modernMessageEditable":count(&editor("Message", true)),
        "sendMessageVisible":count(&labelled("button", "Send message", false)),
        "sendMessageEnabled":count("button[visible=\"true\"][enabled=\"true\"][name=\"Send message\"], button[visible=\"true\"][enabled=\"true\"][description=\"Send message\"]"),
        "startTaskVisible":count(&labelled("button", "Start task", false)),
        "modeGroupVisible":count("group[visible=\"true\"][name=\"Mode\"], group[visible=\"true\"][description=\"Mode\"]"),
        "modeChatVisible":count(&mode_button("Chat", false)),
        "modeChatEnabled":count(&mode_button("Chat", true)),
        "modeCoworkVisible":count(&mode_button("Cowork", false))
    })
}

impl Gui {
    pub(super) fn claude_composer_inventory(&self) -> Option<serde_json::Value> {
        if !cfg!(target_os = "macos")
            || self.kind != DesktopHarnessKind::Claude
            || std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
            || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
            || std::env::var("RUNNER_OS").as_deref() != Ok("macOS")
            || std::env::var("NANH_CLAUDE_MAC_PROFILE_POLICY").as_deref()
                != Ok("native-known-folders")
            || std::env::var("NANH_DESKTOP_QUALIFICATION_MODE").as_deref() != Ok("startup-baseline")
        {
            return None;
        }
        Some(counts(|selector| {
            self.app
                .as_ref()?
                .locator(selector)
                .elements()
                .ok()
                .map(|elements| elements.len())
        }))
    }
}

pub(super) fn record(directory: &Path, owner: u32, source_count: &serde_json::Value) {
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
    // Frozen official Mac ZIP 2.19675.0; the enclosing trial binds its artifact/app digests.
    let value = serde_json::json!({"schemaVersion":1,"mechanism":"claude-native-composer",
        "diagnosticsOnly":true,"sourceVersion":"2.19675.0",
        "classicSourceSha256":"6e6be632eb7adc0e66c1bb795448269d6c1f3ffe8821bea59d9e9374671cf0ea",
        "sendSourceSha256":"69d43f83ac78605402b590559cfb9bd355215336a193cedf80cc30b246c1db60",
        "modernSourceSha256":"a9f54a8a154e19f86a9d9d696b808bd693904b5e47ec63517abb635003a4244d",
        "modeSourceSha256":"0d16680f19e10d03bc11e7797d842d01159da37b5ab410cad9b7307f7eeef3aa",
        "sourceCount":source_count});
    if let Ok(mut file) =
        open_private_new(&directory.join(format!("claude-native-composer-{owner}.json")))
    {
        let _ = file.write_all(value.to_string().as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::{counts, mode_button};
    #[test]
    fn unavailable_and_oversized_queries_are_not_empty_or_unique() {
        let unavailable = counts(|_| None);
        assert!(
            unavailable
                .as_object()
                .unwrap()
                .values()
                .all(serde_json::Value::is_null)
        );
        let oversized = counts(|_| Some(4097));
        assert!(
            oversized
                .as_object()
                .unwrap()
                .values()
                .all(serde_json::Value::is_null)
        );
        let duplicates = counts(|_| Some(2));
        assert_eq!(duplicates["classicEditable"], 2);
    }
    #[test]
    fn selectors_bind_source_labels_roles_and_states_without_exporting_payload() {
        let result = counts(|selector| {
            assert!(selector.contains("[visible=\"true\"]"));
            if selector.contains("Write your prompt to Claude") {
                assert!(selector.contains("text_area") && selector.contains("text_field"));
                Some(1)
            } else if selector.contains("[enabled=\"true\"]") {
                Some(0)
            } else {
                Some(2)
            }
        });
        assert_eq!(result["classicEditable"], 1);
        assert_eq!(result["sendMessageEnabled"], 0);
        assert!(!result.to_string().contains("Claude"));
        assert!(!result.to_string().contains("Send message"));
    }
    #[test]
    fn navigation_queries_exclude_global_chat_and_preserve_ambiguity() {
        for enabled in [false, true] {
            let selector = mode_button("Chat", enabled);
            let clauses: Vec<_> = selector.split(", ").collect();
            assert_eq!(clauses.len(), 4);
            for clause in clauses {
                assert!(clause.starts_with("group[visible=\"true\"]"));
                assert!(clause.contains("=\"Mode\"] button[visible=\"true\"]"));
                assert!(clause.ends_with("=\"Chat\"]"));
                assert_eq!(clause.contains("[enabled=\"true\"]"), enabled);
            }
        }
        let result = counts(|selector| {
            // A globally present Chat button never satisfies the scoped query.
            if selector.contains("group[") && selector.contains(" button[") {
                Some(0)
            } else if selector.starts_with("group[") {
                Some(2)
            } else {
                None
            }
        });
        assert_eq!(result["modeChatVisible"], 0);
        assert_eq!(result["modeChatEnabled"], 0);
        assert_eq!(result["modeGroupVisible"], 2);
        assert!(result["classicEditable"].is_null());
    }
}
