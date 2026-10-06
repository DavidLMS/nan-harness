//! Opt-in, bounded handoff of closed native-launch failure facts to the
//! desktop checker. This is never enabled on normal user launches.

use nan_harness_private_fs::open_private_new;
use serde::Serialize;
#[cfg(target_os = "macos")]
use std::io::Write;
#[cfg(any(target_os = "linux", all(unix, test)))]
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SetupCause {
    Discovery,
    Install,
    Configuration,
    Runtime,
    CurrentDirectory,
    CredentialInvariant,
    Preflight,
    InvalidPlan,
    SerializePlan,
    TelemetrySettings,
    Update,
    Persistence,
    Search,
    Uninstall,
    UsageEvidence,
    Other,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum DiscoveryCause {
    MissingExecutable,
    InvalidExecutable,
    InvalidManifest,
    MissingCompatibilityEntry,
    InvalidVersionCommand,
    VersionCommand,
    VersionCommandFailed,
    VersionProbeTimeout,
    VersionProbeOutputLimit,
    UnsupportedVersion,
    UnparseableVersion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum DiscoveryExit {
    Code(i32),
    Signal(i32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ChildExit {
    Code(i32),
    Signal(i32),
}

#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum SandboxHelperPresence {
    Present,
    Missing,
    Unreadable,
}

#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum SandboxHelperMode {
    SetuidExecutable,
    ExecutableWithoutSetuid,
    NotExecutable,
    Unknown,
}

#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum SandboxHelperOwner {
    Root,
    NonRoot,
    Unknown,
}

#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum SandboxHelperLocation {
    SiblingPresentOrUnreadable,
    SiblingAbsent,
}

#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum NamespacePolicy {
    #[cfg(any(target_os = "linux", test))]
    Restricted,
    #[cfg(any(target_os = "linux", test))]
    Unrestricted,
    Unavailable,
}

#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
struct SandboxFacts {
    helper_presence: SandboxHelperPresence,
    helper_mode: SandboxHelperMode,
    helper_owner: SandboxHelperOwner,
    helper_location: SandboxHelperLocation,
    #[serde(rename = "apparmorUsernsRestriction")]
    apparmor_userns_restriction: NamespacePolicy,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg(any(target_os = "linux", test))]
    sandbox: Option<SandboxFacts>,
    #[serde(skip_serializing_if = "Option::is_none")]
    setup_cause: Option<SetupCause>,
    #[serde(skip_serializing_if = "Option::is_none")]
    discovery_cause: Option<DiscoveryCause>,
    #[serde(skip_serializing_if = "Option::is_none")]
    discovery_exit: Option<DiscoveryExit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    child_exit: Option<ChildExit>,
}

#[derive(Clone, Copy)]
struct RecordFacts {
    app_exit_code: Option<i32>,
    app_exit_signal: Option<i32>,
    startup_hint: Option<StartupHint>,
    setup_cause: Option<SetupCause>,
    discovery_cause: Option<DiscoveryCause>,
    discovery_exit: Option<DiscoveryExit>,
    child_exit: Option<ChildExit>,
    #[cfg(any(target_os = "linux", test))]
    sandbox: Option<SandboxFacts>,
}

pub(crate) fn emit(failure: Failure) {
    let Ok(path) = std::env::var(ENV_PATH) else {
        return;
    };
    emit_to(Path::new(&path), failure);
}

pub(crate) fn emit_with_discovery_cause(
    failure: Failure,
    setup_cause: Option<SetupCause>,
    discovery_cause: Option<DiscoveryCause>,
    discovery_exit: Option<DiscoveryExit>,
) {
    let Ok(path) = std::env::var(ENV_PATH) else {
        return;
    };
    let setup_cause = matches!(failure, Failure::LaunchSetup)
        .then_some(setup_cause)
        .flatten();
    let discovery_cause = (setup_cause == Some(SetupCause::Discovery))
        .then_some(discovery_cause)
        .flatten();
    let discovery_exit = (discovery_cause == Some(DiscoveryCause::VersionCommandFailed))
        .then_some(discovery_exit)
        .flatten();
    #[cfg(any(target_os = "linux", test))]
    emit_record(
        Path::new(&path),
        failure,
        RecordFacts {
            app_exit_code: None,
            app_exit_signal: None,
            startup_hint: None,
            setup_cause,
            discovery_cause,
            discovery_exit,
            child_exit: None,
            sandbox: None,
        },
    );
    #[cfg(not(any(target_os = "linux", test)))]
    emit_record(
        Path::new(&path),
        failure,
        RecordFacts {
            app_exit_code: None,
            app_exit_signal: None,
            startup_hint: None,
            setup_cause,
            discovery_cause,
            discovery_exit,
            child_exit: None,
        },
    );
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
    #[cfg(any(target_os = "linux", test))]
    emit_record(
        path,
        failure,
        RecordFacts {
            app_exit_code: None,
            app_exit_signal: None,
            startup_hint: None,
            setup_cause: None,
            discovery_cause: None,
            discovery_exit: None,
            child_exit: None,
            sandbox: None,
        },
    );
    #[cfg(not(any(target_os = "linux", test)))]
    emit_record(
        path,
        failure,
        RecordFacts {
            app_exit_code: None,
            app_exit_signal: None,
            startup_hint: None,
            setup_cause: None,
            discovery_cause: None,
            discovery_exit: None,
            child_exit: None,
        },
    );
}

pub(crate) fn emit_startup(
    failure: Failure,
    status: std::process::ExitStatus,
    stderr: Option<&Stderr>,
    executable: &Path,
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
    #[cfg(any(target_os = "linux", test))]
    let sandbox = {
        #[cfg(target_os = "linux")]
        {
            Some(sandbox_facts(executable))
        }
        #[cfg(not(target_os = "linux"))]
        {
            None
        }
    };
    #[cfg(not(target_os = "linux"))]
    let _ = executable;
    #[cfg(any(target_os = "linux", test))]
    emit_record(
        Path::new(&path),
        failure,
        RecordFacts {
            app_exit_code: code,
            app_exit_signal: signal,
            startup_hint: hint,
            setup_cause: None,
            discovery_cause: None,
            discovery_exit: None,
            child_exit: None,
            sandbox,
        },
    );
    #[cfg(not(any(target_os = "linux", test)))]
    emit_record(
        Path::new(&path),
        failure,
        RecordFacts {
            app_exit_code: code,
            app_exit_signal: signal,
            startup_hint: hint,
            setup_cause: None,
            discovery_cause: None,
            discovery_exit: None,
            child_exit: None,
        },
    );
}

/// Record the selected Hermes GUI child's terminal status only after its
/// launcher has exited and no desktop process remains. This is intentionally
/// separate from the launcher's wrapper exit fact.
pub(crate) fn emit_hermes_child_exit(status: std::process::ExitStatus) {
    let Ok(path) = std::env::var(ENV_PATH) else {
        return;
    };
    emit_hermes_child_exit_to(Path::new(&path), status);
}

fn emit_hermes_child_exit_to(path: &Path, status: std::process::ExitStatus) {
    if status.success() {
        return;
    }
    let (code, signal) = exit_facts(status);
    let child_exit = code
        .map(ChildExit::Code)
        .or_else(|| signal.map(ChildExit::Signal));
    emit_record(
        path,
        Failure::NativeAppExited,
        RecordFacts {
            app_exit_code: None,
            app_exit_signal: None,
            startup_hint: None,
            setup_cause: None,
            discovery_cause: None,
            discovery_exit: None,
            child_exit,
            #[cfg(any(target_os = "linux", test))]
            sandbox: None,
        },
    );
}

fn emit_record(path: &Path, failure: Failure, facts: RecordFacts) {
    let Ok(mut file) = open_private_new(path) else {
        return;
    };
    let _ = serde_json::to_writer(
        &mut file,
        &Record {
            schema_version: 1,
            failure,
            app_exit_code: facts.app_exit_code,
            app_exit_signal: facts.app_exit_signal,
            startup_hint: facts.startup_hint,
            setup_cause: facts.setup_cause,
            discovery_cause: facts.discovery_cause,
            discovery_exit: facts.discovery_exit,
            child_exit: facts.child_exit,
            #[cfg(any(target_os = "linux", test))]
            sandbox: facts.sandbox,
        },
    );
    let _ = file.sync_all();
}

#[cfg(any(target_os = "linux", all(unix, test)))]
fn sandbox_facts(executable: &Path) -> SandboxFacts {
    let helper = executable
        .parent()
        .map(|parent| parent.join("chrome-sandbox"));
    let Some(helper) = helper else {
        return SandboxFacts {
            helper_presence: SandboxHelperPresence::Missing,
            helper_mode: SandboxHelperMode::Unknown,
            helper_owner: SandboxHelperOwner::Unknown,
            helper_location: SandboxHelperLocation::SiblingAbsent,
            apparmor_userns_restriction: apparmor_userns_restriction(),
        };
    };
    match std::fs::symlink_metadata(&helper) {
        Ok(metadata) if metadata.file_type().is_file() => {
            let mode = metadata.permissions().mode();
            SandboxFacts {
                helper_presence: SandboxHelperPresence::Present,
                helper_mode: if mode & 0o111 == 0 {
                    SandboxHelperMode::NotExecutable
                } else if mode & 0o4000 != 0 {
                    SandboxHelperMode::SetuidExecutable
                } else {
                    SandboxHelperMode::ExecutableWithoutSetuid
                },
                helper_owner: if metadata.uid() == 0 {
                    SandboxHelperOwner::Root
                } else {
                    SandboxHelperOwner::NonRoot
                },
                helper_location: SandboxHelperLocation::SiblingPresentOrUnreadable,
                apparmor_userns_restriction: apparmor_userns_restriction(),
            }
        }
        Err(error)
            if matches!(
                helper_presence_for_error(&error),
                SandboxHelperPresence::Missing
            ) =>
        {
            SandboxFacts {
                helper_presence: SandboxHelperPresence::Missing,
                helper_mode: SandboxHelperMode::Unknown,
                helper_owner: SandboxHelperOwner::Unknown,
                helper_location: SandboxHelperLocation::SiblingAbsent,
                apparmor_userns_restriction: apparmor_userns_restriction(),
            }
        }
        Ok(_) | Err(_) => SandboxFacts {
            helper_presence: SandboxHelperPresence::Unreadable,
            helper_mode: SandboxHelperMode::Unknown,
            helper_owner: SandboxHelperOwner::Unknown,
            helper_location: SandboxHelperLocation::SiblingPresentOrUnreadable,
            apparmor_userns_restriction: apparmor_userns_restriction(),
        },
    }
}

#[cfg(any(target_os = "linux", all(unix, test)))]
fn helper_presence_for_error(error: &std::io::Error) -> SandboxHelperPresence {
    if error.kind() == std::io::ErrorKind::NotFound {
        SandboxHelperPresence::Missing
    } else {
        SandboxHelperPresence::Unreadable
    }
}

#[cfg(any(target_os = "linux", test))]
fn apparmor_userns_restriction() -> NamespacePolicy {
    use std::io::Read as _;
    let Ok(file) = std::fs::File::open("/proc/sys/kernel/apparmor_restrict_unprivileged_userns")
    else {
        return NamespacePolicy::Unavailable;
    };
    let mut bytes = Vec::new();
    if file.take(3).read_to_end(&mut bytes).is_err() || bytes.len() > 2 {
        return NamespacePolicy::Unavailable;
    }
    classify_apparmor_userns_restriction(&bytes)
}

#[cfg(any(target_os = "linux", test))]
fn classify_apparmor_userns_restriction(value: &[u8]) -> NamespacePolicy {
    match value {
        b"1" | b"1\n" => NamespacePolicy::Restricted,
        b"0" | b"0\n" => NamespacePolicy::Unrestricted,
        _ => NamespacePolicy::Unavailable,
    }
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
            RecordFacts {
                app_exit_code: exit_facts(status).0,
                app_exit_signal: exit_facts(status).1,
                startup_hint: Some(StartupHint::Unknown),
                setup_cause: None,
                discovery_cause: None,
                discovery_exit: None,
                child_exit: None,
                sandbox: None,
            },
        );
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(value["appExitCode"], 17);
        assert!(value.get("appExitSignal").is_none());
        assert_eq!(value["startupHint"], "unknown");
        assert!(value.get("stderr").is_none());
        assert!(value.get("url").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn hermes_child_exit_record_preserves_code_or_signal() {
        let directory = tempfile::tempdir().unwrap();
        let code_path = directory.path().join("code.json");
        let code_status = std::process::Command::new("sh")
            .args(["-c", "exit 17"])
            .status()
            .unwrap();
        let signal_path = directory.path().join("signal.json");
        let signal_status = std::process::Command::new("sh")
            .args(["-c", "kill -TERM $$"])
            .status()
            .unwrap();
        emit_hermes_child_exit_to(&code_path, code_status);
        emit_hermes_child_exit_to(&signal_path, signal_status);
        let code: serde_json::Value =
            serde_json::from_slice(&std::fs::read(code_path).unwrap()).unwrap();
        let signal: serde_json::Value =
            serde_json::from_slice(&std::fs::read(signal_path).unwrap()).unwrap();
        assert_eq!(code["childExit"], serde_json::json!({"code": 17}));
        assert_eq!(signal["childExit"], serde_json::json!({"signal": 15}));
        let success_path = directory.path().join("success.json");
        std::fs::write(&success_path, b"existing").unwrap();
        let success_status = std::process::Command::new("sh")
            .args(["-c", "exit 0"])
            .status()
            .unwrap();
        emit_hermes_child_exit_to(&success_path, success_status);
        assert_eq!(std::fs::read(&success_path).unwrap(), b"existing");
        let success_absent_path = directory.path().join("success-absent.json");
        emit_hermes_child_exit_to(&success_absent_path, success_status);
        assert!(!success_absent_path.exists());
        let existing_code_path = directory.path().join("existing-code.json");
        let existing_signal_path = directory.path().join("existing-signal.json");
        std::fs::write(&existing_code_path, b"existing-code").unwrap();
        std::fs::write(&existing_signal_path, b"existing-signal").unwrap();
        emit_hermes_child_exit_to(&existing_code_path, code_status);
        emit_hermes_child_exit_to(&existing_signal_path, signal_status);
        assert_eq!(
            std::fs::read(&existing_code_path).unwrap(),
            b"existing-code"
        );
        assert_eq!(
            std::fs::read(&existing_signal_path).unwrap(),
            b"existing-signal"
        );
    }

    #[cfg(unix)]
    #[test]
    fn sandbox_facts_are_closed_for_a_private_non_setuid_helper() {
        use std::os::unix::fs::PermissionsExt as _;
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("ChatGPT");
        let helper = directory.path().join("chrome-sandbox");
        std::fs::write(&executable, b"app").unwrap();
        let missing = serde_json::to_value(sandbox_facts(&executable)).unwrap();
        assert_eq!(missing["helperPresence"], "missing");
        assert_eq!(missing["helperLocation"], "sibling-absent");
        std::fs::write(&helper, b"helper").unwrap();
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
        let value = serde_json::to_value(sandbox_facts(&executable)).unwrap();
        assert_eq!(value["helperPresence"], "present");
        assert_eq!(value["helperMode"], "executable-without-setuid");
        assert!(matches!(
            value["helperOwner"].as_str(),
            Some("root" | "non-root")
        ));
        assert_eq!(value["helperLocation"], "sibling-present-or-unreadable");
        assert!(value.get("path").is_none());
        assert!(value.get("uid").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn sandbox_helper_mappings_cover_nonexecutable_symlink_and_errors() {
        use std::os::unix::fs::PermissionsExt as _;
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("ChatGPT");
        let helper = directory.path().join("chrome-sandbox");
        std::fs::write(&executable, b"app").unwrap();
        std::fs::write(&helper, b"helper").unwrap();
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o600)).unwrap();
        let value = serde_json::to_value(sandbox_facts(&executable)).unwrap();
        assert_eq!(value["helperPresence"], "present");
        assert_eq!(value["helperMode"], "not-executable");
        std::fs::remove_file(&helper).unwrap();
        std::os::unix::fs::symlink(&executable, &helper).unwrap();
        let value = serde_json::to_value(sandbox_facts(&executable)).unwrap();
        assert_eq!(value["helperPresence"], "unreadable");
        assert_eq!(value["helperMode"], "unknown");
        assert_eq!(
            helper_presence_for_error(&std::io::Error::from(std::io::ErrorKind::PermissionDenied,)),
            SandboxHelperPresence::Unreadable
        );
        assert_eq!(
            helper_presence_for_error(&std::io::Error::from(std::io::ErrorKind::NotFound,)),
            SandboxHelperPresence::Missing
        );
    }

    #[test]
    fn apparmor_userns_restriction_mapping_is_bounded_and_closed() {
        assert!(matches!(
            classify_apparmor_userns_restriction(b"0"),
            NamespacePolicy::Unrestricted
        ));
        assert!(matches!(
            classify_apparmor_userns_restriction(b"1\n"),
            NamespacePolicy::Restricted
        ));
        for value in [b"".as_slice(), b"2", b"10", b"1\n0"] {
            assert!(matches!(
                classify_apparmor_userns_restriction(value),
                NamespacePolicy::Unavailable
            ));
        }
    }

    #[test]
    fn cli_sandbox_facts_match_shared_checker_fixture() {
        let value = serde_json::to_value(SandboxFacts {
            helper_presence: SandboxHelperPresence::Present,
            helper_mode: SandboxHelperMode::SetuidExecutable,
            helper_owner: SandboxHelperOwner::Root,
            helper_location: SandboxHelperLocation::SiblingPresentOrUnreadable,
            apparmor_userns_restriction: NamespacePolicy::Restricted,
        })
        .unwrap();
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../canary/tests/fixtures/chatgpt-sandbox-diagnostic.json"
        ))
        .unwrap();
        assert_eq!(value, fixture);
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

    #[test]
    fn setup_cause_fixture_matches_producer_serialization() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("diagnostic.json");
        emit_record(
            &path,
            Failure::LaunchSetup,
            RecordFacts {
                app_exit_code: None,
                app_exit_signal: None,
                startup_hint: None,
                setup_cause: Some(SetupCause::Runtime),
                discovery_cause: None,
                discovery_exit: None,
                child_exit: None,
                sandbox: None,
            },
        );
        let actual: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let expected: serde_json::Value = serde_json::from_str(include_str!(
            "../../../canary/tests/fixtures/native-setup-cause.json"
        ))
        .unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn discovery_cause_fixture_matches_producer_serialization() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("diagnostic.json");
        emit_record(
            &path,
            Failure::LaunchSetup,
            RecordFacts {
                app_exit_code: None,
                app_exit_signal: None,
                startup_hint: None,
                setup_cause: Some(SetupCause::Discovery),
                discovery_cause: Some(DiscoveryCause::VersionCommandFailed),
                discovery_exit: Some(DiscoveryExit::Code(1)),
                child_exit: None,
                sandbox: None,
            },
        );
        let actual: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let expected: serde_json::Value = serde_json::from_str(include_str!(
            "../../../canary/tests/fixtures/native-discovery-cause.json"
        ))
        .unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn setup_cause_is_a_fixed_private_fact() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("diagnostic.json");
        emit_record(
            &path,
            Failure::LaunchSetup,
            RecordFacts {
                app_exit_code: None,
                app_exit_signal: None,
                startup_hint: None,
                setup_cause: Some(SetupCause::CurrentDirectory),
                discovery_cause: None,
                discovery_exit: None,
                child_exit: None,
                sandbox: None,
            },
        );
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(value["failure"], "launch-setup-failed");
        assert_eq!(value["setupCause"], "current-directory");
        assert!(value.get("path").is_none());
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
