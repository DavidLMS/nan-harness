use nan_harness_telemetry::diagnostic::{
    Diagnostic, DiagnosticDetails, DiagnosticOperation, DiagnosticReason, IoErrorKind,
    VersionComponent,
};

pub(super) fn io(operation: DiagnosticOperation, error: &std::io::Error) -> Diagnostic {
    Diagnostic::new(
        DiagnosticReason::FilesystemOperationFailed,
        DiagnosticDetails::Io {
            operation,
            error_kind: IoErrorKind::from_std(error.kind()),
        },
    )
}

pub(super) fn process(
    reason: DiagnosticReason,
    operation: DiagnosticOperation,
    exit_code: Option<i32>,
) -> Diagnostic {
    Diagnostic::new(
        reason,
        DiagnosticDetails::Process {
            operation,
            exit_code,
        },
    )
}

pub(super) fn version(
    reason: DiagnosticReason,
    component: VersionComponent,
    detected: Option<String>,
    expected: Option<String>,
) -> Diagnostic {
    Diagnostic::new(
        reason,
        DiagnosticDetails::Version {
            component,
            detected,
            expected,
        },
    )
}

pub(super) fn safe_version(value: &str) -> Option<String> {
    value.split_whitespace().find_map(|token| {
        let candidate = token
            .trim_matches(|character: char| !character.is_ascii_alphanumeric() && character != '.')
            .trim_start_matches('v');
        semver::Version::parse(candidate)
            .ok()
            .map(|version| version.to_string())
    })
}
