//! Opt-in, bounded handoff of closed native-launch failure facts to the
//! desktop checker. This is never enabled on normal user launches.

use nan_harness_private_fs::open_private_new;
use serde::Serialize;
use std::path::Path;

const ENV_PATH: &str = "NAN_NATIVE_LAUNCH_DIAGNOSTIC";

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Failure {
    #[serde(rename = "argument-validation-failed")]
    ArgumentValidation,
    #[serde(rename = "launch-setup-failed")]
    LaunchSetup,
    #[serde(rename = "provider-routing-failed")]
    ProviderRouting,
    #[serde(rename = "native-app-spawn-failed")]
    NativeAppSpawn,
    #[serde(rename = "child-cli-failed")]
    ChildCli,
    NativeArgument,
    NativeCapabilityProbe,
    NativeCapabilityMissing,
    NativeCompatibility,
    NativeVersionProbe,
    NativeVersionUnparseable,
    NativeProcessInspection,
    NativeInstallation,
    NativeAlreadyRunning,
    NativeProfile,
    NativeModelCatalog,
    NativeBridgeHandshake,
    NativeAppExited,
    CredentialUnavailable,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    schema_version: u8,
    failure: Failure,
}

pub(crate) fn emit(failure: Failure) {
    let Ok(path) = std::env::var(ENV_PATH) else {
        return;
    };
    emit_to(Path::new(&path), failure);
}

fn emit_to(path: &Path, failure: Failure) {
    let Ok(mut file) = open_private_new(path) else {
        return;
    };
    let _ = serde_json::to_writer(
        &mut file,
        &Record {
            schema_version: 1,
            failure,
        },
    );
    let _ = file.sync_all();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_child_failure_is_bounded_and_typed() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("diagnostic.json");
        emit_to(&path, Failure::NativeAppSpawn);
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(value["schemaVersion"], 1);
        assert_eq!(value["failure"], "native-app-spawn-failed");
        assert!(value.get("stderr").is_none());
    }

    #[test]
    fn concrete_native_errors_emit_only_closed_causes_and_preserve_existing_files() {
        use crate::commands::chatgpt_desktop::ChatGptDesktopError;
        use crate::commands::hermes_desktop::HermesDesktopError;
        let directory = tempfile::tempdir().unwrap();
        let cases = [
            (
                crate::error::CliError::HermesDesktop(
                    HermesDesktopError::MissingDesktopCapabilities("private path and token".into()),
                ),
                "native-capability-missing",
            ),
            (
                crate::error::CliError::ChatGptDesktop(ChatGptDesktopError::UnparseableVersion),
                "native-version-unparseable",
            ),
        ];
        for (index, (error, expected)) in cases.into_iter().enumerate() {
            let path = directory.path().join(format!("{index}.json"));
            emit_to(&path, crate::native_failure(&error));
            emit_to(&path, Failure::ChildCli);
            let value: serde_json::Value =
                serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
            assert_eq!(
                value,
                serde_json::json!({"schemaVersion":1,"failure":expected})
            );
        }
    }
}
