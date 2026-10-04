#[allow(clippy::wildcard_imports)]
use super::*;
use qualification_prelaunch::{
    ConfigurationDocument, ConfigurationSubstage as Substage, observe_configuration,
};

pub(super) fn apply_gateway(
    paths: &DesktopPaths,
    base_url: &str,
    token: &str,
) -> Result<(), ClaudeDesktopError> {
    #[cfg(feature = "desktop-qualification")]
    let chat_only = observe_configuration(
        match std::env::var("NANH_CLAUDE_MAC_CHAT_NAVIGATION") {
            Err(std::env::VarError::NotPresent) => Ok(false),
            Ok(value)
                if value == "1" && qualification_config::observation_directory(paths).is_some() =>
            {
                Ok(true)
            }
            _ => Err(ClaudeDesktopError::InvalidStatePath),
        },
        Substage::MacPolicy,
        None,
    )?;
    #[cfg(feature = "desktop-qualification")]
    let windows_chat_only = observe_configuration(
        (|| {
            windows_chat_trial(
                std::env::var("NANH_CLAUDE_WINDOWS_CHAT_ONLY")
                    .map(Some)
                    .or_else(|error| match error {
                        std::env::VarError::NotPresent => Ok(None),
                        std::env::VarError::NotUnicode(_) => {
                            Err(ClaudeDesktopError::InvalidStatePath)
                        }
                    })?
                    .as_deref(),
                cfg!(windows),
                std::env::var("RUNNER_OS").ok().as_deref(),
                std::env::var("NANH_CLAUDE_WINDOWS_PROFILE_POLICY")
                    .ok()
                    .as_deref(),
                qualification_config::observation_directory(paths).is_some(),
            )
        })(),
        Substage::WindowsPolicy,
        None,
    )?;
    #[cfg(feature = "desktop-qualification")]
    let chat_only = chat_only || windows_chat_only;
    #[cfg(feature = "desktop-qualification")]
    let linux_chat_only = observe_configuration(
        qualification_linux::requested(paths),
        Substage::LinuxPolicy,
        None,
    )?;
    #[cfg(feature = "desktop-qualification")]
    let chat_only = chat_only || linux_chat_only;
    let mut documents = read_documents(paths)?;
    documents[0].insert("deploymentMode".to_owned(), json!("3p"));
    documents[1].insert("deploymentMode".to_owned(), json!("3p"));

    documents[2].insert("appliedId".to_owned(), json!(PROFILE_ID));
    let entries = documents[2]
        .remove("entries")
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    let mut entries = entries
        .into_iter()
        .filter(|entry| entry.get("id").and_then(Value::as_str) != Some(PROFILE_ID))
        .collect::<Vec<_>>();
    entries.push(json!({"id": PROFILE_ID, "name": PROFILE_NAME}));
    documents[2].insert("entries".to_owned(), Value::Array(entries));

    let profile = &mut documents[3];
    profile.insert("inferenceProvider".to_owned(), json!("gateway"));
    profile.insert("inferenceGatewayBaseUrl".to_owned(), json!(base_url));
    profile.insert("inferenceGatewayApiKey".to_owned(), json!(token));
    profile.insert("inferenceGatewayAuthScheme".to_owned(), json!("bearer"));
    profile.insert("deploymentDisplayName".to_owned(), json!(PROFILE_NAME));
    profile.insert("modelDiscoveryEnabled".to_owned(), json!(true));
    profile.insert("chatTabEnabled".to_owned(), json!(true));
    profile.insert("autoModeEnabled".to_owned(), json!(true));
    profile.insert("disableDeploymentModeChooser".to_owned(), json!(true));
    profile.insert("coworkEgressAllowedHosts".to_owned(), json!(["*"]));
    profile.remove("inferenceModels");
    #[cfg(feature = "desktop-qualification")]
    configure_chat_trial(profile, chat_only);
    #[cfg(feature = "desktop-qualification")]
    observe_configuration(
        qualification_mcp::configure(paths, profile),
        Substage::ManagedMcp,
        Some(ConfigurationDocument::Profile),
    )?;

    write_documents(paths, documents)
}

fn read_documents(paths: &DesktopPaths) -> Result<Vec<Map<String, Value>>, ClaudeDesktopError> {
    let documents = paths
        .documents()
        .into_iter()
        .zip(ConfigurationDocument::all())
        .map(|(path, document)| {
            observe_configuration(
                read_json_object(path),
                Substage::DocumentRead,
                Some(document),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(documents)
}

fn write_documents(
    paths: &DesktopPaths,
    documents: Vec<Map<String, Value>>,
) -> Result<(), ClaudeDesktopError> {
    for ((document, path), kind) in documents
        .into_iter()
        .zip(paths.documents())
        .zip(ConfigurationDocument::all())
    {
        let mut payload = observe_configuration(
            serde_json::to_vec_pretty(&document).map_err(ClaudeDesktopError::SerializeConfig),
            Substage::Serialize,
            Some(kind),
        )?;
        payload.push(b'\n');
        let permissions = observe_configuration(
            existing_permissions(path),
            Substage::ExistingPermissions,
            Some(kind),
        )?;
        atomic_write_configuration(path, &payload, permissions.as_ref(), kind)?;
    }
    Ok(())
}

pub(super) fn read_json_object(path: &Path) -> Result<Map<String, Value>, ClaudeDesktopError> {
    reject_symlink(path)?;
    match fs::read(path) {
        Ok(contents) => serde_json::from_slice::<Value>(&contents)
            .map_err(ClaudeDesktopError::ParseConfig)?
            .as_object()
            .cloned()
            .ok_or(ClaudeDesktopError::ConfigRoot),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(Map::new()),
        Err(error) => Err(ClaudeDesktopError::ReadConfig(error)),
    }
}

pub(super) fn existing_permissions(path: &Path) -> Result<Option<Permissions>, ClaudeDesktopError> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(Some(metadata.permissions())),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ClaudeDesktopError::ReadConfig(error)),
    }
}

#[cfg(feature = "desktop-qualification")]
fn configure_chat_trial(profile: &mut Map<String, Value>, requested: bool) {
    if requested {
        // Official managed configuration disables the Cowork landing surface.
        // The enclosing receipt restores the original document after the trial.
        profile.insert("coworkTabEnabled".to_owned(), json!(false));
    }
}

#[cfg(feature = "desktop-qualification")]
fn windows_chat_trial(
    value: Option<&str>,
    windows_host: bool,
    runner_os: Option<&str>,
    profile_policy: Option<&str>,
    owned_observation: bool,
) -> Result<bool, ClaudeDesktopError> {
    match value {
        None => Ok(false),
        Some("1")
            if windows_host
                && runner_os == Some("Windows")
                && profile_policy == Some("private-env")
                && owned_observation =>
        {
            Ok(true)
        }
        _ => Err(ClaudeDesktopError::InvalidStatePath),
    }
}

#[cfg(all(test, feature = "desktop-qualification"))]
mod qualification_tests {
    use super::*;

    #[test]
    fn windows_chat_only_requires_owned_hosted_private_profile_and_explicit_flag() {
        assert!(!windows_chat_trial(None, false, None, None, false).unwrap());
        assert!(
            windows_chat_trial(Some("1"), true, Some("Windows"), Some("private-env"), true)
                .unwrap()
        );
        for (value, host, os, profile, owned) in [
            (Some("0"), true, Some("Windows"), Some("private-env"), true),
            (Some("1"), false, Some("Windows"), Some("private-env"), true),
            (Some("1"), true, Some("macOS"), Some("private-env"), true),
            (Some("1"), true, Some("Windows"), None, true),
            (Some("1"), true, Some("Windows"), Some("private-env"), false),
        ] {
            assert!(windows_chat_trial(value, host, os, profile, owned).is_err());
        }
    }

    #[test]
    fn chat_trial_preserves_default_and_uses_supported_managed_field() {
        let mut profile = Map::new();
        profile.insert("coworkTabEnabled".to_owned(), json!(true));
        profile.insert("retainedSetting".to_owned(), json!("synthetic"));
        configure_chat_trial(&mut profile, false);
        assert_eq!(profile["coworkTabEnabled"], true);
        configure_chat_trial(&mut profile, true);
        assert_eq!(profile["coworkTabEnabled"], false);
        assert_eq!(profile["retainedSetting"], "synthetic");
    }
}
