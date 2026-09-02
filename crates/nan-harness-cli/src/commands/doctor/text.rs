use super::discovery;
use super::models::{
    ConfigurationTextReport, ExperimentalTextReport, HarnessDetails, HarnessTextReport,
    HarnessTextStatus, ProviderTextReport, TelemetryTextReport, TextSystemReport,
};
use super::report;
use crate::app::DoctorArgs;
use nan_harness_core::{DesktopHarnessKind, HarnessKind};
use nan_harness_runtime::DiscoveryError;
use std::fmt;

fn append_report_line(report: &mut String, arguments: fmt::Arguments<'_>) {
    report.push_str(&arguments.to_string());
    report.push('\n');
}

macro_rules! append_report_line {
    ($report:expr, $($arguments:tt)*) => {
        {
            append_report_line($report, format_args!($($arguments)*));
        }
    };
}

pub(crate) fn print_harness_report(
    harness: HarnessKind,
    arguments: &DoctorArgs,
) -> Result<(), DiscoveryError> {
    let discovery = discovery::one_harness(
        harness,
        arguments.executable.as_deref(),
        arguments.allow_unsupported,
        arguments.allow_untested,
    )?;
    let report = report::harness_details(discovery);
    print_harness_details(&report);
    Ok(())
}

fn print_harness_details(report: &HarnessDetails) {
    println!("Harness: {}", report.harness);
    println!("Executable: {}", report.executable);
    println!("Version output: {}", report.detected_version);
    println!("Minimum supported: {}", report.minimum_supported_version);
    println!("Last compatible: {}", report.last_compatible_version);
    println!("Compatible at: {}", report.compatible_at);
    println!(
        "Last live verified: {}",
        report
            .last_live_verified_version
            .as_deref()
            .unwrap_or("none")
    );
    println!(
        "Live verified at: {}",
        report.live_verified_at.as_deref().unwrap_or("none")
    );
    println!("Compatibility: {}", report.compatibility);
    for warning in &report.warnings {
        println!("Warning: {warning}");
    }
}

pub(crate) fn print_experimental_report(kind: DesktopHarnessKind) -> i32 {
    let Ok(entry) = discovery::one_experimental(kind) else {
        println!("Experimental Desktop harness: {kind}\nCompatibility registry: unavailable");
        return 1;
    };
    let report = report::experimental_report(entry);
    println!("Experimental Desktop harness: {}", report.id);
    println!("Platform: {}", report.platform);
    println!(
        "Availability: {}",
        if report.available {
            "available"
        } else {
            "unavailable"
        }
    );
    println!("Evidence: {}", evidence_label(report.evidence));
    println!("Transport: {}", report.transport);
    println!("Compatibility data: local (not remotely refreshable)");
    0
}

pub(crate) fn render_system_report(report: TextSystemReport) -> String {
    let mut output = String::new();
    append_report_line!(&mut output, "nan-harness");
    append_report_line!(&mut output, "[OK] Version: {}", env!("CARGO_PKG_VERSION"));
    append_report_line!(
        &mut output,
        "[OK] Platform: {}/{}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    render_provider_health(&mut output, report.provider);

    append_report_line!(&mut output, "\nHarnesses");
    render_harness_health(&mut output, report.harnesses);

    append_report_line!(&mut output, "\nExperimental Desktop harnesses");
    render_experimental_health(&mut output, report.experimental_harnesses);

    append_report_line!(&mut output, "\nManaged harness configurations");
    render_configuration_health(&mut output, report.managed_configurations);

    append_report_line!(&mut output, "\nTelemetry");
    render_telemetry_health(&mut output, report.telemetry);

    append_report_line!(
        &mut output,
        "\nSafe to share: API keys, paths, prompts, model output, and private configuration are excluded."
    );
    output
}

fn render_provider_health(report: &mut String, provider: ProviderTextReport) {
    match provider {
        ProviderTextReport::NotConfigured => {
            append_report_line!(report, "[INFO] API key: not configured");
            append_report_line!(
                report,
                "[SKIP] NaN API and model discovery: API key required"
            );
        }
        ProviderTextReport::Invalid(code) => {
            append_report_line!(report, "[ERROR] Provider configuration: invalid ({code})");
        }
        ProviderTextReport::Models(models) => {
            append_report_line!(report, "[OK] API key: configured");
            append_report_line!(report, "[OK] NaN API: reachable");
            append_report_line!(report, "[OK] Coding models: {} available", models.len());
            if let Some((catalog, generic_present)) = super::models::model_catalog_text(&models) {
                append_report_line!(report, "[INFO] Model catalog: {catalog}");
                if generic_present {
                    append_report_line!(
                        report,
                        "[INFO] * conservative default profile; limits are not provider-authoritative"
                    );
                }
            }
        }
        ProviderTextReport::NoModels => {
            append_report_line!(report, "[OK] API key: configured");
            append_report_line!(report, "[OK] NaN API: reachable");
            append_report_line!(report, "[WARN] Coding models: none available");
        }
        ProviderTextReport::Status(status) => {
            let diagnosis = if matches!(status, 401 | 403) {
                "authentication rejected"
            } else {
                "request rejected"
            };
            append_report_line!(report, "[OK] API key: configured");
            append_report_line!(report, "[ERROR] NaN API: {diagnosis} (HTTP {status})");
        }
        ProviderTextReport::InvalidResponse => {
            append_report_line!(report, "[OK] API key: configured");
            append_report_line!(report, "[OK] NaN API: reachable");
            append_report_line!(report, "[ERROR] Coding models: invalid API response");
        }
        ProviderTextReport::Unavailable(code) => {
            append_report_line!(report, "[OK] API key: configured");
            append_report_line!(report, "[ERROR] NaN API: unavailable ({code})");
        }
        ProviderTextReport::Timeout => {
            append_report_line!(report, "[OK] API key: configured");
            append_report_line!(report, "[ERROR] NaN API: timed out after 10 seconds");
        }
    }
}

fn render_harness_health(report: &mut String, harnesses: Vec<HarnessTextReport>) {
    for harness in harnesses {
        match harness.status {
            HarnessTextStatus::Installed {
                version,
                level,
                label,
            } => {
                append_report_line!(report, "[{level}] {}: {version} ({label})", harness.harness);
            }
            HarnessTextStatus::NotInstalled => {
                append_report_line!(report, "[INFO] {}: not installed", harness.harness);
            }
            HarnessTextStatus::Failed(code) => {
                append_report_line!(report, "[ERROR] {}: check failed ({code})", harness.harness);
            }
        }
    }
}

fn render_experimental_health(report: &mut String, harnesses: Vec<ExperimentalTextReport>) {
    for harness in harnesses {
        match harness {
            ExperimentalTextReport::Available {
                harness,
                platform,
                evidence,
                transport,
                ..
            } => {
                append_report_line!(
                    report,
                    "[INFO] {harness}: {} on {platform} ({transport})",
                    evidence_label(evidence)
                );
            }
            ExperimentalTextReport::Failed { harness, error } => {
                append_report_line!(report, "[WARN] {harness}: {error}");
            }
        }
    }
}

fn render_configuration_health(report: &mut String, configuration: ConfigurationTextReport) {
    match configuration {
        ConfigurationTextReport::NoneConfigured => {
            append_report_line!(report, "[INFO] None configured");
        }
        ConfigurationTextReport::Failed {
            subject,
            status,
            code,
        } => append_report_line!(report, "[ERROR] {subject}: {status} ({code})"),
        ConfigurationTextReport::Configured(integrations) => {
            for integration in integrations {
                let (level, state) = if integration.active {
                    ("OK", "active")
                } else {
                    ("WARN", "managed configuration changed or missing")
                };
                append_report_line!(report, "[{level}] {}: {state}", integration.id);
            }
        }
    }
}

fn render_telemetry_health(report: &mut String, telemetry: TelemetryTextReport) {
    match telemetry {
        TelemetryTextReport::State(enabled) => {
            let state = if enabled { "on" } else { "off" };
            append_report_line!(report, "[INFO] Telemetry: {state}");
        }
        TelemetryTextReport::Failed => {
            append_report_line!(
                report,
                "[ERROR] Telemetry settings: unreadable (NH-TELEMETRY-001)"
            );
        }
    }
}

const fn evidence_label(
    evidence: nan_harness_runtime::desktop_compatibility::DesktopCompatibilityEvidence,
) -> &'static str {
    match evidence {
        nan_harness_runtime::desktop_compatibility::DesktopCompatibilityEvidence::LiveVerified => {
            "live-verified"
        }
        nan_harness_runtime::desktop_compatibility::DesktopCompatibilityEvidence::ContractOnly => {
            "contract-only"
        }
        nan_harness_runtime::desktop_compatibility::DesktopCompatibilityEvidence::Unavailable => {
            "unavailable"
        }
    }
}
