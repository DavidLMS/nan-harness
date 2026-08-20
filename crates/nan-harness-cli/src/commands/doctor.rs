use crate::app::DoctorArgs;
use crate::commands::credentials::resolve_existing_config;
use crate::commands::persistence::{PersistenceError, PersistenceManager, discover_models};
use nan_harness_core::{HarnessKind, VersionStatus};
use nan_harness_runtime::{DiscoveryError, DiscoveryOptions, discover_harness};
use nan_harness_telemetry::consent::TelemetrySettingsStore;
use std::fmt::Write as _;
use std::time::Duration;

const MODEL_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) async fn run(arguments: &DoctorArgs) -> Result<(), DiscoveryError> {
    if let Some(harness) = arguments.harness {
        print_harness_report(harness, arguments)?;
    } else {
        print!("{}", system_report().await);
    }
    Ok(())
}

fn print_harness_report(
    harness: HarnessKind,
    arguments: &DoctorArgs,
) -> Result<(), DiscoveryError> {
    let report = discover_harness(
        harness,
        arguments.executable.as_deref(),
        DiscoveryOptions {
            allow_unsupported: arguments.allow_unsupported,
            allow_untested: arguments.allow_untested,
        },
    )?;

    println!("Harness: {}", report.harness.kind);
    println!("Executable: {}", report.harness.executable);
    println!("Version output: {}", report.harness.detected_version);
    println!("Minimum supported: {}", report.minimum_supported_version);
    println!("Last verified: {}", report.last_verified_version);
    println!(
        "Compatibility: {}",
        compatibility_label(report.harness.version_status)
    );
    for warning in report.warnings {
        println!("Warning: {warning}");
    }
    Ok(())
}

async fn system_report() -> String {
    let mut report = String::new();
    writeln!(report, "NaN").expect("writing to a String cannot fail");
    writeln!(report, "[OK] Version: {}", env!("CARGO_PKG_VERSION"))
        .expect("writing to a String cannot fail");
    writeln!(
        report,
        "[OK] Platform: {}/{}",
        std::env::consts::OS,
        std::env::consts::ARCH
    )
    .expect("writing to a String cannot fail");
    write_provider_health(&mut report).await;

    writeln!(report, "\nHarnesses").expect("writing to a String cannot fail");
    for harness in HarnessKind::ALL {
        write_harness_health(&mut report, harness);
    }

    writeln!(report, "\nPersistent integrations").expect("writing to a String cannot fail");
    write_persistence_health(&mut report);

    writeln!(report, "\nTelemetry").expect("writing to a String cannot fail");
    write_telemetry_health(&mut report);

    writeln!(
        report,
        "\nSafe to share: API keys, paths, prompts, model output, and private configuration are excluded."
    )
    .expect("writing to a String cannot fail");
    report
}

async fn write_provider_health(report: &mut String) {
    let config = match resolve_existing_config(None) {
        Ok(Some(config)) => config,
        Ok(None) => {
            writeln!(report, "[INFO] API key: not configured")
                .expect("writing to a String cannot fail");
            writeln!(
                report,
                "[SKIP] NaN API and model discovery: API key required"
            )
            .expect("writing to a String cannot fail");
            return;
        }
        Err(error) => {
            writeln!(
                report,
                "[ERROR] Provider configuration: invalid ({})",
                error.code()
            )
            .expect("writing to a String cannot fail");
            return;
        }
    };

    writeln!(report, "[OK] API key: configured").expect("writing to a String cannot fail");
    match tokio::time::timeout(MODEL_DISCOVERY_TIMEOUT, discover_models(&config)).await {
        Ok(Ok(models)) => {
            writeln!(report, "[OK] NaN API: reachable").expect("writing to a String cannot fail");
            writeln!(report, "[OK] Coding models: {} available", models.len())
                .expect("writing to a String cannot fail");
        }
        Ok(Err(PersistenceError::NoModels)) => {
            writeln!(report, "[OK] NaN API: reachable").expect("writing to a String cannot fail");
            writeln!(report, "[WARN] Coding models: none available")
                .expect("writing to a String cannot fail");
        }
        Ok(Err(PersistenceError::ModelDiscoveryStatus(status))) => {
            let diagnosis = if matches!(status, 401 | 403) {
                "authentication rejected"
            } else {
                "request rejected"
            };
            writeln!(report, "[ERROR] NaN API: {diagnosis} (HTTP {status})")
                .expect("writing to a String cannot fail");
        }
        Ok(Err(PersistenceError::ParseModels(_))) => {
            writeln!(report, "[OK] NaN API: reachable").expect("writing to a String cannot fail");
            writeln!(report, "[ERROR] Coding models: invalid API response")
                .expect("writing to a String cannot fail");
        }
        Ok(Err(error)) => {
            writeln!(report, "[ERROR] NaN API: unavailable ({})", error.code())
                .expect("writing to a String cannot fail");
        }
        Err(_) => {
            writeln!(report, "[ERROR] NaN API: timed out after 10 seconds")
                .expect("writing to a String cannot fail");
        }
    }
}

fn write_harness_health(report: &mut String, harness: HarnessKind) {
    match discover_harness(
        harness,
        None,
        DiscoveryOptions {
            allow_unsupported: true,
            allow_untested: true,
        },
    ) {
        Ok(discovery) => {
            let version = normalized_version(&discovery.harness.detected_version)
                .unwrap_or_else(|| "unparseable".to_owned());
            let (level, label) = match discovery.harness.version_status {
                VersionStatus::Tested => ("OK", "tested"),
                VersionStatus::Supported => ("OK", "supported"),
                VersionStatus::NewerUntested => ("WARN", "newer than verified"),
                VersionStatus::OlderUnsupported => ("ERROR", "unsupported"),
                VersionStatus::Unparseable => ("WARN", "version unparseable"),
            };
            writeln!(report, "[{level}] {harness}: {version} ({label})")
                .expect("writing to a String cannot fail");
        }
        Err(DiscoveryError::ExecutableNotFound(_)) => {
            writeln!(report, "[INFO] {harness}: not installed")
                .expect("writing to a String cannot fail");
        }
        Err(error) => {
            writeln!(report, "[ERROR] {harness}: check failed ({})", error.code())
                .expect("writing to a String cannot fail");
        }
    }
}

fn write_persistence_health(report: &mut String) {
    let manager = match PersistenceManager::from_environment() {
        Ok(manager) => manager,
        Err(error) => {
            writeln!(
                report,
                "[ERROR] Integration state: unavailable ({})",
                error.code()
            )
            .expect("writing to a String cannot fail");
            return;
        }
    };
    let integrations = match manager.configured_integrations() {
        Ok(integrations) => integrations,
        Err(error) => {
            writeln!(
                report,
                "[ERROR] Integration state: unreadable ({})",
                error.code()
            )
            .expect("writing to a String cannot fail");
            return;
        }
    };
    if integrations.is_empty() {
        writeln!(report, "[INFO] None configured").expect("writing to a String cannot fail");
        return;
    }
    for integration in integrations {
        if manager.integration_is_active(integration) {
            writeln!(report, "[OK] {integration}: active")
                .expect("writing to a String cannot fail");
        } else {
            writeln!(
                report,
                "[WARN] {integration}: managed configuration changed or missing"
            )
            .expect("writing to a String cannot fail");
        }
    }
}

fn write_telemetry_health(report: &mut String) {
    match TelemetrySettingsStore::from_environment().and_then(|store| store.load()) {
        Ok(settings) => {
            let state = if settings.enabled() { "on" } else { "off" };
            writeln!(report, "[INFO] Telemetry: {state}").expect("writing to a String cannot fail");
        }
        Err(_) => {
            writeln!(
                report,
                "[ERROR] Telemetry settings: unreadable (NH-TELEMETRY-001)"
            )
            .expect("writing to a String cannot fail");
        }
    }
}

fn normalized_version(output: &str) -> Option<String> {
    output.split_whitespace().find_map(|token| {
        let candidate = token
            .trim_matches(|character: char| !character.is_ascii_alphanumeric() && character != '.')
            .trim_start_matches('v');
        semver::Version::parse(candidate)
            .ok()
            .map(|version| version.to_string())
    })
}

const fn compatibility_label(status: VersionStatus) -> &'static str {
    match status {
        VersionStatus::Tested => "tested",
        VersionStatus::Supported => "supported",
        VersionStatus::NewerUntested => "newer-untested",
        VersionStatus::OlderUnsupported => "older-unsupported",
        VersionStatus::Unparseable => "unparseable",
    }
}
