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

pub(super) fn private_directory(path: &Path) -> bool {
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

pub(super) fn windows_roots(paths: &DesktopPaths) -> Option<[PathBuf; 2]> {
    if !cfg!(windows)
        || std::env::var("RUNNER_OS").as_deref() != Ok("Windows")
        || std::env::var("NANH_CLAUDE_WINDOWS_PROFILE_POLICY").as_deref() != Ok("private-env")
        || std::env::var("NANH_DESKTOP_QUALIFICATION_MODE").as_deref() != Ok("startup-baseline")
        || std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
        || std::env::var_os("CLAUDE_USER_DATA_DIR").is_some()
        || std::env::var_os("CLAUDE_CDP_AUTH").is_some()
    {
        return None;
    }
    let workspace = std::env::current_dir().ok()?;
    let home = workspace.join("profile").join("home");
    let local = home.join("AppData").join("Local");
    let roaming = home.join("AppData").join("Roaming");
    for (key, expected) in [
        ("HOME", &home),
        ("USERPROFILE", &home),
        ("APPDATA", &roaming),
        ("LOCALAPPDATA", &local),
    ] {
        if !std::env::var_os(key)
            .map(PathBuf::from)
            .is_some_and(|actual| same_directory(&actual, expected))
        {
            return None;
        }
    }
    validated_windows_roots(paths, &workspace, &roaming, &local)
}

fn validated_windows_roots(
    paths: &DesktopPaths,
    workspace: &Path,
    roaming: &Path,
    local: &Path,
) -> Option<[PathBuf; 2]> {
    let home = workspace.join("profile").join("home");
    if !same_directory(roaming, &home.join("AppData").join("Roaming"))
        || !same_directory(local, &home.join("AppData").join("Local"))
    {
        return None;
    }
    let roots = [roaming.join("Claude"), local.join("Claude-3p")];
    if !documents_match_windows_roots(paths, &roots) {
        return None;
    }
    let owned = workspace.join("profile").canonicalize().ok()?;
    if roots.iter().any(|root| {
        root.canonicalize()
            .ok()
            .is_none_or(|root| !root.starts_with(&owned))
    }) {
        return None;
    }
    Some(roots)
}

// Windows current_dir and canonicalize may use different drive-prefix spellings.
// Compare existing directory identities, while rejecting any reparse ancestor.
fn same_directory(actual: &Path, expected: &Path) -> bool {
    fn canonical_directory(path: &Path) -> Option<PathBuf> {
        if !path.is_absolute()
            || path.ancestors().count() > 64
            || path
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return None;
        }
        for ancestor in path.ancestors() {
            let metadata = std::fs::symlink_metadata(ancestor).ok()?;
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return None;
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt as _;
                if metadata.file_attributes() & 0x400 != 0 {
                    return None;
                }
            }
        }
        path.canonicalize().ok()
    }
    canonical_directory(actual)
        .zip(canonical_directory(expected))
        .is_some_and(|(actual, expected)| actual == expected)
}

fn documents_match_windows_roots(paths: &DesktopPaths, roots: &[PathBuf; 2]) -> bool {
    fn matches(path: &Path, root: &Path, leaf: &str, library: bool) -> bool {
        if path.file_name() != Some(std::ffi::OsStr::new(leaf)) {
            return false;
        }
        let Some(mut parent) = path.parent() else {
            return false;
        };
        if library {
            if parent.file_name() != Some(std::ffi::OsStr::new("configLibrary")) {
                return false;
            }
            if let Ok(metadata) = std::fs::symlink_metadata(parent) {
                if !metadata.is_dir() || metadata.file_type().is_symlink() {
                    return false;
                }
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt as _;
                    if metadata.file_attributes() & 0x400 != 0 {
                        return false;
                    }
                }
            } else if !matches!(parent.try_exists(), Ok(false)) {
                return false;
            }
            let Some(root_parent) = parent.parent() else {
                return false;
            };
            parent = root_parent;
        }
        same_directory(parent, root)
    }
    matches(
        &paths.normal_config,
        &roots[0],
        "claude_desktop_config.json",
        false,
    ) && matches(
        &paths.third_party_config,
        &roots[1],
        "claude_desktop_config.json",
        false,
    ) && matches(&paths.meta, &roots[1], "_meta.json", true)
        && matches(
            &paths.profile,
            &roots[1],
            &format!("{}.json", super::PROFILE_ID),
            true,
        )
}

fn documents_match_roots(paths: &DesktopPaths, roots: &[PathBuf; 2]) -> bool {
    paths.normal_config == roots[0].join("claude_desktop_config.json")
        && paths.third_party_config == roots[1].join("claude_desktop_config.json")
        && paths.meta == roots[1].join("configLibrary/_meta.json")
        && paths.profile == roots[1].join(format!("configLibrary/{}.json", super::PROFILE_ID))
}

fn mac_observation_scope(paths: &DesktopPaths) -> Option<()> {
    if !cfg!(target_os = "macos") || std::env::var("RUNNER_OS").as_deref() != Ok("macOS") {
        return None;
    }
    let policy = std::env::var("NANH_CLAUDE_MAC_PROFILE_POLICY").ok()?;
    if !matches!(
        policy.as_str(),
        "native-known-folders" | "electron-user-data-dir"
    ) {
        return None;
    }
    let workspace = std::env::current_dir().ok()?;
    let profile = workspace.join("profile");
    let home = PathBuf::from(std::env::var_os("HOME")?);
    if !private_directory(&profile)
        || (policy == "electron-user-data-dir" && home != profile.join("home"))
    {
        return None;
    }
    let support = home.join("Library/Application Support");
    let roots = [support.join("Claude"), support.join("Claude-3p")];
    if !documents_match_roots(paths, &roots) || roots.iter().any(|root| !private_directory(root)) {
        return None;
    }
    Some(())
}

pub(super) fn observation_directory(paths: &DesktopPaths) -> Option<PathBuf> {
    if std::env::var("NANH_DESKTOP_QUALIFICATION_MODE").as_deref() != Ok("startup-baseline")
        || std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
    {
        return None;
    }
    let supported = if cfg!(windows) {
        windows_roots(paths).is_some()
    } else {
        mac_observation_scope(paths).is_some()
    };
    if !supported {
        return None;
    }
    let directory = PathBuf::from(std::env::var_os("NANH_DESKTOP_QUALIFICATION_FACTS")?);
    if !private_directory(&directory) {
        return None;
    }
    Some(directory)
}

// Qualification-only private authority channel; never place this in public facts.
pub(super) fn write_bridge_receipt(
    paths: &DesktopPaths,
    base_url: &str,
    token: &str,
) -> Result<(), super::ClaudeDesktopError> {
    if !cfg!(windows) || std::env::var("NANH_CLAUDE_WINDOWS_CHAT_ONLY").as_deref() != Ok("1") {
        return Ok(());
    }
    let roots = windows_roots(paths).ok_or(super::ClaudeDesktopError::InvalidStatePath)?;
    write_private_bridge(&roots[1].join(".nanh-bridge.private"), base_url, token)
}

fn write_private_bridge(
    path: &Path,
    base_url: &str,
    token: &str,
) -> Result<(), super::ClaudeDesktopError> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct BridgeReceipt<'a> {
        schema_version: u8,
        process_id: u32,
        base_url: &'a str,
        token: &'a str,
    }
    let file =
        nan_harness_private_fs::open_private_new(path).map_err(super::ClaudeDesktopError::Write)?;
    serde_json::to_writer(
        &file,
        &BridgeReceipt {
            schema_version: 1,
            process_id: std::process::id(),
            base_url,
            token,
        },
    )
    .map_err(super::ClaudeDesktopError::SerializeConfig)?;
    file.sync_all().map_err(super::ClaudeDesktopError::Write)
}

pub(super) async fn record(paths: &DesktopPaths, base_url: &str, token: &str) {
    if cfg!(windows) && windows_roots(paths).is_none() {
        return;
    }
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
    fn bridge_receipt_is_private_exclusive_and_separate_from_public_facts() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(".nanh-bridge.private");
        write_private_bridge(&path, "http://127.0.0.1:43210", "synthetic-secret").unwrap();
        let original = std::fs::read(&path).unwrap();
        let value: Value = serde_json::from_slice(&original).unwrap();
        assert_eq!(value["processId"], std::process::id());
        assert_eq!(value["baseUrl"], "http://127.0.0.1:43210");
        assert_eq!(value["token"], "synthetic-secret");
        assert!(write_private_bridge(&path, "http://127.0.0.1:43211", "replacement").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_ne!(path.extension().unwrap(), "json");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(path.metadata().unwrap().permissions().mode() & 0o777, 0o600);
        }
    }

    #[test]
    fn windows_binding_rejects_foreign_roots_and_profile_documents() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().canonicalize().unwrap();
        let roaming = workspace
            .join("profile")
            .join("home")
            .join("AppData")
            .join("Roaming");
        let local = workspace
            .join("profile")
            .join("home")
            .join("AppData")
            .join("Local");
        for path in [roaming.join("Claude"), local.join("Claude-3p")] {
            nan_harness_private_fs::create_private_dir_all(&path).unwrap();
        }
        let mut paths = DesktopPaths::new(
            &roaming.join("Claude"),
            &local.join("Claude-3p"),
            &workspace.join("profile").join("nanh"),
        );
        assert!(validated_windows_roots(&paths, &workspace, &roaming, &local).is_some());
        assert!(validated_windows_roots(&paths, &workspace, &local, &roaming).is_none());
        paths.profile = workspace.join("foreign-profile.json");
        assert!(validated_windows_roots(&paths, &workspace, &roaming, &local).is_none());
    }

    #[cfg(windows)]
    #[test]
    fn windows_binding_accepts_drive_prefix_alias_but_rejects_foreign_layout() {
        let temp = tempfile::tempdir().unwrap();
        let verbatim = temp.path().canonicalize().unwrap();
        let text = verbatim.to_str().unwrap();
        let ordinary = PathBuf::from(text.strip_prefix(r"\\?\").unwrap());
        let home = verbatim.join("profile").join("home");
        let local = home.join("AppData").join("Local");
        let roaming = home.join("AppData").join("Roaming");
        for root in [local.join("Claude-3p"), roaming.join("Claude")] {
            nan_harness_private_fs::create_private_dir_all(&root).unwrap();
        }
        let paths = DesktopPaths::new(&roaming.join("Claude"), &local.join("Claude-3p"), &home);
        assert!(validated_windows_roots(&paths, &ordinary, &roaming, &local).is_some());
        assert!(same_directory(&ordinary, &verbatim));
        assert!(!same_directory(&ordinary, &local));
        assert_eq!(
            paths.meta.parent().unwrap().file_name().unwrap(),
            "configLibrary"
        );
        let mut foreign = paths;
        foreign.meta = local.join("Claude-3p").join("other").join("_meta.json");
        assert!(validated_windows_roots(&foreign, &ordinary, &roaming, &local).is_none());
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
