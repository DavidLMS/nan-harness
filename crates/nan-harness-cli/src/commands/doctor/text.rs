use super::discovery;
use super::models::{
    ConfigurationTextReport, ExperimentalTextReport, HarnessDetails, HarnessTextReport,
    HarnessTextStatus, ProviderTextReport, TelemetryTextReport, TextSystemReport,
};
use super::report;
use crate::app::DoctorArgs;
use nan_harness_core::{DesktopHarnessKind, HarnessKind};
use nan_harness_i18n::{locale, messages};
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
    println!("{}", messages::text_harness(locale(), &(report.harness)));
    println!(
        "{}",
        messages::text_executable(locale(), &(report.executable))
    );
    println!(
        "{}",
        messages::text_version_output(locale(), &(report.detected_version))
    );
    println!(
        "{}",
        messages::text_minimum_supported(locale(), &(report.minimum_supported_version))
    );
    println!(
        "{}",
        messages::text_last_compatible(locale(), &(report.last_compatible_version))
    );
    println!(
        "{}",
        messages::text_compatible_at(locale(), &(report.compatible_at))
    );
    println!(
        "{}",
        messages::text_last_live_verified(
            locale(),
            &(report
                .last_live_verified_version
                .as_deref()
                .unwrap_or(messages::terminal_none_text(locale())))
        )
    );
    println!(
        "{}",
        messages::text_live_verified_at(
            locale(),
            &(report
                .live_verified_at
                .as_deref()
                .unwrap_or(messages::terminal_none_text(locale())))
        )
    );
    println!(
        "{}",
        messages::text_compatibility(locale(), &(report.compatibility))
    );
    for warning in &report.warnings {
        println!("{}", messages::text_warning(locale(), &(warning)));
    }
}

pub(crate) fn print_experimental_report(kind: DesktopHarnessKind) -> i32 {
    let Ok(entry) = discovery::one_experimental(kind) else {
        println!(
            "{}",
            messages::text_experimental_desktop_harness_compatibility_registry_unavailable(
                locale(),
                &(kind)
            )
        );
        return 1;
    };
    let report = report::experimental_report(entry);
    println!(
        "{}",
        messages::text_experimental_desktop_harness(locale(), &(report.id))
    );
    println!("{}", messages::text_platform(locale(), &(report.platform)));
    println!(
        "{}",
        messages::text_availability(
            locale(),
            &(if report.available {
                messages::terminal_available_text(locale())
            } else {
                messages::terminal_unavailable_text(locale())
            })
        )
    );
    println!(
        "{}",
        messages::text_evidence(locale(), &(evidence_label(report.evidence)))
    );
    println!(
        "{}",
        messages::text_transport(locale(), &(report.transport))
    );
    print_optional_version(
        messages::terminal_minimum_app_version_text(locale()),
        report.minimum_supported_version.as_deref(),
    );
    print_optional_version(
        messages::terminal_last_compatible_app_version_text(locale()),
        report.last_compatible_version.as_deref(),
    );
    print_optional_version(
        messages::terminal_minimum_runtime_version_text(locale()),
        report.minimum_runtime_version.as_deref(),
    );
    print_optional_version(
        messages::terminal_last_compatible_runtime_version_text(locale()),
        report.last_compatible_runtime_version.as_deref(),
    );
    println!(
        "{}",
        messages::text_evidence_date(locale(), &(report.compatible_at))
    );
    println!(
        "{}",
        messages::text_compatibility_data(
            locale(),
            &(evidence_source_label(report.evidence_source))
        )
    );
    0
}

fn print_optional_version(label: &str, version: Option<&str>) {
    println!(
        "{label}: {}",
        version.unwrap_or(messages::terminal_none_text(locale()))
    );
}

pub(crate) fn render_system_report(report: TextSystemReport) -> String {
    let mut output = String::new();
    append_report_line!(&mut output, "nan-harness");
    append_report_line!(
        &mut output,
        "{}",
        messages::terminal_ok_version(locale(), &(env!("CARGO_PKG_VERSION")))
    );
    append_report_line!(
        &mut output,
        "{}",
        messages::terminal_ok_platform(
            locale(),
            &(std::env::consts::OS),
            &(std::env::consts::ARCH)
        )
    );
    render_provider_health(&mut output, report.provider);

    append_report_line!(&mut output, "{}", messages::terminal_harnesses(locale()));
    render_harness_health(&mut output, report.harnesses);

    append_report_line!(
        &mut output,
        "{}",
        messages::terminal_experimental_desktop_harnesses(locale())
    );
    render_experimental_health(&mut output, report.experimental_harnesses);

    append_report_line!(
        &mut output,
        "{}",
        messages::terminal_managed_harness_configurations(locale())
    );
    render_configuration_health(&mut output, report.managed_configurations);

    append_report_line!(&mut output, "{}", messages::terminal_telemetry(locale()));
    render_telemetry_health(&mut output, report.telemetry);

    append_report_line!(
        &mut output,
        "{}", messages::terminal_safe_to_share_api_keys_paths_prompts_model_output_and_private_configuration_are_exclu(locale()));
    output
}

fn render_provider_health(report: &mut String, provider: ProviderTextReport) {
    if !matches!(
        provider,
        ProviderTextReport::SkippedOffline
            | ProviderTextReport::NotConfigured
            | ProviderTextReport::Invalid(_)
    ) {
        append_report_line!(
            report,
            "{}",
            messages::terminal_ok_api_key_configured(locale())
        );
    }
    if matches!(
        provider,
        ProviderTextReport::Models(_)
            | ProviderTextReport::NoModels
            | ProviderTextReport::InvalidResponse
    ) {
        append_report_line!(
            report,
            "{}",
            messages::terminal_ok_nan_api_reachable(locale())
        );
    }
    match provider {
        ProviderTextReport::SkippedOffline => {
            append_report_line!(report, "{}", messages::doctor_key_unchecked(locale()));
            append_report_line!(report, "{}", messages::doctor_provider_offline(locale()));
        }
        ProviderTextReport::NotConfigured => {
            append_report_line!(
                report,
                "{}",
                messages::terminal_info_api_key_not_configured(locale())
            );
            append_report_line!(report, "{}", messages::doctor_provider_no_key(locale()));
        }
        ProviderTextReport::Invalid(code) => {
            append_report_line!(
                report,
                "{}",
                messages::terminal_error_provider_configuration_invalid(locale(), &(code))
            );
        }
        ProviderTextReport::Models(models) => {
            append_report_line!(
                report,
                "{}",
                messages::terminal_ok_coding_models_available(locale(), &(models.len()))
            );
            render_provider_models(report, &models);
        }
        ProviderTextReport::NoModels => {
            append_report_line!(
                report,
                "{}",
                messages::terminal_warn_coding_models_none_available(locale())
            );
        }
        ProviderTextReport::Status(status) => {
            let diagnosis = if matches!(status, 401 | 403) {
                messages::terminal_authentication_rejected_text(locale())
            } else {
                messages::terminal_request_rejected_text(locale())
            };
            append_report_line!(
                report,
                "{}",
                messages::terminal_error_nan_api_http(locale(), &(diagnosis), &(status))
            );
        }
        ProviderTextReport::InvalidResponse => {
            append_report_line!(report, "{}", messages::doctor_catalog_invalid(locale()));
        }
        ProviderTextReport::Unavailable(code) => {
            append_report_line!(
                report,
                "{}",
                messages::terminal_error_nan_api_unavailable(locale(), &(code))
            );
        }
        ProviderTextReport::Timeout => {
            append_report_line!(report, "{}", messages::doctor_provider_timeout(locale()));
        }
    }
}

fn render_provider_models(report: &mut String, models: &[nan_harness_core::CodingModelProfile]) {
    if let Some((catalog, generic_present)) = super::models::model_catalog_text(models) {
        append_report_line!(
            report,
            "{}",
            messages::terminal_info_model_catalog(locale(), &(catalog))
        );
        if generic_present {
            append_report_line!(
                report,
                "{}",
                messages::doctor_profile_limits_advisory(locale())
            );
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
                append_report_line!(
                    report,
                    "{}",
                    messages::terminal_info_not_installed(locale(), &(harness.harness))
                );
            }
            HarnessTextStatus::Failed(code) => {
                append_report_line!(
                    report,
                    "{}",
                    messages::terminal_error_check_failed(locale(), &(code), &(harness.harness))
                );
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
                compatible_at,
                evidence_source,
            } => {
                append_report_line!(
                    report,
                    "{}",
                    messages::terminal_info_on_of(
                        locale(),
                        &(compatible_at),
                        &(harness),
                        &(platform),
                        &(transport),
                        &(evidence_label(evidence)),
                        &(evidence_source_label(evidence_source))
                    )
                );
            }
            ExperimentalTextReport::Failed { harness, error } => {
                append_report_line!(
                    report,
                    "{}",
                    messages::doctor_harness_warning(locale(), &error, &harness)
                );
            }
        }
    }
}

fn render_configuration_health(report: &mut String, configuration: ConfigurationTextReport) {
    match configuration {
        ConfigurationTextReport::NoneConfigured => {
            append_report_line!(
                report,
                "{}",
                messages::terminal_info_none_configured(locale())
            );
        }
        ConfigurationTextReport::Failed {
            subject,
            status,
            code,
        } => append_report_line!(
            report,
            "{}",
            messages::doctor_integration_error(locale(), &code, &status, &subject)
        ),
        ConfigurationTextReport::Configured(integrations) => {
            for integration in integrations {
                let level = if integration.error_code.is_some() {
                    "ERROR"
                } else if integration.active {
                    "OK"
                } else {
                    "WARN"
                };
                let state = integration.state.terminal_label(locale());
                if let Some(code) = integration.error_code {
                    append_report_line!(report, "[{level}] {}: {state} ({code})", integration.id);
                } else {
                    append_report_line!(report, "[{level}] {}: {state}", integration.id);
                }
                if let Some(hint) = integration.state.recovery_hint() {
                    append_report_line!(report, "  {hint}");
                }
            }
        }
    }
}

fn render_telemetry_health(report: &mut String, telemetry: TelemetryTextReport) {
    match telemetry {
        TelemetryTextReport::State(enabled) => {
            let state = if enabled {
                messages::terminal_on_text(locale())
            } else {
                messages::terminal_off_text(locale())
            };
            append_report_line!(
                report,
                "{}",
                messages::terminal_info_telemetry(locale(), &(state))
            );
        }
        TelemetryTextReport::Failed => {
            append_report_line!(
                report,
                "{}",
                messages::terminal_error_telemetry_settings_unreadable_nh_telemetry_001(locale())
            );
        }
    }
}

fn evidence_source_label(
    source: nan_harness_runtime::desktop_compatibility::DesktopEvidenceSource,
) -> &'static str {
    match source {
        nan_harness_runtime::desktop_compatibility::DesktopEvidenceSource::EmbeddedRegistry => {
            messages::terminal_embedded_registry_text(locale())
        }
        nan_harness_runtime::desktop_compatibility::DesktopEvidenceSource::RemoteFeed => {
            messages::terminal_remote_compatibility_feed_text(locale())
        }
    }
}

fn evidence_label(
    evidence: nan_harness_runtime::desktop_compatibility::DesktopCompatibilityEvidence,
) -> &'static str {
    match evidence {
        nan_harness_runtime::desktop_compatibility::DesktopCompatibilityEvidence::LiveVerified => {
            messages::terminal_live_verified_text(locale())
        }
        nan_harness_runtime::desktop_compatibility::DesktopCompatibilityEvidence::ContractOnly => {
            messages::terminal_contract_only_text(locale())
        }
        nan_harness_runtime::desktop_compatibility::DesktopCompatibilityEvidence::Unavailable => {
            messages::terminal_unavailable_text(locale())
        }
    }
}
