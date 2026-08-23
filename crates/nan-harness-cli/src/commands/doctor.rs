use crate::app::DoctorArgs;
use crate::commands::configuration::ConfigurationManager;
use crate::commands::credentials::resolve_existing_config;
use crate::commands::persistence::{
    PersistenceError, PersistenceManager, PersistentIntegration, discover_models,
};
use nan_harness_core::{HarnessKind, VersionStatus};
use nan_harness_runtime::{DiscoveryError, DiscoveryOptions, discover_harness};
use nan_harness_telemetry::consent::TelemetrySettingsStore;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::time::Duration;

const MODEL_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(10);
const DOCTOR_SCHEMA_VERSION: u8 = 3;

pub(crate) async fn run(arguments: &DoctorArgs) -> Result<i32, DiscoveryError> {
    if let Some(harness) = arguments.harness {
        if arguments.json {
            Ok(print_harness_json_report(harness, arguments))
        } else {
            print_harness_report(harness, arguments)?;
            Ok(0)
        }
    } else if arguments.json {
        let report = system_json_report().await;
        let exit_code = i32::from(report.has_errors());
        println!(
            "{}",
            serde_json::to_string_pretty(&report)
                .expect("the typed doctor report should always serialize")
        );
        Ok(exit_code)
    } else {
        print!("{}", system_report().await);
        Ok(0)
    }
}

fn print_harness_json_report(harness: HarnessKind, arguments: &DoctorArgs) -> i32 {
    let discovery = discover_harness(
        harness,
        arguments.executable.as_deref(),
        DiscoveryOptions {
            allow_unsupported: arguments.allow_unsupported,
            allow_untested: arguments.allow_untested,
        },
    );
    let (report, exit_code) = match discovery {
        Ok(discovery) => {
            let level = diagnostic_level(discovery.harness.version_status);
            let exit_code = i32::from(level == DiagnosticLevel::Error);
            (
                HarnessDoctorReport {
                    schema_version: DOCTOR_SCHEMA_VERSION,
                    harness: discovery.harness.kind,
                    level,
                    installed: true,
                    version: normalized_version(&discovery.harness.detected_version),
                    minimum_supported_version: Some(
                        discovery.minimum_supported_version.to_string(),
                    ),
                    last_compatible_version: Some(discovery.last_compatible_version.to_string()),
                    compatible_at: Some(discovery.compatible_at),
                    last_live_verified_version: discovery
                        .last_live_verified_version
                        .map(|version| version.to_string()),
                    live_verified_at: discovery.live_verified_at,
                    compatibility: Some(compatibility_label(discovery.harness.version_status)),
                    warnings: discovery.warnings,
                    error_code: None,
                    safe_to_share: true,
                },
                exit_code,
            )
        }
        Err(error) => (
            HarnessDoctorReport {
                schema_version: DOCTOR_SCHEMA_VERSION,
                harness,
                level: DiagnosticLevel::Error,
                installed: !matches!(&error, DiscoveryError::ExecutableNotFound(_)),
                version: None,
                minimum_supported_version: None,
                last_compatible_version: None,
                compatible_at: None,
                last_live_verified_version: None,
                live_verified_at: None,
                compatibility: None,
                warnings: Vec::new(),
                error_code: Some(error.code()),
                safe_to_share: true,
            },
            1,
        ),
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&report)
            .expect("the typed harness doctor report should always serialize")
    );
    exit_code
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
    println!("Last compatible: {}", report.last_compatible_version);
    println!("Compatible at: {}", report.compatible_at);
    println!(
        "Last live verified: {}",
        report
            .last_live_verified_version
            .as_ref()
            .map_or_else(|| "none".to_owned(), ToString::to_string)
    );
    println!(
        "Live verified at: {}",
        report.live_verified_at.as_deref().unwrap_or("none")
    );
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
    writeln!(report, "nan-harness").expect("writing to a String cannot fail");
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

    writeln!(report, "\nManaged harness configurations").expect("writing to a String cannot fail");
    write_configuration_health(&mut report);

    writeln!(report, "\nTelemetry").expect("writing to a String cannot fail");
    write_telemetry_health(&mut report);

    writeln!(
        report,
        "\nSafe to share: API keys, paths, prompts, model output, and private configuration are excluded."
    )
    .expect("writing to a String cannot fail");
    report
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum DiagnosticLevel {
    Ok,
    Warning,
    Info,
    Error,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HarnessDoctorReport {
    schema_version: u8,
    harness: HarnessKind,
    level: DiagnosticLevel,
    installed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    minimum_supported_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_compatible_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    compatible_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_live_verified_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    live_verified_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    compatibility: Option<&'static str>,
    warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_code: Option<&'static str>,
    safe_to_share: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SystemDoctorReport {
    schema_version: u8,
    nan_harness_version: &'static str,
    platform: PlatformReport,
    provider: ProviderReport,
    harnesses: Vec<HarnessReport>,
    managed_configurations: IntegrationSection,
    telemetry: TelemetryReport,
    safe_to_share: bool,
}

impl SystemDoctorReport {
    fn has_errors(&self) -> bool {
        self.provider.level == DiagnosticLevel::Error
            || self
                .harnesses
                .iter()
                .any(|harness| harness.level == DiagnosticLevel::Error)
            || self.managed_configurations.level == DiagnosticLevel::Error
            || self.telemetry.level == DiagnosticLevel::Error
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PlatformReport {
    operating_system: &'static str,
    architecture: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderReport {
    level: DiagnosticLevel,
    credential: &'static str,
    api: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    coding_model_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    http_status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_code: Option<&'static str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HarnessReport {
    id: HarnessKind,
    level: DiagnosticLevel,
    installed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    minimum_supported_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_compatible_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    compatible_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_live_verified_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    live_verified_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    compatibility: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_code: Option<&'static str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct IntegrationSection {
    level: DiagnosticLevel,
    integrations: Vec<IntegrationReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_code: Option<&'static str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct IntegrationReport {
    id: String,
    active: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TelemetryReport {
    level: DiagnosticLevel,
    #[serde(skip_serializing_if = "Option::is_none")]
    enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_code: Option<&'static str>,
}

async fn system_json_report() -> SystemDoctorReport {
    SystemDoctorReport {
        schema_version: DOCTOR_SCHEMA_VERSION,
        nan_harness_version: env!("CARGO_PKG_VERSION"),
        platform: PlatformReport {
            operating_system: std::env::consts::OS,
            architecture: std::env::consts::ARCH,
        },
        provider: provider_json_report().await,
        harnesses: HarnessKind::ALL
            .into_iter()
            .map(harness_json_report)
            .collect(),
        managed_configurations: integration_json_report(),
        telemetry: telemetry_json_report(),
        safe_to_share: true,
    }
}

async fn provider_json_report() -> ProviderReport {
    let config = match resolve_existing_config(None) {
        Ok(Some(config)) => config,
        Ok(None) => {
            return ProviderReport {
                level: DiagnosticLevel::Info,
                credential: "not-configured",
                api: "skipped",
                coding_model_count: None,
                http_status: None,
                error_code: None,
            };
        }
        Err(error) => {
            return ProviderReport {
                level: DiagnosticLevel::Error,
                credential: "invalid",
                api: "skipped",
                coding_model_count: None,
                http_status: None,
                error_code: Some(error.code()),
            };
        }
    };

    match tokio::time::timeout(MODEL_DISCOVERY_TIMEOUT, discover_models(&config)).await {
        Ok(Ok(models)) => ProviderReport {
            level: DiagnosticLevel::Ok,
            credential: "configured",
            api: "reachable",
            coding_model_count: Some(models.len()),
            http_status: None,
            error_code: None,
        },
        Ok(Err(PersistenceError::NoModels)) => ProviderReport {
            level: DiagnosticLevel::Warning,
            credential: "configured",
            api: "reachable",
            coding_model_count: Some(0),
            http_status: None,
            error_code: None,
        },
        Ok(Err(PersistenceError::ModelDiscoveryStatus(status))) => ProviderReport {
            level: DiagnosticLevel::Error,
            credential: "configured",
            api: if matches!(status, 401 | 403) {
                "authentication-rejected"
            } else {
                "request-rejected"
            },
            coding_model_count: None,
            http_status: Some(status),
            error_code: Some("NH-PERSISTENCE-003"),
        },
        Ok(Err(PersistenceError::ParseModels(_))) => ProviderReport {
            level: DiagnosticLevel::Error,
            credential: "configured",
            api: "invalid-response",
            coding_model_count: None,
            http_status: None,
            error_code: Some("NH-PERSISTENCE-004"),
        },
        Ok(Err(error)) => ProviderReport {
            level: DiagnosticLevel::Error,
            credential: "configured",
            api: "unavailable",
            coding_model_count: None,
            http_status: None,
            error_code: Some(error.code()),
        },
        Err(_) => ProviderReport {
            level: DiagnosticLevel::Error,
            credential: "configured",
            api: "timeout",
            coding_model_count: None,
            http_status: None,
            error_code: Some("NH-PERSISTENCE-002"),
        },
    }
}

fn harness_json_report(harness: HarnessKind) -> HarnessReport {
    match discover_harness(
        harness,
        None,
        DiscoveryOptions {
            allow_unsupported: true,
            allow_untested: true,
        },
    ) {
        Ok(discovery) => HarnessReport {
            id: harness,
            level: diagnostic_level(discovery.harness.version_status),
            installed: true,
            version: normalized_version(&discovery.harness.detected_version),
            minimum_supported_version: Some(discovery.minimum_supported_version.to_string()),
            last_compatible_version: Some(discovery.last_compatible_version.to_string()),
            compatible_at: Some(discovery.compatible_at),
            last_live_verified_version: discovery
                .last_live_verified_version
                .map(|version| version.to_string()),
            live_verified_at: discovery.live_verified_at,
            compatibility: Some(compatibility_label(discovery.harness.version_status)),
            error_code: None,
        },
        Err(DiscoveryError::ExecutableNotFound(_)) => HarnessReport {
            id: harness,
            level: DiagnosticLevel::Info,
            installed: false,
            version: None,
            minimum_supported_version: None,
            last_compatible_version: None,
            compatible_at: None,
            last_live_verified_version: None,
            live_verified_at: None,
            compatibility: None,
            error_code: None,
        },
        Err(error) => HarnessReport {
            id: harness,
            level: DiagnosticLevel::Error,
            installed: true,
            version: None,
            minimum_supported_version: None,
            last_compatible_version: None,
            compatible_at: None,
            last_live_verified_version: None,
            live_verified_at: None,
            compatibility: None,
            error_code: Some(error.code()),
        },
    }
}

fn integration_json_report() -> IntegrationSection {
    let configuration_manager = match ConfigurationManager::from_environment() {
        Ok(manager) => manager,
        Err(error) => {
            return IntegrationSection {
                level: DiagnosticLevel::Error,
                integrations: Vec::new(),
                error_code: Some(error.code()),
            };
        }
    };
    let manager = match PersistenceManager::from_environment() {
        Ok(manager) => manager,
        Err(error) => {
            return IntegrationSection {
                level: DiagnosticLevel::Error,
                integrations: Vec::new(),
                error_code: Some(error.code()),
            };
        }
    };
    let integrations = match manager.configured_integrations() {
        Ok(integrations) => integrations,
        Err(error) => {
            return IntegrationSection {
                level: DiagnosticLevel::Error,
                integrations: Vec::new(),
                error_code: Some(error.code()),
            };
        }
    };
    let native = match configuration_manager.configured_harnesses() {
        Ok(configurations) => configurations,
        Err(error) => {
            return IntegrationSection {
                level: DiagnosticLevel::Error,
                integrations: Vec::new(),
                error_code: Some(error.code()),
            };
        }
    };
    let mut reports = BTreeMap::new();
    for harness in native {
        reports.insert(
            harness.to_string(),
            configuration_manager.is_active(harness).unwrap_or(false),
        );
    }
    for integration in integrations {
        reports
            .entry(persistent_integration_id(integration).to_owned())
            .or_insert_with(|| manager.integration_is_active(integration));
    }
    let integrations = reports
        .into_iter()
        .map(|(id, active)| IntegrationReport { id, active })
        .collect::<Vec<_>>();
    let level = if integrations.iter().all(|integration| integration.active) {
        DiagnosticLevel::Info
    } else {
        DiagnosticLevel::Warning
    };
    IntegrationSection {
        level,
        integrations,
        error_code: None,
    }
}

const fn persistent_integration_id(integration: PersistentIntegration) -> &'static str {
    match integration {
        PersistentIntegration::OpenCode => "opencode",
        PersistentIntegration::Pi => "pi",
        PersistentIntegration::PrimeAgent => "prime-agent",
        PersistentIntegration::QwenCode => "qwen-code",
        PersistentIntegration::DeepSeekHarness => "deepseek-harness",
        PersistentIntegration::Aider => "aider",
    }
}

fn telemetry_json_report() -> TelemetryReport {
    match TelemetrySettingsStore::from_environment().and_then(|store| store.load()) {
        Ok(settings) => TelemetryReport {
            level: DiagnosticLevel::Info,
            enabled: Some(settings.enabled()),
            error_code: None,
        },
        Err(_) => TelemetryReport {
            level: DiagnosticLevel::Error,
            enabled: None,
            error_code: Some("NH-TELEMETRY-001"),
        },
    }
}

const fn diagnostic_level(status: VersionStatus) -> DiagnosticLevel {
    match status {
        VersionStatus::Tested | VersionStatus::Supported => DiagnosticLevel::Ok,
        VersionStatus::NewerUntested | VersionStatus::Unparseable => DiagnosticLevel::Warning,
        VersionStatus::OlderUnsupported => DiagnosticLevel::Error,
    }
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
                VersionStatus::NewerUntested => ("WARN", "newer than compatible"),
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

fn write_configuration_health(report: &mut String) {
    let configuration_manager = match ConfigurationManager::from_environment() {
        Ok(manager) => manager,
        Err(error) => {
            writeln!(
                report,
                "[ERROR] Configuration state: unavailable ({})",
                error.code()
            )
            .expect("writing to a String cannot fail");
            return;
        }
    };
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
    let native = match configuration_manager.configured_harnesses() {
        Ok(configurations) => configurations,
        Err(error) => {
            writeln!(
                report,
                "[ERROR] Configuration state: unreadable ({})",
                error.code()
            )
            .expect("writing to a String cannot fail");
            return;
        }
    };
    if integrations.is_empty() && native.is_empty() {
        writeln!(report, "[INFO] None configured").expect("writing to a String cannot fail");
        return;
    }
    let mut reported = BTreeMap::new();
    for harness in native {
        let active = configuration_manager.is_active(harness).unwrap_or(false);
        reported.insert(harness.to_string(), active);
    }
    for integration in integrations {
        reported
            .entry(persistent_integration_id(integration).to_owned())
            .or_insert_with(|| manager.integration_is_active(integration));
    }
    for (harness, active) in reported {
        let (level, state) = if active {
            ("OK", "active")
        } else {
            ("WARN", "managed configuration changed or missing")
        };
        writeln!(report, "[{level}] {harness}: {state}").expect("writing to a String cannot fail");
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
