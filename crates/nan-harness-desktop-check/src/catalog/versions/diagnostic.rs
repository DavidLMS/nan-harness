use nan_harness_core::DesktopHarnessKind;
use serde::{Deserialize, Serialize};
use std::io;

const PREFIX: &str = "DESKTOP_VERSION_DIAGNOSTIC:";
const MAX_BYTES: usize = 2048;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Source {
    PackageMetadata,
    AsarMetadata,
    AppVersionCommand,
    RuntimeMetadata,
    RuntimeVersionCommand,
    WindowsPackageEnumeration,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Failure {
    Spawn,
    Wait,
    Timeout,
    NonzeroExit,
    Pipe,
    Read,
    Oversize,
    Encoding,
    MetadataUnreadable,
    InvalidMetadata,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum RuntimeReadAccess {
    Readable,
    AccessDenied,
    OtherError,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum RuntimeFile {
    RegularFile,
    Directory,
    SymlinkOrReparse,
    Other,
    Missing,
    QueryUnknown,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum RuntimeImageHeader {
    MachineMatchesHost,
    MachineDiffers,
    UnsupportedMachine,
    InvalidHeader,
    QueryUnknown,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct CommandFailure {
    pub(super) failure: Failure,
    pub(super) exit_code: Option<i32>,
    pub(super) os_error: Option<i32>,
}

impl CommandFailure {
    pub(super) const fn new(failure: Failure) -> Self {
        Self {
            failure,
            exit_code: None,
            os_error: None,
        }
    }

    pub(super) fn io(failure: Failure, error: &io::Error) -> Self {
        Self {
            os_error: error.raw_os_error(),
            ..Self::new(failure)
        }
    }
}

pub(super) fn emit_command(app: DesktopHarnessKind, source: Source, error: &CommandFailure) {
    emit_command_with_runtime_read_access(app, source, error, None);
}

pub(super) fn emit_command_with_runtime_read_access(
    app: DesktopHarnessKind,
    source: Source,
    error: &CommandFailure,
    runtime_read_access: Option<RuntimeReadAccess>,
) {
    emit_command_with_runtime_observations(app, source, error, runtime_read_access, None, None);
}

pub(super) fn emit_command_with_runtime_observations(
    app: DesktopHarnessKind,
    source: Source,
    error: &CommandFailure,
    runtime_read_access: Option<RuntimeReadAccess>,
    runtime_file: Option<RuntimeFile>,
    runtime_image_header: Option<RuntimeImageHeader>,
) {
    emit_event(Event {
        schema_version: 1,
        app,
        source,
        failure: error.failure,
        exit_code: error.exit_code,
        os_error: error.os_error,
        runtime_read_access,
        runtime_file,
        runtime_image_header,
    });
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Event {
    pub(super) schema_version: u8,
    pub(super) app: DesktopHarnessKind,
    pub(super) source: Source,
    pub(super) failure: Failure,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) os_error: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) runtime_read_access: Option<RuntimeReadAccess>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) runtime_file: Option<RuntimeFile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) runtime_image_header: Option<RuntimeImageHeader>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EventFields {
    schema_version: u8,
    app: DesktopHarnessKind,
    source: Source,
    failure: Failure,
    #[serde(default)]
    exit_code: Option<i32>,
    #[serde(default)]
    os_error: Option<i32>,
    #[serde(default)]
    runtime_read_access: Option<RuntimeReadAccess>,
    #[serde(default)]
    runtime_file: Option<RuntimeFile>,
    #[serde(default)]
    runtime_image_header: Option<RuntimeImageHeader>,
}

impl<'de> Deserialize<'de> for Event {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let fields = EventFields::deserialize(deserializer)?;
        if (fields.runtime_read_access.is_some()
            || fields.runtime_file.is_some()
            || fields.runtime_image_header.is_some())
            && !(fields.app == DesktopHarnessKind::ChatGpt
                && fields.source == Source::RuntimeVersionCommand
                && fields.failure == Failure::Spawn
                && fields.os_error == Some(5))
        {
            return Err(serde::de::Error::custom(
                "runtime observations are only valid for ChatGPT runtime spawn error 5",
            ));
        }
        Ok(Self {
            schema_version: fields.schema_version,
            app: fields.app,
            source: fields.source,
            failure: fields.failure,
            exit_code: fields.exit_code,
            os_error: fields.os_error,
            runtime_read_access: fields.runtime_read_access,
            runtime_file: fields.runtime_file,
            runtime_image_header: fields.runtime_image_header,
        })
    }
}

pub(super) fn emit(
    app: DesktopHarnessKind,
    source: Source,
    failure: Failure,
    exit_code: Option<i32>,
    error: Option<&io::Error>,
) {
    let event = Event {
        schema_version: 1,
        app,
        source,
        failure,
        exit_code,
        os_error: error.and_then(io::Error::raw_os_error),
        runtime_read_access: None,
        runtime_file: None,
        runtime_image_header: None,
    };
    emit_event(event);
}

fn emit_event(event: Event) {
    let Ok(bytes) = serde_json::to_vec(&event) else {
        return;
    };
    if bytes.len() <= MAX_BYTES
        && let Ok(line) = String::from_utf8(bytes)
    {
        eprintln!("{PREFIX}{line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_diagnostic_schema_is_closed_and_bounded() {
        let event = Event {
            schema_version: 1,
            app: DesktopHarnessKind::ChatGpt,
            source: Source::RuntimeVersionCommand,
            failure: Failure::NonzeroExit,
            exit_code: Some(7),
            os_error: None,
            runtime_read_access: None,
            runtime_file: None,
            runtime_image_header: None,
        };
        let value = serde_json::to_value(event).unwrap();
        assert_eq!(value["schemaVersion"], 1);
        assert_eq!(value["app"], "chatgpt-desktop");
        assert_eq!(value["source"], "runtime-version-command");
        assert_eq!(value["failure"], "nonzero-exit");
        assert!(serde_json::from_value::<Event>(value.clone()).is_ok());
        let mut extra = value.as_object().unwrap().clone();
        extra.insert("path".into(), "private".into());
        assert!(serde_json::from_value::<Event>(extra.into()).is_err());
        let access = Event {
            failure: Failure::Spawn,
            exit_code: None,
            os_error: Some(5),
            runtime_read_access: Some(RuntimeReadAccess::AccessDenied),
            ..event
        };
        assert_eq!(
            serde_json::to_value(access).unwrap()["runtimeReadAccess"],
            "access-denied"
        );
        assert!(
            serde_json::from_value::<Event>(serde_json::json!({
                "schemaVersion": 1,
                "app": "chatgpt-desktop",
                "source": "runtime-version-command",
                "failure": "nonzero-exit",
                "runtimeReadAccess": "readable"
            }))
            .is_err()
        );
        let observations = serde_json::json!({
            "schemaVersion": 1,
            "app": "chatgpt-desktop",
            "source": "runtime-version-command",
            "failure": "spawn",
            "osError": 5,
            "runtimeFile": "regular-file",
            "runtimeImageHeader": "query-unknown"
        });
        assert!(serde_json::from_value::<Event>(observations).is_ok());
        let mut wrong_context = serde_json::json!({
            "schemaVersion": 1,
            "app": "chatgpt-desktop",
            "source": "runtime-version-command",
            "failure": "spawn",
            "runtimeFile": "missing"
        });
        assert!(serde_json::from_value::<Event>(wrong_context.clone()).is_err());
        wrong_context["osError"] = serde_json::json!(5);
        wrong_context["runtimeImageHeader"] = serde_json::json!("invalid-header");
        assert!(serde_json::from_value::<Event>(wrong_context).is_ok());
    }
}
