//! Closed observations of disposable configuration; presence is not consumption.
use super::DesktopPaths;
use serde::Serialize;
use serde_json::Value;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use zeroize::{Zeroize as _, Zeroizing};

const MAX_DOCUMENT: u64 = 1024 * 1024;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Facts {
    schema_version: u8,
    mechanism: &'static str,
    diagnostics_only: bool,
    configuration_present: bool,
    object_schema: bool,
    #[serde(flatten)]
    selection: ProfileSelection,
    #[serde(flatten)]
    connection: GatewayConnection,
    #[serde(flatten)]
    features: ChatFeatures,
    native_path_alignment: Option<bool>,
    configuration_consumed: Option<bool>,
    model_discovery_seen: Option<bool>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProfileSelection {
    deployment_mode_matches: bool,
    profile_matches: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GatewayConnection {
    provider_gateway: bool,
    loopback_base_url_matches: bool,
    auth_matches: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatFeatures {
    model_discovery_enabled: bool,
    chat_enabled: bool,
    chooser_disabled: bool,
}

fn private_directory(path: &Path) -> bool {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if !path.is_absolute() || !metadata.is_dir() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        metadata.permissions().mode().trailing_zeros() >= 6
    }
    #[cfg(not(unix))]
    {
        nan_harness_private_fs::restrict_path(
            path,
            nan_harness_private_fs::PrivatePathKind::Directory,
        )
        .is_ok()
    }
}

fn owned_document(path: &Path, profile: &Path) -> Option<Value> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_DOCUMENT {
        return None;
    }
    let parent = path.parent()?.canonicalize().ok()?;
    if !parent.starts_with(profile) {
        return None;
    }
    let file = nan_harness_private_fs::open_private_read(path).ok()?.0;
    let mut bytes = Zeroizing::new(Vec::new());
    file.take(MAX_DOCUMENT + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > MAX_DOCUMENT {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

fn observations(documents: &[Option<Value>; 4], base_url: &str, token: &str) -> Facts {
    let profile = documents[3].as_ref();
    let matches = |key: &str, expected: &Value| profile.and_then(|p| p.get(key)) == Some(expected);
    let loopback = url::Url::parse(base_url).is_ok_and(|url| {
        url.scheme() == "http"
            && url.host_str() == Some("127.0.0.1")
            && url.username().is_empty()
            && url.password().is_none()
    });
    Facts {
        schema_version: 1,
        mechanism: "claude-owned-configuration",
        diagnostics_only: true,
        configuration_present: documents.iter().all(Option::is_some),
        object_schema: documents
            .iter()
            .all(|d| d.as_ref().is_some_and(Value::is_object)),
        selection: ProfileSelection {
            deployment_mode_matches: documents[..2].iter().all(|d| {
                d.as_ref()
                    .and_then(|v| v.get("deploymentMode"))
                    .and_then(Value::as_str)
                    == Some("3p")
            }),
            profile_matches: documents[2]
                .as_ref()
                .and_then(|d| d.get("appliedId"))
                .and_then(Value::as_str)
                == Some(super::PROFILE_ID),
        },
        connection: GatewayConnection {
            provider_gateway: matches("inferenceProvider", &Value::from("gateway")),
            loopback_base_url_matches: loopback
                && matches("inferenceGatewayBaseUrl", &Value::from(base_url)),
            auth_matches: !token.is_empty()
                && profile
                    .and_then(|p| p.get("inferenceGatewayApiKey"))
                    .and_then(Value::as_str)
                    == Some(token)
                && matches("inferenceGatewayAuthScheme", &Value::from("bearer")),
        },
        features: ChatFeatures {
            model_discovery_enabled: matches("modelDiscoveryEnabled", &Value::Bool(true)),
            chat_enabled: matches("chatTabEnabled", &Value::Bool(true)),
            chooser_disabled: matches("disableDeploymentModeChooser", &Value::Bool(true)),
        },
        native_path_alignment: None,
        configuration_consumed: None,
        model_discovery_seen: None,
    }
}

async fn native_alignment() -> Option<bool> {
    use tokio::io::AsyncReadExt as _;
    if !cfg!(target_os = "macos") {
        return None;
    }
    // Query Foundation using the exact inherited environment, without opening
    // either the returned native path or any application/user configuration.
    let script = r#"import Foundation
let expected = ProcessInfo.processInfo.environment["HOME"].map { $0 + "/Library/Application Support" }
if let actual = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first?.path, let expected = expected { print(actual == expected ? "true" : "false") }
"#;
    let mut command = tokio::process::Command::new("/usr/bin/swift");
    command
        .args(["-e", script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = command.spawn().ok()?;
    let stdout = child.stdout.take()?;
    let operation = async {
        let mut output = Vec::new();
        stdout.take(16).read_to_end(&mut output).await.ok()?;
        if !child.wait().await.ok()?.success() {
            return None;
        }
        match output.as_slice() {
            b"true\n" => Some(true),
            b"false\n" => Some(false),
            _ => None,
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), operation)
        .await
        .ok()
        .flatten()
}

pub(super) async fn record(paths: &DesktopPaths, base_url: &str, token: &str) {
    let expected_os = if cfg!(target_os = "macos") {
        "macOS"
    } else if cfg!(windows) {
        "Windows"
    } else {
        "Linux"
    };
    if std::env::var("RUNNER_OS").as_deref() != Ok(expected_os)
        || std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
    {
        return;
    }
    let Some(directory) = std::env::var_os("NANH_DESKTOP_QUALIFICATION_FACTS").map(PathBuf::from)
    else {
        return;
    };
    let Ok(workspace) = std::env::current_dir() else {
        return;
    };
    let profile = workspace.join("profile");
    if !private_directory(&directory) || !private_directory(&profile) {
        return;
    }
    let Ok(profile) = profile.canonicalize() else {
        return;
    };
    let owned_root = if std::env::var("NANH_CLAUDE_MAC_PROFILE_POLICY").as_deref()
        == Ok("native-known-folders")
    {
        if !cfg!(target_os = "macos") || native_alignment().await != Some(true) {
            return;
        }
        let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
            return;
        };
        let support = home.join("Library/Application Support");
        if paths.normal_config != support.join("Claude/claude_desktop_config.json")
            || paths.third_party_config != support.join("Claude-3p/claude_desktop_config.json")
            || !private_directory(&support.join("Claude"))
            || !private_directory(&support.join("Claude-3p"))
        {
            return;
        }
        support
    } else {
        profile
    };
    if paths.documents().iter().any(|p| {
        p.parent()
            .and_then(|parent| parent.canonicalize().ok())
            .is_none_or(|parent| !parent.starts_with(&owned_root))
    }) {
        return;
    }
    let mut documents = paths
        .documents()
        .map(|path| owned_document(path, &owned_root));
    let mut facts = observations(&documents, base_url, token);
    for document in documents.iter_mut().flatten() {
        if let Some(Value::String(credential)) = document.get_mut("inferenceGatewayApiKey") {
            credential.zeroize();
        }
    }
    facts.native_path_alignment = native_alignment().await;
    let path = directory.join(format!(
        "claude-owned-configuration-{}.json",
        std::process::id()
    ));
    if let Ok(file) = nan_harness_private_fs::open_private_new(&path) {
        let _ = serde_json::to_writer(file, &facts);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn documents() -> [Option<Value>; 4] {
        [
            Some(json!({"deploymentMode":"3p"})),
            Some(json!({"deploymentMode":"3p"})),
            Some(json!({"appliedId":super::super::PROFILE_ID})),
            Some(json!({
            "inferenceProvider":"gateway", "inferenceGatewayBaseUrl":"http://127.0.0.1:1234/v1",
            "inferenceGatewayApiKey":"synthetic-private-token", "inferenceGatewayAuthScheme":"bearer",
            "modelDiscoveryEnabled":true, "chatTabEnabled":true, "disableDeploymentModeChooser":true})),
        ]
    }

    #[test]
    fn presence_does_not_certify_consumption_or_expose_configuration() {
        let value = serde_json::to_value(observations(
            &documents(),
            "http://127.0.0.1:1234/v1",
            "synthetic-private-token",
        ))
        .unwrap();
        for field in [
            "configurationPresent",
            "objectSchema",
            "deploymentModeMatches",
            "profileMatches",
            "providerGateway",
            "loopbackBaseUrlMatches",
            "authMatches",
            "modelDiscoveryEnabled",
            "chatEnabled",
            "chooserDisabled",
        ] {
            assert_eq!(value[field], true, "{field}");
        }
        for field in [
            "configurationConsumed",
            "modelDiscoverySeen",
            "nativePathAlignment",
        ] {
            assert!(value[field].is_null());
        }
        let text = value.to_string();
        for secret in [
            "synthetic-private-token",
            "127.0.0.1",
            super::super::PROFILE_ID,
        ] {
            assert!(!text.contains(secret));
        }
        let mut wrong = documents();
        wrong[3].as_mut().unwrap()["inferenceGatewayApiKey"] = json!("stale");
        let mismatch = observations(
            &wrong,
            "http://127.0.0.1:1234/v1",
            "synthetic-private-token",
        );
        assert!(!mismatch.connection.auth_matches);
        assert!(
            !observations(
                &documents(),
                "https://remote.invalid/v1",
                "synthetic-private-token"
            )
            .connection
            .loopback_base_url_matches
        );
        wrong[2] = None;
        assert!(
            !observations(
                &wrong,
                "http://127.0.0.1:1234/v1",
                "synthetic-private-token"
            )
            .configuration_present
        );
    }

    #[test]
    fn document_reads_reject_outside_profile_and_oversized_payloads() {
        let directory = tempfile::tempdir().unwrap();
        let profile = directory.path().join("profile");
        std::fs::create_dir(&profile).unwrap();
        let profile = profile.canonicalize().unwrap();
        let valid = profile.join("valid.json");
        let file = nan_harness_private_fs::open_private_new(&valid).unwrap();
        serde_json::to_writer(file, &json!({"chatTabEnabled":true})).unwrap();
        assert_eq!(
            owned_document(&valid, &profile),
            Some(json!({"chatTabEnabled":true}))
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                valid.metadata().unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let outside = directory.path().join("outside.json");
        std::fs::write(&outside, b"{} private").unwrap();
        assert!(owned_document(&outside, &profile).is_none());
        let large = profile.join("large.json");
        std::fs::File::create(&large)
            .unwrap()
            .set_len(MAX_DOCUMENT + 1)
            .unwrap();
        assert!(owned_document(&large, &profile).is_none());
        #[cfg(unix)]
        {
            let link = profile.join("link.json");
            std::os::unix::fs::symlink(&outside, &link).unwrap();
            assert!(owned_document(&link, &profile).is_none());
        }
    }
}
