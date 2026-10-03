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

#[cfg(any(target_os = "macos", test))]
fn source_composer_ready(classic: Option<usize>, modern: Option<usize>) -> bool {
    matches!((classic, modern), (Some(1), Some(0)) | (Some(0), Some(1)))
}

impl Gui {
    #[cfg(target_os = "macos")]
    pub(super) fn wait_initial_claude_composer(
        &self,
        deadline: std::time::Instant,
    ) -> Result<(), super::visual::AcquisitionFailure> {
        let unavailable = || {
            (
                crate::report::Reason::DesktopUnavailable,
                crate::diagnostics::GuiAcquisitionStage::WindowStability,
                super::error_category(crate::report::Reason::DesktopUnavailable),
                None,
                None,
            )
        };
        let app = self.app.as_ref().ok_or_else(unavailable)?;
        loop {
            if std::time::Instant::now() >= deadline {
                return Err(unavailable());
            }
            // These public source labels distinguish an actual conversation
            // editor from a stable startup shell. No control is activated.
            let count = |label: &str| {
                app.locator(&format!(
                    "text_area[visible=\"true\"][editable=\"true\"][name=\"{label}\"], text_area[visible=\"true\"][editable=\"true\"][description=\"{label}\"], text_field[visible=\"true\"][editable=\"true\"][name=\"{label}\"], text_field[visible=\"true\"][editable=\"true\"][description=\"{label}\"]"
                )).elements().ok().map(|elements| elements.len())
            };
            let classic = count("Write your prompt to Claude");
            if std::time::Instant::now() >= deadline {
                return Err(unavailable());
            }
            let modern = count("Message");
            if std::time::Instant::now() >= deadline {
                return Err(unavailable());
            }
            if source_composer_ready(classic, modern) {
                return Ok(());
            }
            std::thread::sleep(
                std::time::Duration::from_millis(200)
                    .min(deadline.saturating_duration_since(std::time::Instant::now())),
            );
        }
    }

    pub(super) fn claude_composer_inventory(&self) -> Option<serde_json::Value> {
        let mac = cfg!(target_os = "macos")
            && std::env::var("RUNNER_OS").as_deref() == Ok("macOS")
            && std::env::var("NANH_CLAUDE_MAC_PROFILE_POLICY").as_deref()
                == Ok("native-known-folders");
        let linux = cfg!(target_os = "linux")
            && std::env::var("RUNNER_OS").as_deref() == Ok("Linux")
            && std::env::var("NANH_CLAUDE_LINUX_SOURCE_POLICY").as_deref()
                == Ok("official-2.9939.4");
        if !(mac || linux)
            || self.kind != DesktopHarnessKind::Claude
            || std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
            || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
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
    let mut value = serde_json::json!({"schemaVersion":1,"mechanism":"claude-native-composer",
        "diagnosticsOnly":true,"sourceVersion":"2.19675.0",
        "classicSourceSha256":"6e6be632eb7adc0e66c1bb795448269d6c1f3ffe8821bea59d9e9374671cf0ea",
        "sendSourceSha256":"69d43f83ac78605402b590559cfb9bd355215336a193cedf80cc30b246c1db60",
        "modernSourceSha256":"a9f54a8a154e19f86a9d9d696b808bd693904b5e47ec63517abb635003a4244d",
        "modeSourceSha256":"0d16680f19e10d03bc11e7797d842d01159da37b5ab410cad9b7307f7eeef3aa",
        "sourceCount":source_count});
    if cfg!(target_os = "linux") {
        value["sourceVersion"] = "2.9939.4".into();
        value["classicSourceSha256"] =
            "26f823bafc90cff4a749bfad6916ee69e4c3189f18b54a4e958ca387939c1181".into();
        value["sendSourceSha256"] =
            "d076b2f208fc5e572d0f3cd39aba35c6bacbe100a82db569851a0ce2317fa05c".into();
        value["modernSourceSha256"] =
            "5d1afc949ac69080ef6fe15491137ca0c3d2056991a9581537cba2bcc3724287".into();
        value["modeSourceSha256"] =
            "62ffbc1b8a3e4440ae77a33be142afd1914796f945bcd75d58cfe73679925f61".into();
    }
    if let Ok(mut file) =
        open_private_new(&directory.join(format!("claude-native-composer-{owner}.json")))
    {
        let _ = file.write_all(value.to_string().as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::{counts, mode_button, source_composer_ready};
    #[test]
    fn initial_readiness_requires_one_source_editor_and_measured_absence_of_other() {
        assert!(source_composer_ready(Some(1), Some(0)));
        assert!(source_composer_ready(Some(0), Some(1)));
        for counts in [
            (None, Some(1)),
            (Some(1), None),
            (Some(0), Some(0)),
            (Some(1), Some(1)),
            (Some(2), Some(0)),
        ] {
            assert!(!source_composer_ready(counts.0, counts.1));
        }
    }

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
