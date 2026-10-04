//! Payload-free failure boundaries for an explicitly opted-in private hosted trial.

#[derive(Clone, Copy)]
pub(super) enum Stage {
    Persistence,
    RememberedModel,
    Paths,
    SessionLock,
    PendingRecovery,
    ProcessQuery,
    ProcessPresent,
    Credentials,
    Bridge,
    Snapshot,
    ReceiptWrite,
    Configuration,
    VendorLaunch,
}

#[derive(Clone, Copy)]
pub(super) enum ConfigurationSubstage {
    #[cfg(feature = "desktop-qualification")]
    MacPolicy,
    #[cfg(feature = "desktop-qualification")]
    WindowsPolicy,
    #[cfg(feature = "desktop-qualification")]
    LinuxPolicy,
    DocumentRead,
    #[cfg(feature = "desktop-qualification")]
    ManagedMcp,
    Serialize,
    ExistingPermissions,
    ParentCreate,
    PathCheck,
    TemporaryCreate,
    TemporaryWrite,
    TemporaryPermissions,
    Persist,
}

#[derive(Clone, Copy)]
pub(super) enum ConfigurationDocument {
    NormalConfig,
    ThirdPartyConfig,
    Metadata,
    Profile,
}

impl ConfigurationDocument {
    pub(super) fn all() -> [Self; 4] {
        [
            Self::NormalConfig,
            Self::ThirdPartyConfig,
            Self::Metadata,
            Self::Profile,
        ]
    }
}

pub(super) fn observe_configuration<T, E>(
    result: Result<T, E>,
    substage: ConfigurationSubstage,
    document: Option<ConfigurationDocument>,
) -> Result<T, E> {
    if result.is_err() {
        emit_at(Stage::Configuration, Some((substage, document)));
    }
    result
}

pub(super) fn observe<T, E>(result: Result<T, E>, stage: Stage) -> Result<T, E> {
    if result.is_err() {
        emit(stage);
    }
    result
}

fn emit(stage: Stage) {
    emit_at(stage, None);
}

fn emit_at(
    stage: Stage,
    configuration: Option<(ConfigurationSubstage, Option<ConfigurationDocument>)>,
) {
    #[cfg(feature = "desktop-qualification")]
    if enabled() {
        let stage = match stage {
            Stage::Persistence => "persistence",
            Stage::RememberedModel => "remembered-model",
            Stage::Paths => "paths",
            Stage::SessionLock => "session-lock",
            Stage::PendingRecovery => "pending-recovery",
            Stage::ProcessQuery => "process-query",
            Stage::ProcessPresent => "process-present",
            Stage::Credentials => "credentials",
            Stage::Bridge => "bridge",
            Stage::Snapshot => "snapshot",
            Stage::ReceiptWrite => "receipt-write",
            Stage::Configuration => "configuration",
            Stage::VendorLaunch => "vendor-launch",
        };
        let mut record = serde_json::json!({"schemaVersion":1,"mechanism":"claude-cli-prelaunch",
            "phase":"prelaunch","stage":stage,"status":"failed","diagnosticsOnly":true});
        if let Some((substage, document)) = configuration {
            record["configurationSubstage"] = serde_json::json!(substage.as_str());
            if let Some(document) = document {
                record["configurationDocument"] = serde_json::json!(document.as_str());
            }
        }
        if let Some(directory) = facts_directory() {
            write_record(&directory, &record);
        }
    }
    #[cfg(not(feature = "desktop-qualification"))]
    let _ = (stage, configuration);
}

#[cfg(feature = "desktop-qualification")]
fn write_record(directory: &std::path::Path, record: &serde_json::Value) {
    // The detailed inner boundary wins; outer observations cannot replace it.
    if let Ok(mut file) =
        nan_harness_private_fs::open_private_new(&directory.join("claude-cli-prelaunch.json"))
    {
        let _ = serde_json::to_writer(&mut file, record);
    }
}

#[cfg(feature = "desktop-qualification")]
impl ConfigurationSubstage {
    fn as_str(self) -> &'static str {
        match self {
            Self::MacPolicy => "mac-policy",
            Self::WindowsPolicy => "windows-policy",
            Self::LinuxPolicy => "linux-policy",
            Self::DocumentRead => "document-read",
            Self::ManagedMcp => "managed-mcp",
            Self::Serialize => "serialize",
            Self::ExistingPermissions => "existing-permissions",
            Self::ParentCreate => "parent-create",
            Self::PathCheck => "path-check",
            Self::TemporaryCreate => "temporary-create",
            Self::TemporaryWrite => "temporary-write",
            Self::TemporaryPermissions => "temporary-permissions",
            Self::Persist => "persist",
        }
    }
}
#[cfg(feature = "desktop-qualification")]
impl ConfigurationDocument {
    fn as_str(self) -> &'static str {
        match self {
            Self::NormalConfig => "normal-config",
            Self::ThirdPartyConfig => "third-party-config",
            Self::Metadata => "metadata",
            Self::Profile => "profile",
        }
    }
}

#[cfg(feature = "desktop-qualification")]
fn facts_directory() -> Option<std::path::PathBuf> {
    let directory = std::path::PathBuf::from(std::env::var_os("NANH_DESKTOP_QUALIFICATION_FACTS")?);
    let metadata = std::fs::symlink_metadata(&directory).ok()?;
    if metadata.file_type().is_symlink()
        || !directory.is_absolute()
        || !super::qualification_config::private_directory(&directory)
    {
        return None;
    }
    Some(directory)
}

#[cfg(feature = "desktop-qualification")]
fn enabled() -> bool {
    enabled_values(cfg!(windows), |key| std::env::var(key).ok())
}

#[cfg(any(feature = "desktop-qualification", test))]
fn enabled_values(windows: bool, value: impl Fn(&str) -> Option<String>) -> bool {
    windows
        && [
            ("NANH_CLAUDE_PRELAUNCH_DIAGNOSTICS", "1"),
            ("GITHUB_ACTIONS", "true"),
            ("RUNNER_ENVIRONMENT", "github-hosted"),
            ("RUNNER_OS", "Windows"),
            ("NANH_CLAUDE_WINDOWS_PROFILE_POLICY", "private-env"),
            ("NANH_DESKTOP_QUALIFICATION_MODE", "startup-baseline"),
        ]
        .into_iter()
        .all(|(key, expected)| value(key).as_deref() == Some(expected))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn boundary_preserves_original_error_and_success_values() {
        assert_eq!(
            observe_configuration::<(), _>(
                Err("PRIVATE_SENTINEL"),
                ConfigurationSubstage::Persist,
                Some(ConfigurationDocument::Profile)
            ),
            Err("PRIVATE_SENTINEL")
        );
        assert_eq!(
            observe_configuration::<_, ()>(
                Ok(91),
                ConfigurationSubstage::DocumentRead,
                Some(ConfigurationDocument::NormalConfig)
            ),
            Ok(91)
        );
        assert_eq!(observe::<(), _>(Err(37), Stage::Configuration), Err(37));
        assert_eq!(observe::<_, ()>(Ok(91), Stage::VendorLaunch), Ok(91));
    }
    #[cfg(feature = "desktop-qualification")]
    #[test]
    fn first_configuration_boundary_is_not_overwritten() {
        let directory = tempfile::tempdir().unwrap();
        let inner = serde_json::json!({"stage":"configuration", "configurationSubstage":"persist", "configurationDocument":"profile"});
        write_record(directory.path(), &inner);
        write_record(
            directory.path(),
            &serde_json::json!({"stage":"configuration"}),
        );
        let bytes = std::fs::read(directory.path().join("claude-cli-prelaunch.json")).unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
            inner
        );
    }

    #[test]
    fn scope_rejects_missing_opt_in_or_foreign_host() {
        let values = |key: &str| {
            Some(
                match key {
                    "NANH_CLAUDE_PRELAUNCH_DIAGNOSTICS" => "1",
                    "GITHUB_ACTIONS" => "true",
                    "RUNNER_ENVIRONMENT" => "github-hosted",
                    "RUNNER_OS" => "Windows",
                    "NANH_CLAUDE_WINDOWS_PROFILE_POLICY" => "private-env",
                    "NANH_DESKTOP_QUALIFICATION_MODE" => "startup-baseline",
                    _ => "",
                }
                .to_owned(),
            )
        };
        assert!(enabled_values(true, values));
        assert!(!enabled_values(false, values));
        assert!(!enabled_values(true, |key| {
            if key == "NANH_CLAUDE_PRELAUNCH_DIAGNOSTICS" {
                None
            } else {
                values(key)
            }
        }));
        assert!(!enabled_values(
            true,
            |key| if key == "RUNNER_ENVIRONMENT" {
                Some("self-hosted".into())
            } else {
                values(key)
            }
        ));
    }
}
