use crate::support::{assert_success, command, native_harness, root};
use std::net::TcpListener;

#[test]
fn offline_text_and_json_use_native_local_discovery_without_http_or_credentials() {
    let root = root();
    native_harness(root.path());
    let state = root.path().join("state");
    let credential = state.join("credential.json");
    std::fs::write(&credential, b"invalid-private-credential-state").unwrap();
    nan_harness_telemetry::consent::TelemetrySettingsStore::new(&state)
        .set(nan_harness_telemetry::consent::TelemetryPreference::On)
        .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let endpoint = format!("http://{address}");
    for target in [None, Some("claude"), Some("chatgpt-desktop")] {
        for json in [false, true] {
            let mut command = command(root.path());
            command.args(["doctor", "--offline"]);
            command.args(target);
            if json {
                command.arg("--json");
            }
            let output = command
                .env("NAN_BASE_URL", &endpoint)
                .env(
                    "NAN_COMPATIBILITY_MANIFEST_URL",
                    format!("{endpoint}/compatibility"),
                )
                .env("NAN_UPDATE_MANIFEST_URL", format!("{endpoint}/update"))
                .env(
                    "NAN_HARNESS_GLITCHTIP_DSN",
                    format!("http://synthetic@{address}/1"),
                )
                .env("NAN_HARNESS_UMAMI_URL", &endpoint)
                .env("NAN_HARNESS_UMAMI_WEBSITE_ID", "synthetic-website")
                .output()
                .unwrap();
            assert_success(&output);
            assert_eq!(
                listener.accept().unwrap_err().kind(),
                std::io::ErrorKind::WouldBlock
            );
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(!stdout.contains("invalid-private-credential-state"));
            assert!(!stdout.contains(&endpoint));
            assert!(
                !String::from_utf8_lossy(&output.stderr)
                    .contains("invalid-private-credential-state")
            );
            if json {
                assert_offline_report(&output.stdout, target);
                assert!(!stdout.contains(root.path().to_string_lossy().as_ref()));
            } else {
                assert!(stdout.contains("Offline:"));
                assert!(stdout.contains("not refreshed"));
                if target.is_none() {
                    assert!(stdout.contains("[SKIP] NaN API and model discovery: offline"));
                }
            }
        }
    }
    assert_eq!(
        std::fs::read(credential).unwrap(),
        b"invalid-private-credential-state"
    );
}

fn assert_offline_report(bytes: &[u8], target: Option<&str>) {
    let report: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    assert_eq!(report["offline"], true);
    if target.is_none() {
        assert_eq!(report["provider"]["credential"], "not-checked");
        assert_eq!(report["provider"]["api"], "skipped-offline");
        assert_eq!(report["provider"]["codingModels"], serde_json::json!([]));
        assert!(
            report["harnesses"]
                .as_array()
                .unwrap()
                .iter()
                .any(|harness| {
                    harness["id"] == "claude-code"
                        && harness["installed"] == true
                        && harness["version"] == "2.1.251"
                })
        );
    } else if target == Some("claude") {
        assert_eq!(report["installed"], true);
        assert_eq!(report["version"], "2.1.251");
    }
}

#[test]
fn offline_missing_native_target_remains_an_error_in_text_and_json() {
    let root = root();
    for json in [false, true] {
        let mut command = command(root.path());
        command.args(["doctor", "claude", "--offline"]);
        if json {
            command.arg("--json");
        }
        let output = command.output().unwrap();
        assert!(!output.status.success());
        if json {
            let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(report["offline"], true);
            assert_eq!(report["level"], "error");
            assert_eq!(report["installed"], false);
        }
    }
}
