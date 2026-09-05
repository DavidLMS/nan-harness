use nan_harness_core::DesktopHarnessKind;
use serde_json::Value;
use std::str::FromStr as _;

#[test]
fn desktop_harness_names_display_names_and_aliases_are_stable() {
    let registry: [(DesktopHarnessKind, &str, &str, &[&str]); 5] = [
        (
            DesktopHarnessKind::ChatGpt,
            "chatgpt-desktop",
            "ChatGPT Desktop",
            &["codex-desktop"],
        ),
        (
            DesktopHarnessKind::Claude,
            "claude-desktop",
            "Claude Desktop",
            &[],
        ),
        (
            DesktopHarnessKind::Hermes,
            "hermes-desktop",
            "Hermes Desktop",
            &[],
        ),
        (
            DesktopHarnessKind::Pen,
            "pen-desktop",
            "Pen Desktop",
            &["pen"],
        ),
        (DesktopHarnessKind::Zed, "zed-desktop", "Zed", &["zed"]),
    ];

    for (kind, name, display_name, aliases) in registry {
        assert_eq!(kind.to_string(), name);
        assert_eq!(kind.display_name(), display_name);
        assert_eq!(
            serde_json::to_value(kind).expect("desktop harness should serialize"),
            Value::String(name.to_owned())
        );
        assert_eq!(
            DesktopHarnessKind::from_str(name).expect("canonical name should parse"),
            kind
        );
        for alias in aliases {
            assert_eq!(
                DesktopHarnessKind::from_str(alias).expect("alias should parse"),
                kind
            );
        }
    }
}

#[test]
fn unknown_desktop_harness_errors_preserve_the_requested_name() {
    let error = DesktopHarnessKind::from_str("not-a-desktop")
        .expect_err("unknown desktop harness should not parse");

    assert!(error.to_string().contains("not-a-desktop"));
}
