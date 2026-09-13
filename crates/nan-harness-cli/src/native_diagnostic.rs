//! Opt-in, bounded handoff of closed native-launch failure facts to the
//! desktop checker. This is never enabled on normal user launches.

use nan_harness_private_fs::open_private_new;
use serde::Serialize;
#[cfg(target_os = "macos")]
use std::io::Write;
use std::path::Path;

const ENV_PATH: &str = "NAN_NATIVE_LAUNCH_DIAGNOSTIC";
#[cfg(target_os = "macos")]
pub(crate) const PROCESS_OBSERVATION_ENV_PATH: &str = "NAN_NATIVE_PROCESS_OBSERVATION";

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum NativeProcessObservation {
    MatchingProcessPresent,
    MatchingProcessAbsent,
    QueryFailed,
}

#[cfg(target_os = "macos")]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProcessObservationRecord {
    schema_version: u8,
    observation: NativeProcessObservation,
    ever_observed_present: bool,
}

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

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum StartupHint {
    NoUsableSandbox,
    MissingSharedLibrary,
    DisplayUnavailable,
    Unknown,
    OutputUnavailable,
}

pub(crate) struct Stderr {
    pub(crate) bytes: zeroize::Zeroizing<Vec<u8>>,
    pub(crate) overflow: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    schema_version: u8,
    failure: Failure,
    #[serde(skip_serializing_if = "Option::is_none")]
    app_exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    app_exit_signal: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    startup_hint: Option<StartupHint>,
}

pub(crate) fn emit(failure: Failure) {
    let Ok(path) = std::env::var(ENV_PATH) else {
        return;
    };
    emit_to(Path::new(&path), failure);
}

pub(crate) fn enabled(debug: bool) -> bool {
    diagnostic_enabled(debug, std::env::var_os(ENV_PATH).is_some())
}

#[cfg(target_os = "macos")]
pub(crate) fn record_process_observation(
    observation: NativeProcessObservation,
    ever_observed_present: bool,
) {
    let Ok(path) = std::env::var(PROCESS_OBSERVATION_ENV_PATH) else {
        return;
    };
    record_process_observation_at(Some(Path::new(&path)), observation, ever_observed_present);
}

#[cfg(target_os = "macos")]
pub(crate) fn record_process_observation_at(
    path: Option<&Path>,
    observation: NativeProcessObservation,
    ever_observed_present: bool,
) {
    let Some(path) = path else {
        return;
    };
    write_process_observation(path, observation, ever_observed_present);
}

#[cfg(target_os = "macos")]
pub(crate) fn map_process_observation(result: Result<bool, ()>) -> NativeProcessObservation {
    match result {
        Ok(true) => NativeProcessObservation::MatchingProcessPresent,
        Ok(false) => NativeProcessObservation::MatchingProcessAbsent,
        Err(()) => NativeProcessObservation::QueryFailed,
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn accumulate_process_observation(
    ever_observed_present: bool,
    result: Result<bool, ()>,
) -> (NativeProcessObservation, bool) {
    let observation = map_process_observation(result);
    let ever_observed_present =
        ever_observed_present || observation == NativeProcessObservation::MatchingProcessPresent;
    (observation, ever_observed_present)
}

#[cfg(target_os = "macos")]
pub(crate) fn write_process_observation(
    path: &Path,
    observation: NativeProcessObservation,
    ever_observed_present: bool,
) {
    let record = ProcessObservationRecord {
        schema_version: 1,
        observation,
        ever_observed_present,
    };
    let Some(parent) = path.parent() else {
        return;
    };
    let Ok(mut temporary) = tempfile::Builder::new()
        .prefix(".nan-observation-")
        .make_in(parent, open_private_new)
    else {
        return;
    };
    if serde_json::to_writer(&mut temporary, &record).is_err()
        || temporary.flush().is_err()
        || temporary.as_file().sync_all().is_err()
    {
        return;
    }
    let _ = temporary.persist(path);
}

pub(crate) const fn diagnostic_enabled(debug: bool, configured: bool) -> bool {
    configured && !debug
}

fn emit_to(path: &Path, failure: Failure) {
    emit_record(path, failure, None, None, None);
}

pub(crate) fn emit_startup(
    failure: Failure,
    status: std::process::ExitStatus,
    stderr: Option<&Stderr>,
) {
    let Ok(path) = std::env::var(ENV_PATH) else {
        return;
    };
    let (code, signal) = exit_facts(status);
    let hint = Some(match stderr {
        Some(capture) if capture.overflow => StartupHint::Unknown,
        Some(capture) => classify_startup_hint(&capture.bytes),
        None => StartupHint::OutputUnavailable,
    });
    emit_record(Path::new(&path), failure, code, signal, hint);
}

fn emit_record(
    path: &Path,
    failure: Failure,
    app_exit_code: Option<i32>,
    app_exit_signal: Option<i32>,
    startup_hint: Option<StartupHint>,
) {
    let Ok(mut file) = open_private_new(path) else {
        return;
    };
    let _ = serde_json::to_writer(
        &mut file,
        &Record {
            schema_version: 1,
            failure,
            app_exit_code,
            app_exit_signal,
            startup_hint,
        },
    );
    let _ = file.sync_all();
}

fn classify_startup_hint(stderr: &[u8]) -> StartupHint {
    if std::str::from_utf8(stderr).is_err() {
        return StartupHint::Unknown;
    }
    if stderr
        .windows(b"No usable sandbox!".len())
        .any(|w| w == b"No usable sandbox!")
    {
        StartupHint::NoUsableSandbox
    } else if stderr
        .windows(b"error while loading shared libraries:".len())
        .any(|w| w == b"error while loading shared libraries:")
    {
        StartupHint::MissingSharedLibrary
    } else if stderr
        .windows(b"Missing X server or $DISPLAY".len())
        .any(|w| w == b"Missing X server or $DISPLAY")
    {
        StartupHint::DisplayUnavailable
    } else if stderr.is_empty() {
        StartupHint::OutputUnavailable
    } else {
        StartupHint::Unknown
    }
}

fn exit_facts(status: std::process::ExitStatus) -> (Option<i32>, Option<i32>) {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        if let Some(signal) = status.signal() {
            return (None, Some(signal));
        }
    }
    (status.code(), None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_gate_requires_opt_in_and_non_debug_mode() {
        assert!(!diagnostic_enabled(false, false));
        assert!(!diagnostic_enabled(true, true));
        assert!(diagnostic_enabled(false, true));
    }

    #[test]
    fn startup_signatures_are_closed_and_never_returned() {
        assert!(matches!(
            classify_startup_hint(b"No usable sandbox!"),
            StartupHint::NoUsableSandbox
        ));
        assert!(matches!(
            classify_startup_hint(b"error while loading shared libraries: libx"),
            StartupHint::MissingSharedLibrary
        ));
        assert!(matches!(
            classify_startup_hint(b"Missing X server or $DISPLAY"),
            StartupHint::DisplayUnavailable
        ));
        assert!(matches!(
            classify_startup_hint(&[0xff]),
            StartupHint::Unknown
        ));
        assert!(matches!(
            classify_startup_hint(b""),
            StartupHint::OutputUnavailable
        ));
    }

    #[cfg(unix)]
    #[test]
    fn startup_record_contains_only_closed_facts() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("startup.json");
        let status = std::process::Command::new("sh")
            .args(["-c", "exit 17"])
            .status()
            .unwrap();
        emit_record(
            &path,
            Failure::NativeAppExited,
            exit_facts(status).0,
            exit_facts(status).1,
            Some(StartupHint::Unknown),
        );
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(value["appExitCode"], 17);
        assert!(value.get("appExitSignal").is_none());
        assert_eq!(value["startupHint"], "unknown");
        assert!(value.get("stderr").is_none());
        assert!(value.get("url").is_none());
    }

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

    #[cfg(target_os = "macos")]
    #[test]
    fn process_observation_mapping_and_atomic_records_are_closed() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("native-process-observation.json");
        assert_eq!(
            map_process_observation(Ok(true)),
            NativeProcessObservation::MatchingProcessPresent
        );
        assert_eq!(
            map_process_observation(Ok(false)),
            NativeProcessObservation::MatchingProcessAbsent
        );
        assert_eq!(
            map_process_observation(Err(())),
            NativeProcessObservation::QueryFailed
        );
        assert_eq!(
            accumulate_process_observation(false, Ok(true)),
            (NativeProcessObservation::MatchingProcessPresent, true)
        );
        assert_eq!(
            accumulate_process_observation(true, Ok(false)),
            (NativeProcessObservation::MatchingProcessAbsent, true)
        );
        assert_eq!(
            accumulate_process_observation(true, Err(())),
            (NativeProcessObservation::QueryFailed, true)
        );
        write_process_observation(
            &path,
            NativeProcessObservation::MatchingProcessPresent,
            true,
        );
        write_process_observation(&path, NativeProcessObservation::MatchingProcessAbsent, true);
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(value["observation"], "matching-process-absent");
        assert_eq!(value["everObservedPresent"], true);
        assert!(value.get("pid").is_none());
        assert!(value.get("name").is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn process_observation_is_disabled_without_opt_in_environment() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("native-process-observation.json");
        record_process_observation_at(None, NativeProcessObservation::MatchingProcessAbsent, false);
        assert!(!path.exists());
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
