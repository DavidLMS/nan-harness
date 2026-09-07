use std::process::Command;

#[cfg(unix)]
fn assert_offline_json(
    output: &std::process::Output,
    target: Option<&str>,
    private_path: &std::path::Path,
) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON report");
    assert_eq!(report["schemaVersion"], 7);
    assert_eq!(report["offline"], true);
    assert!(!stdout.contains(private_path.to_string_lossy().as_ref()));
    if target.is_none() {
        assert_eq!(report["provider"]["level"], "info");
        assert_eq!(report["provider"]["credential"], "not-checked");
        assert_eq!(report["provider"]["api"], "skipped-offline");
        assert_eq!(report["provider"]["codingModels"], serde_json::json!([]));
        assert!(report["provider"].get("codingModelCount").is_none());
        assert!(
            report["harnesses"]
                .as_array()
                .expect("harness list")
                .iter()
                .any(|h| h["id"] == "claude-code" && h["installed"] == true)
        );
    } else if target == Some("claude") {
        assert_eq!(report["installed"], true);
        assert_eq!(report["lastCompatibleVersion"], "2.1.263");
    }
}

#[cfg(unix)]
#[test]
fn offline_doctor_text_and_json_keep_local_checks_without_network_requests() {
    use std::net::TcpListener;
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().expect("temporary directory");
    let home = directory.path().join("home");
    let bin = directory.path().join("bin");
    let state = directory.path().join("state");
    for path in [&home, &bin, &state] {
        std::fs::create_dir_all(path).expect("isolated directory");
    }
    let executable = bin.join("claude");
    std::fs::write(&executable, "#!/bin/sh\nprintf '%s\\n' 'claude 2.1.251'\n")
        .expect("synthetic harness");
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
        .expect("executable permissions");
    // Invalid credential state would fail live resolution, but must remain untouched offline.
    let credential = state.join("credential.json");
    std::fs::write(&credential, "invalid-private-credential-state").expect("credential sentinel");
    nan_harness_telemetry::consent::TelemetrySettingsStore::new(&state)
        .set(nan_harness_telemetry::consent::TelemetryPreference::On)
        .expect("enable synthetic telemetry to exercise offline suppression");
    let listener = TcpListener::bind("127.0.0.1:0").expect("synthetic endpoint");
    listener
        .set_nonblocking(true)
        .expect("nonblocking listener");
    let endpoint = format!(
        "http://{}",
        listener.local_addr().expect("listener address")
    );
    for target in [None, Some("claude"), Some("chatgpt-desktop")] {
        for json in [false, true] {
            let mut command = Command::new(env!("CARGO_BIN_EXE_nanh"));
            command.env_clear().args(["doctor", "--offline"]);
            if let Some(target) = target {
                command.arg(target);
            }
            if json {
                command.arg("--json");
            }
            let output = command
                .env("HOME", &home)
                .env("USERPROFILE", &home)
                .env("PATH", &bin)
                .env("NAN_HARNESS_CONFIG_DIR", &state)
                .env("NAN_HARNESS_CREDENTIAL_BACKEND", "file")
                .env("NAN_API_KEY", "synthetic-offline-secret")
                .env("NAN_BASE_URL", &endpoint)
                .env(
                    "NAN_COMPATIBILITY_MANIFEST_URL",
                    format!("{endpoint}/compatibility"),
                )
                .env("NAN_UPDATE_MANIFEST_URL", format!("{endpoint}/update"))
                .env(
                    "NAN_HARNESS_GLITCHTIP_DSN",
                    format!(
                        "http://synthetic@{}/1",
                        listener.local_addr().expect("address")
                    ),
                )
                .env("NAN_HARNESS_UMAMI_URL", &endpoint)
                .env("NAN_HARNESS_UMAMI_WEBSITE_ID", "synthetic-website")
                .output()
                .expect("offline doctor should run");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                listener
                    .accept()
                    .expect_err("offline must not connect")
                    .kind(),
                std::io::ErrorKind::WouldBlock
            );
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(!stdout.contains("synthetic-offline-secret"));
            assert!(!stdout.contains(&endpoint));
            if json {
                assert_offline_json(&output, target, directory.path());
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
        std::fs::read_to_string(credential).expect("credential sentinel"),
        "invalid-private-credential-state"
    );
}

#[cfg(unix)]
#[test]
fn offline_doctor_missing_target_remains_an_error() {
    let directory = tempfile::tempdir().expect("isolated home");
    for json in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_nanh"));
        command
            .env_clear()
            .args(["doctor", "claude", "--offline"])
            .env("HOME", directory.path())
            .env("PATH", directory.path())
            .env("NAN_HARNESS_CONFIG_DIR", directory.path().join("state"));
        if json {
            command.arg("--json");
        }
        let output = command.output().expect("doctor runs");
        assert!(!output.status.success());
        if json {
            let report: serde_json::Value =
                serde_json::from_slice(&output.stdout).expect("JSON error report");
            assert_eq!(report["offline"], true);
            assert_eq!(report["level"], "error");
            assert_eq!(report["installed"], false);
        }
    }
}

#[test]
fn offline_doctor_json_preserves_local_configuration_errors() {
    let directory = tempfile::tempdir().expect("isolated state");
    std::fs::write(directory.path().join("configurations.json"), "invalid")
        .expect("invalid state fixture");
    let output = Command::new(env!("CARGO_BIN_EXE_nanh"))
        .env_clear()
        .args(["doctor", "--offline", "--json"])
        .env("HOME", directory.path())
        .env("USERPROFILE", directory.path())
        .env("APPDATA", directory.path().join("appdata"))
        .env("LOCALAPPDATA", directory.path().join("localappdata"))
        .env("PATH", directory.path())
        .env("NAN_HARNESS_CONFIG_DIR", directory.path())
        .output()
        .expect("doctor runs");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON report");
    assert!(!output.status.success());
    assert_eq!(report["provider"]["level"], "info");
    assert_eq!(report["managedConfigurations"]["level"], "error");
    assert!(report["managedConfigurations"]["errorCode"].is_string());
}

#[test]
fn offline_doctor_uses_cached_desktop_evidence_without_refreshing_it() {
    use sha2::{Digest as _, Sha256};
    let directory = tempfile::tempdir().expect("isolated state");
    let endpoint = "http://127.0.0.1:1/synthetic-feed";
    let fingerprint =
        Sha256::digest(endpoint.as_bytes())
            .iter()
            .fold(String::new(), |mut fingerprint, byte| {
                use std::fmt::Write as _;
                write!(fingerprint, "{byte:02x}").expect("write fingerprint to string");
                fingerprint
            });
    let cache = serde_json::json!({
        "schemaVersion": 3,
        "sourceFingerprint": fingerprint,
        "lastCheckedUnixSeconds": 1,
        "cachedManifest": {
            "schemaVersion": 3,
            "releases": [{
                "nanHarnessVersion": env!("CARGO_PKG_VERSION"),
                "verifications": [],
                "desktopVerifications": [{
                    "id": "chatgpt-desktop", "platform": "macos", "evidence": "live-verified",
                    "lastCompatibleAppVersion": "26.831.21537", "lastCompatibleRuntimeVersion": "0.152.0",
                    "compatibleAt": "2026-09-07T00:00:00Z"
                }]
            }]
        }
    });
    let path = directory.path().join("compatibility-v3.json");
    let original = serde_json::to_vec(&cache).expect("cache bytes");
    std::fs::write(&path, &original).expect("cached evidence");
    let output = Command::new(env!("CARGO_BIN_EXE_nanh"))
        .env_clear()
        .args(["doctor", "chatgpt-desktop", "--offline", "--json"])
        .env("HOME", directory.path())
        .env("USERPROFILE", directory.path())
        .env("APPDATA", directory.path().join("appdata"))
        .env("LOCALAPPDATA", directory.path().join("localappdata"))
        .env("PATH", directory.path())
        .env("NAN_HARNESS_CONFIG_DIR", directory.path())
        .env("NAN_COMPATIBILITY_MANIFEST_URL", endpoint)
        .output()
        .expect("doctor runs");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON report");
    assert!(output.status.success());
    assert_eq!(report["offline"], true);
    if cfg!(target_os = "macos") {
        assert_eq!(report["evidenceSource"], "remote-feed");
        assert_eq!(report["compatibleAt"], "2026-09-07T00:00:00Z");
    }
    assert_eq!(std::fs::read(path).expect("unchanged cache"), original);
}
