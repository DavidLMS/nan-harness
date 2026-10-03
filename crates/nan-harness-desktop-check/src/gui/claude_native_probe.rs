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

// Both pinned Linux mode variants are passive diagnostics, never navigation authority.
fn linux_mode_selector(role: &str, mode: &str, enabled: bool) -> String {
    let state = if enabled { "[enabled=\"true\"]" } else { "" };
    let labels = [
        mode.to_owned(),
        format!("{mode}, awaiting your input"),
        format!("{mode}, unread activity"),
        format!("{mode}, working"),
    ];
    ["name", "description"].into_iter().flat_map(|group_name| {
        labels.iter().flat_map(move |label| ["name", "description"].map(move |name| {
            format!("group[visible=\"true\"][{group_name}=\"Mode\"] {role}[visible=\"true\"]{state}[{name}=\"{label}\"]")
        }))
    }).collect::<Vec<_>>().join(", ")
}
fn linux_mode_counts(mut query: impl FnMut(&str) -> Option<usize>) -> serde_json::Value {
    let group =
        "group[visible=\"true\"][name=\"Mode\"], group[visible=\"true\"][description=\"Mode\"]";
    let mut count = |selector: &str| query(selector).filter(|count| *count <= 4096);
    let mut values = serde_json::json!({"modeGroupVisible": count(group)});
    for (key, role, mode, enabled) in [
        ("chatButtonVisible", "button", "Chat", false),
        ("chatButtonEnabled", "button", "Chat", true),
        ("chatRadioVisible", "radio_button", "Chat", false),
        ("chatRadioEnabled", "radio_button", "Chat", true),
        ("coworkButtonVisible", "button", "Cowork", false),
        ("coworkButtonEnabled", "button", "Cowork", true),
        ("coworkRadioVisible", "radio_button", "Cowork", false),
        ("coworkRadioEnabled", "radio_button", "Cowork", true),
    ] {
        values[key] = if values["modeGroupVisible"] == 1 {
            count(&linux_mode_selector(role, mode, enabled)).into()
        } else {
            serde_json::Value::Null
        };
    }
    // Each source query must remain under a unique Mode group; never aggregate
    // controls across newly duplicated groups into an observed receipt.
    if values["modeGroupVisible"] == 1 {
        values["modeGroupVisible"] = count(group).into();
    }
    let status = if values["modeGroupVisible"].is_null() {
        "query-failed"
    } else if values["modeGroupVisible"] == 0 {
        "group-unavailable"
    } else if values["modeGroupVisible"] != 1 {
        "group-ambiguous"
    } else if values
        .as_object()
        .unwrap()
        .values()
        .any(serde_json::Value::is_null)
    {
        "query-failed"
    } else {
        "observed"
    };
    if status != "observed" {
        for (key, value) in values.as_object_mut().unwrap() {
            if key != "modeGroupVisible" {
                *value = serde_json::Value::Null;
            }
        }
    }
    serde_json::json!({"status":status,"sourceCount":values})
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

#[cfg(any(target_os = "macos", windows, test))]
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

    #[cfg(windows)]
    pub(super) fn wait_initial_windows_claude_composer<P: crate::process::Observation>(
        &self,
        process: &mut P,
        deadline: std::time::Instant,
    ) -> Result<(), super::visual::AcquisitionFailure> {
        loop {
            // Reuse the original-window acquisition checks and its absolute
            // deadline. A source editor cannot waive native stability or focus.
            self.visual.observe_windows_pending(process, deadline)?;
            let count = |label: &str| {
                self.app.as_ref()?.locator(&format!(
                    "text_area[visible=\"true\"][editable=\"true\"][name=\"{label}\"], text_area[visible=\"true\"][editable=\"true\"][description=\"{label}\"], text_field[visible=\"true\"][editable=\"true\"][name=\"{label}\"], text_field[visible=\"true\"][editable=\"true\"][description=\"{label}\"]"
                )).elements().ok().map(|elements| elements.len())
            };
            let classic = count("Write your prompt to Claude");
            self.visual.observe_windows_pending(process, deadline)?;
            let modern = count("Message");
            self.visual.observe_windows_pending(process, deadline)?;
            if source_composer_ready(classic, modern) {
                // Source readiness precedes the sole fit and strict final display proof.
                self.visual
                    .finish_windows_initial_acquisition(process, deadline)?;
                if source_composer_ready(count("Write your prompt to Claude"), count("Message")) {
                    self.visual
                        .finish_windows_initial_acquisition(process, deadline)?;
                    return Ok(());
                }
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
        let windows = cfg!(windows)
            && std::env::var("RUNNER_OS").as_deref() == Ok("Windows")
            && std::env::var("NANH_CLAUDE_WINDOWS_PROFILE_POLICY").as_deref() == Ok("private-env");
        if !(mac || linux || windows)
            || self.kind != DesktopHarnessKind::Claude
            || std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
            || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
            || std::env::var("NANH_DESKTOP_QUALIFICATION_MODE").as_deref() != Ok("startup-baseline")
        {
            return None;
        }
        let query = |selector: &str| {
            self.app
                .as_ref()?
                .locator(selector)
                .elements()
                .ok()
                .map(|elements| elements.len())
        };
        let mut inventory = counts(query);
        if linux {
            inventory["linuxModeRoles"] = linux_mode_counts(query);
        }
        Some(inventory)
    }
}

pub(super) fn record(directory: &Path, owner: u32, source_count: &serde_json::Value) {
    let Some(directory) = super::qualification_directory::canonical_directory(directory) else {
        return;
    };
    let mut source_count = source_count.clone();
    let linux_roles = source_count
        .as_object_mut()
        .and_then(|value| value.remove("linuxModeRoles"));
    if cfg!(target_os = "linux")
        && let Some(roles) = linux_roles
    {
        let value = serde_json::json!({"schemaVersion":1,"mechanism":"claude-linux-mode-roles",
                "diagnosticsOnly":true,"sourceVersion":"2.9939.4",
                "modeSourceSha256":"62ffbc1b8a3e4440ae77a33be142afd1914796f945bcd75d58cfe73679925f61",
                "segmentedSourceSha256":"1fe986422649ab736613079340a52157efd7791b96e0b9c00c46681731b7a4ea",
                "radioSourceSha256":"9c6ff87b4eaf0e9ad25e6329536f4337586b015e0f868389e72480c1769920a9",
                "status":roles["status"],"sourceCount":roles["sourceCount"]});
        if let Ok(mut file) =
            open_private_new(&directory.join(format!("claude-linux-mode-roles-{owner}.json")))
        {
            let _ = file.write_all(value.to_string().as_bytes());
        }
    }
    // Frozen official Mac ZIP and Windows MSIX 2.19675.0 contain byte-identical
    // renderer chunks; the enclosing trial binds platform artifact/app digests.
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
    fn readiness_does_not_misclassify_new_chat_send_state_as_cowork_or_login() {
        let measured = counts(|selector| {
            if selector.contains("Write your prompt to Claude") || selector.contains("Start task") {
                Some(1)
            } else {
                Some(0)
            }
        });
        assert_eq!(measured["startTaskVisible"], 1);
        assert_eq!(measured["sendMessageEnabled"], 0);
        assert!(source_composer_ready(
            measured["classicEditable"]
                .as_u64()
                .and_then(|count| usize::try_from(count).ok()),
            measured["modernMessageEditable"]
                .as_u64()
                .and_then(|count| usize::try_from(count).ok()),
        ));
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
    #[test]
    fn linux_mode_distinguishes_radios_and_exact_attention_labels() {
        let counts = super::linux_mode_counts(|selector| {
            if !selector.contains(" button[") && !selector.contains(" radio_button[") {
                return Some(1);
            }
            if selector.contains(" radio_button[") && selector.contains("Chat, working") {
                Some(1)
            } else {
                Some(0)
            }
        });
        assert_eq!(counts["status"], "observed");
        assert_eq!(counts["sourceCount"]["chatRadioVisible"], 1);
        assert_eq!(counts["sourceCount"]["chatButtonVisible"], 0);
        assert!(!counts.to_string().contains("Chat"));
        for label in [
            "Chat",
            "Chat, awaiting your input",
            "Chat, unread activity",
            "Chat, working",
        ] {
            assert!(
                super::linux_mode_selector("radio_button", "Chat", true)
                    .contains(&format!("=\"{label}\"]"))
            );
        }
    }
    #[test]
    fn linux_mode_rejects_ambiguous_or_failed_group_queries() {
        for group in [None, Some(0), Some(2), Some(4097)] {
            let result = super::linux_mode_counts(|selector| {
                assert!(!selector.contains(" button[") && !selector.contains(" radio_button["));
                group
            });
            assert_ne!(result["status"], "observed");
            for (key, count) in result["sourceCount"].as_object().unwrap() {
                if key != "modeGroupVisible" {
                    assert!(count.is_null());
                }
            }
        }
        let mut calls = 0;
        let changed = super::linux_mode_counts(|_| {
            calls += 1;
            if calls == 10 { Some(2) } else { Some(1) }
        });
        assert_eq!(changed["status"], "group-ambiguous");
        assert!(changed["sourceCount"]["chatRadioVisible"].is_null());
        let failed = super::linux_mode_counts(|s| {
            if s.contains(" radio_button[") {
                None
            } else {
                Some(1)
            }
        });
        assert_eq!(failed["status"], "query-failed");
        assert!(failed["sourceCount"]["chatButtonVisible"].is_null());
    }
}
