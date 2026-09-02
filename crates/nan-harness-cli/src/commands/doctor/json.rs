use super::discovery;
use super::models::{DOCTOR_SCHEMA_VERSION, SystemDoctorReport};
use super::report;
use crate::app::DoctorArgs;
use nan_harness_core::{DesktopHarnessKind, HarnessKind};

pub(crate) fn print_system_report(report: &SystemDoctorReport) -> i32 {
    let exit_code = i32::from(report.has_errors());
    let Ok(serialized) = serde_json::to_string_pretty(&report) else {
        eprintln!("could not serialize the typed doctor report");
        return 1;
    };
    println!("{serialized}");
    exit_code
}

pub(crate) fn print_harness_report(harness: HarnessKind, arguments: &DoctorArgs) -> i32 {
    let discovery = discovery::one_harness(
        harness,
        arguments.executable.as_deref(),
        arguments.allow_unsupported,
        arguments.allow_untested,
    );
    let report = report::harness_json_report(harness, discovery);
    let Ok(serialized) = serde_json::to_string_pretty(&report) else {
        eprintln!("could not serialize the typed harness doctor report");
        return 1;
    };
    println!("{serialized}");
    i32::from(report.level == super::models::DiagnosticLevel::Error)
}

pub(crate) fn print_experimental_report(kind: DesktopHarnessKind) -> i32 {
    let Ok(entry) = discovery::one_experimental(kind) else {
        println!(
            "{{\"schemaVersion\":{DOCTOR_SCHEMA_VERSION},\"harness\":\"{kind}\",\"level\":\"error\",\"safeToShare\":true}}"
        );
        return 1;
    };
    let report = report::experimental_json_report(entry);
    let Ok(serialized) = serde_json::to_string_pretty(&report) else {
        return 1;
    };
    println!("{serialized}");
    0
}
