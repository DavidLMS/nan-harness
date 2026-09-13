//! Opt-in, bounded handoff of closed native-launch failure facts to the
//! desktop checker. This is never enabled on normal user launches.

use nan_harness_private_fs::open_private_new;
use serde::Serialize;
use std::path::Path;

const ENV_PATH: &str = "NAN_NATIVE_LAUNCH_DIAGNOSTIC";

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Failure {
    ArgumentValidationFailed,
    LaunchSetupFailed,
    ProviderRoutingFailed,
    NativeAppSpawnFailed,
    ChildCliFailed,
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
        emit_to(&path, Failure::NativeAppSpawnFailed);
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(value["schemaVersion"], 1);
        assert_eq!(value["failure"], "native-app-spawn-failed");
        assert!(value.get("stderr").is_none());
    }
}
