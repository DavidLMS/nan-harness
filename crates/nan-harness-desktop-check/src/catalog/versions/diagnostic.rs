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
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
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
    };
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
    }
}
