use crate::app::{DoctorArgs, DoctorTarget};
use crate::commands::configuration::ConfigurationManager;
use crate::commands::credentials::resolve_existing_config;
use crate::commands::persistence::{
    PersistenceError, PersistenceManager, PersistentIntegration, discover_models,
};
use nan_harness_core::{
    CodingModelProfile, DesktopHarnessKind, HarnessKind, ProfileSource, ReasoningPolicy,
    VersionStatus,
};
use nan_harness_runtime::desktop_compatibility::{
    DesktopCompatibilityEvidence, desktop_compatibility,
};
use nan_harness_runtime::{DiscoveryError, DiscoveryOptions, discover_harness};
use nan_harness_telemetry::consent::TelemetrySettingsStore;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

const MODEL_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(10);
const DOCTOR_SCHEMA_VERSION: u8 = 5;
const HARNESS_DISCOVERY_CONCURRENCY: usize = 4;

fn append_report_line(report: &mut String, arguments: fmt::Arguments<'_>) {
    report.push_str(&arguments.to_string());
    report.push('\n');
}

macro_rules! append_report_line {
    ($report:expr, $($arguments:tt)*) => {
        append_report_line($report, format_args!($($arguments)*));
    };
}

pub(crate) async fn run(arguments: &DoctorArgs) -> Result<i32, DiscoveryError> {
    if let Some(target) = arguments.harness {
        match target {
            DoctorTarget::Stable(harness) if arguments.json => {
                Ok(print_harness_json_report(harness, arguments))
            }
            DoctorTarget::Stable(harness) => {
                print_harness_report(harness, arguments)?;
                Ok(0)
            }
            DoctorTarget::Experimental(kind) => Ok(print_experimental_report(kind, arguments.json)),
        }
    } else if arguments.json {
        let report = system_json_report().await;
        let exit_code = i32::from(report.has_errors());
        let Ok(serialized) = serde_json::to_string_pretty(&report) else {
            eprintln!("could not serialize the typed doctor report");
            return Ok(1);
        };
        println!("{serialized}");
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
    let Ok(serialized) = serde_json::to_string_pretty(&report) else {
        eprintln!("could not serialize the typed harness doctor report");
        return 1;
    };
    println!("{serialized}");
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
    append_report_line!(&mut report, "nan-harness");
    append_report_line!(&mut report, "[OK] Version: {}", env!("CARGO_PKG_VERSION"));
    append_report_line!(
        &mut report,
        "[OK] Platform: {}/{}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    write_provider_health(&mut report).await;

    let harnesses = discover_all_harnesses().await;
    append_report_line!(&mut report, "\nHarnesses");
    for (harness, discovery) in harnesses {
        write_harness_health(&mut report, harness, discovery);
    }

    append_report_line!(&mut report, "\nExperimental Desktop harnesses");
    for kind in DesktopHarnessKind::ALL {
        match desktop_compatibility(kind) {
            Ok(entry) => {
                append_report_line!(
                    &mut report,
                    "[INFO] {kind}: {} on {} ({})",
                    evidence_label(entry.evidence),
                    entry.platform,
                    entry.transport
                );
            }
            Err(error) => {
                append_report_line!(&mut report, "[WARN] {kind}: {error}");
            }
        }
    }

    append_report_line!(&mut report, "\nManaged harness configurations");
    write_configuration_health(&mut report);

    append_report_line!(&mut report, "\nTelemetry");
    write_telemetry_health(&mut report);

    append_report_line!(
        &mut report,
        "\nSafe to share: API keys, paths, prompts, model output, and private configuration are excluded."
    );
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
    experimental_harnesses: Vec<ExperimentalHarnessReport>,
    managed_configurations: IntegrationSection,
    telemetry: TelemetryReport,
    safe_to_share: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExperimentalHarnessReport {
    id: DesktopHarnessKind,
    level: DiagnosticLevel,
    platform: String,
    available: bool,
    evidence: DesktopCompatibilityEvidence,
    transport: nan_harness_core::DesktopTransport,
    #[serde(skip_serializing_if = "Option::is_none")]
    minimum_supported_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_compatible_version: Option<String>,
    compatible_at: String,
    safe_to_share: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExperimentalHarnessDoctorReport {
    schema_version: u8,
    harness: DesktopHarnessKind,
    experimental: bool,
    level: DiagnosticLevel,
    platform: String,
    available: bool,
    evidence: DesktopCompatibilityEvidence,
    transport: nan_harness_core::DesktopTransport,
    #[serde(skip_serializing_if = "Option::is_none")]
    minimum_supported_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_compatible_version: Option<String>,
    compatible_at: String,
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
    coding_models: Vec<CodingModelSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    http_status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_code: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct CodingModelSummary {
    id: String,
    context_window: u64,
    max_output_tokens: u64,
    image_input: bool,
    reasoning: ReasoningPolicy,
    source: ProfileSource,
}

impl From<CodingModelProfile> for CodingModelSummary {
    fn from(model: CodingModelProfile) -> Self {
        Self {
            id: model.id,
            context_window: model.context_window,
            max_output_tokens: model.max_output_tokens,
            image_input: model.image_input,
            reasoning: model.reasoning,
            source: model.source,
        }
    }
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

type HarnessDiscovery = (
    HarnessKind,
    Result<nan_harness_runtime::DiscoveryReport, DiscoveryError>,
);

async fn discover_all_harnesses() -> Vec<HarnessDiscovery> {
    discover_harnesses(&HarnessKind::ALL, |harness| {
        discover_harness(
            harness,
            None,
            DiscoveryOptions {
                allow_unsupported: true,
                allow_untested: true,
            },
        )
    })
    .await
}

async fn discover_harnesses<F>(harnesses: &[HarnessKind], discover: F) -> Vec<HarnessDiscovery>
where
    F: Fn(HarnessKind) -> Result<nan_harness_runtime::DiscoveryReport, DiscoveryError>
        + Send
        + Sync
        + 'static,
{
    let discover = Arc::new(discover);
    let mut workers = tokio::task::JoinSet::new();
    let initial_workers = harnesses.len().min(HARNESS_DISCOVERY_CONCURRENCY);
    for (index, &harness) in harnesses.iter().take(initial_workers).enumerate() {
        let discover = Arc::clone(&discover);
        workers.spawn_blocking(move || (index, harness, discover(harness)));
    }
    let mut next_index = initial_workers;
    let mut results = (0..harnesses.len())
        .map(|_| None)
        .collect::<Vec<Option<HarnessDiscovery>>>();

    while let Some(worker) = workers.join_next().await {
        let (index, harness, discovery) = match worker {
            Ok(worker) => worker,
            Err(error) => panic!("harness discovery worker panicked: {error}"),
        };
        results[index] = Some((harness, discovery));

        if next_index < harnesses.len() {
            let harness = harnesses[next_index];
            let discover = Arc::clone(&discover);
            workers.spawn_blocking(move || (next_index, harness, discover(harness)));
            next_index += 1;
        }
    }

    results
        .into_iter()
        .enumerate()
        .map(|(index, result)| {
            result.unwrap_or_else(|| panic!("harness discovery worker missing result: {index}"))
        })
        .collect()
}

async fn system_json_report() -> SystemDoctorReport {
    let provider = provider_json_report().await;
    let harnesses = harness_json_reports(discover_all_harnesses().await);
    SystemDoctorReport {
        schema_version: DOCTOR_SCHEMA_VERSION,
        nan_harness_version: env!("CARGO_PKG_VERSION"),
        platform: PlatformReport {
            operating_system: std::env::consts::OS,
            architecture: std::env::consts::ARCH,
        },
        provider,
        harnesses,
        experimental_harnesses: experimental_json_reports(),
        managed_configurations: integration_json_report(),
        telemetry: telemetry_json_report(),
        safe_to_share: true,
    }
}

fn experimental_json_reports() -> Vec<ExperimentalHarnessReport> {
    DesktopHarnessKind::ALL
        .into_iter()
        .filter_map(|kind| {
            let entry = desktop_compatibility(kind).ok()?;
            let available = entry.evidence != DesktopCompatibilityEvidence::Unavailable;
            Some(ExperimentalHarnessReport {
                id: kind,
                level: if available {
                    DiagnosticLevel::Warning
                } else {
                    DiagnosticLevel::Info
                },
                platform: entry.platform,
                available,
                evidence: entry.evidence,
                transport: entry.transport,
                minimum_supported_version: entry
                    .minimum_app_version
                    .map(|version| version.to_string()),
                last_compatible_version: entry
                    .last_compatible_app_version
                    .map(|version| version.to_string()),
                compatible_at: entry.compatible_at,
                safe_to_share: true,
            })
        })
        .collect()
}

fn print_experimental_report(kind: DesktopHarnessKind, json: bool) -> i32 {
    let Ok(entry) = desktop_compatibility(kind) else {
        if json {
            println!(
                "{{\"schemaVersion\":{DOCTOR_SCHEMA_VERSION},\"harness\":\"{kind}\",\"level\":\"error\",\"safeToShare\":true}}"
            );
        } else {
            println!("Experimental Desktop harness: {kind}\nCompatibility registry: unavailable");
        }
        return 1;
    };
    let available = entry.evidence != DesktopCompatibilityEvidence::Unavailable;
    let report = ExperimentalHarnessReport {
        id: kind,
        level: if available {
            DiagnosticLevel::Warning
        } else {
            DiagnosticLevel::Info
        },
        platform: entry.platform,
        available,
        evidence: entry.evidence,
        transport: entry.transport,
        minimum_supported_version: entry.minimum_app_version.map(|version| version.to_string()),
        last_compatible_version: entry
            .last_compatible_app_version
            .map(|version| version.to_string()),
        compatible_at: entry.compatible_at,
        safe_to_share: true,
    };
    if json {
        let json_report = ExperimentalHarnessDoctorReport {
            schema_version: DOCTOR_SCHEMA_VERSION,
            harness: report.id,
            experimental: true,
            level: report.level,
            platform: report.platform,
            available: report.available,
            evidence: report.evidence,
            transport: report.transport,
            minimum_supported_version: report.minimum_supported_version,
            last_compatible_version: report.last_compatible_version,
            compatible_at: report.compatible_at,
            safe_to_share: report.safe_to_share,
        };
        match serde_json::to_string_pretty(&json_report) {
            Ok(value) => println!("{value}"),
            Err(_) => return 1,
        }
    } else {
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
    }
    0
}

const fn evidence_label(evidence: DesktopCompatibilityEvidence) -> &'static str {
    match evidence {
        DesktopCompatibilityEvidence::LiveVerified => "live-verified",
        DesktopCompatibilityEvidence::ContractOnly => "contract-only",
        DesktopCompatibilityEvidence::Unavailable => "unavailable",
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
                coding_models: Vec::new(),
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
                coding_models: Vec::new(),
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
            coding_models: coding_model_summaries(models),
            http_status: None,
            error_code: None,
        },
        Ok(Err(PersistenceError::NoModels)) => ProviderReport {
            level: DiagnosticLevel::Warning,
            credential: "configured",
            api: "reachable",
            coding_model_count: Some(0),
            coding_models: Vec::new(),
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
            coding_models: Vec::new(),
            http_status: Some(status),
            error_code: Some("NH-PERSISTENCE-003"),
        },
        Ok(Err(PersistenceError::ParseModels(_))) => ProviderReport {
            level: DiagnosticLevel::Error,
            credential: "configured",
            api: "invalid-response",
            coding_model_count: None,
            coding_models: Vec::new(),
            http_status: None,
            error_code: Some("NH-PERSISTENCE-004"),
        },
        Ok(Err(error)) => ProviderReport {
            level: DiagnosticLevel::Error,
            credential: "configured",
            api: "unavailable",
            coding_model_count: None,
            coding_models: Vec::new(),
            http_status: None,
            error_code: Some(error.code()),
        },
        Err(_) => ProviderReport {
            level: DiagnosticLevel::Error,
            credential: "configured",
            api: "timeout",
            coding_model_count: None,
            coding_models: Vec::new(),
            http_status: None,
            error_code: Some("NH-PERSISTENCE-002"),
        },
    }
}

fn coding_model_summaries(mut models: Vec<CodingModelProfile>) -> Vec<CodingModelSummary> {
    models.sort_by(|left, right| left.id.cmp(&right.id));
    models.into_iter().map(CodingModelSummary::from).collect()
}

fn model_catalog_text(models: &[CodingModelProfile]) -> Option<(String, bool)> {
    if models.is_empty() {
        return None;
    }

    let mut sorted = models.iter().collect::<Vec<_>>();
    sorted.sort_by(|left, right| left.id.cmp(&right.id));
    let generic_present = sorted
        .iter()
        .any(|model| model.source == ProfileSource::Generic);
    let mut ids = sorted
        .iter()
        .take(8)
        .map(|model| {
            if model.source == ProfileSource::Generic {
                format!("{}*", model.id)
            } else {
                model.id.clone()
            }
        })
        .collect::<Vec<_>>();
    if sorted.len() > ids.len() {
        ids.push(format!("+{} more", sorted.len() - ids.len()));
    }
    Some((ids.join(" · "), generic_present))
}

fn harness_json_reports(discoveries: Vec<HarnessDiscovery>) -> Vec<HarnessReport> {
    discoveries
        .into_iter()
        .map(|(harness, discovery)| harness_json_report(harness, discovery))
        .collect()
}

fn harness_json_report(
    harness: HarnessKind,
    discovery: Result<nan_harness_runtime::DiscoveryReport, DiscoveryError>,
) -> HarnessReport {
    match discovery {
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
            append_report_line!(report, "[INFO] API key: not configured");
            append_report_line!(
                report,
                "[SKIP] NaN API and model discovery: API key required"
            );
            return;
        }
        Err(error) => {
            append_report_line!(
                report,
                "[ERROR] Provider configuration: invalid ({})",
                error.code()
            );
            return;
        }
    };

    append_report_line!(report, "[OK] API key: configured");
    match tokio::time::timeout(MODEL_DISCOVERY_TIMEOUT, discover_models(&config)).await {
        Ok(Ok(models)) => {
            append_report_line!(report, "[OK] NaN API: reachable");
            append_report_line!(report, "[OK] Coding models: {} available", models.len());
            if let Some((catalog, generic_present)) = model_catalog_text(&models) {
                append_report_line!(report, "[INFO] Model catalog: {catalog}");
                if generic_present {
                    append_report_line!(
                        report,
                        "[INFO] * conservative default profile; limits are not provider-authoritative"
                    );
                }
            }
        }
        Ok(Err(PersistenceError::NoModels)) => {
            append_report_line!(report, "[OK] NaN API: reachable");
            append_report_line!(report, "[WARN] Coding models: none available");
        }
        Ok(Err(PersistenceError::ModelDiscoveryStatus(status))) => {
            let diagnosis = if matches!(status, 401 | 403) {
                "authentication rejected"
            } else {
                "request rejected"
            };
            append_report_line!(report, "[ERROR] NaN API: {diagnosis} (HTTP {status})");
        }
        Ok(Err(PersistenceError::ParseModels(_))) => {
            append_report_line!(report, "[OK] NaN API: reachable");
            append_report_line!(report, "[ERROR] Coding models: invalid API response");
        }
        Ok(Err(error)) => {
            append_report_line!(report, "[ERROR] NaN API: unavailable ({})", error.code());
        }
        Err(_) => {
            append_report_line!(report, "[ERROR] NaN API: timed out after 10 seconds");
        }
    }
}

fn write_harness_health(
    report: &mut String,
    harness: HarnessKind,
    discovery: Result<nan_harness_runtime::DiscoveryReport, DiscoveryError>,
) {
    match discovery {
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
            append_report_line!(report, "[{level}] {harness}: {version} ({label})");
        }
        Err(DiscoveryError::ExecutableNotFound(_)) => {
            append_report_line!(report, "[INFO] {harness}: not installed");
        }
        Err(error) => {
            append_report_line!(report, "[ERROR] {harness}: check failed ({})", error.code());
        }
    }
}

fn write_configuration_health(report: &mut String) {
    let configuration_manager = match ConfigurationManager::from_environment() {
        Ok(manager) => manager,
        Err(error) => {
            append_report_line!(
                report,
                "[ERROR] Configuration state: unavailable ({})",
                error.code()
            );
            return;
        }
    };
    let manager = match PersistenceManager::from_environment() {
        Ok(manager) => manager,
        Err(error) => {
            append_report_line!(
                report,
                "[ERROR] Integration state: unavailable ({})",
                error.code()
            );
            return;
        }
    };
    let integrations = match manager.configured_integrations() {
        Ok(integrations) => integrations,
        Err(error) => {
            append_report_line!(
                report,
                "[ERROR] Integration state: unreadable ({})",
                error.code()
            );
            return;
        }
    };
    let native = match configuration_manager.configured_harnesses() {
        Ok(configurations) => configurations,
        Err(error) => {
            append_report_line!(
                report,
                "[ERROR] Configuration state: unreadable ({})",
                error.code()
            );
            return;
        }
    };
    if integrations.is_empty() && native.is_empty() {
        append_report_line!(report, "[INFO] None configured");
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
        append_report_line!(report, "[{level}] {harness}: {state}");
    }
}

fn write_telemetry_health(report: &mut String) {
    match TelemetrySettingsStore::from_environment().and_then(|store| store.load()) {
        Ok(settings) => {
            let state = if settings.enabled() { "on" } else { "off" };
            append_report_line!(report, "[INFO] Telemetry: {state}");
        }
        Err(_) => {
            append_report_line!(
                report,
                "[ERROR] Telemetry settings: unreadable (NH-TELEMETRY-001)"
            );
        }
    }
}

fn normalized_version(output: &str) -> Option<String> {
    output.split_whitespace().find_map(|token| {
        let candidate = token
            .rsplit_once('/')
            .map_or(token, |(_, version)| version)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Barrier, Mutex};

    #[test]
    fn normalized_version_accepts_slash_prefixed_versions() {
        assert_eq!(
            normalized_version("omp/18.0.11"),
            Some("18.0.11".to_owned())
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn harness_discovery_is_bounded_concurrent_and_ordered() {
        let harnesses = HarnessKind::ALL[..8].to_vec();
        let active = Arc::new(AtomicUsize::new(0));
        let maximum_active = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(Barrier::new(4));
        let completed = Arc::new(Mutex::new(Vec::with_capacity(harnesses.len())));
        let harnesses_for_discovery = harnesses.clone();
        let harnesses_for_worker = harnesses.clone();
        let active_for_worker = Arc::clone(&active);
        let maximum_active_for_worker = Arc::clone(&maximum_active);
        let completed_for_worker = Arc::clone(&completed);
        let discoveries = discover_harnesses(&harnesses_for_discovery, move |harness| {
            let index = harnesses_for_worker
                .iter()
                .position(|candidate| *candidate == harness)
                .expect("test harness should be in the input batch");
            let current = active_for_worker.fetch_add(1, Ordering::SeqCst) + 1;
            maximum_active_for_worker.fetch_max(current, Ordering::SeqCst);

            if index < HARNESS_DISCOVERY_CONCURRENCY {
                barrier.wait();
            }
            std::thread::sleep(Duration::from_millis((8 - index) as u64 * 5));

            completed_for_worker
                .lock()
                .expect("completion list should not be poisoned")
                .push(harness);
            active_for_worker.fetch_sub(1, Ordering::SeqCst);

            Err(DiscoveryError::ExecutableNotFound(harness.to_string()))
        })
        .await;

        let completion_order = completed
            .lock()
            .expect("completion list should not be poisoned")
            .clone();
        assert_eq!(
            maximum_active.load(Ordering::SeqCst),
            HARNESS_DISCOVERY_CONCURRENCY
        );
        assert!(maximum_active.load(Ordering::SeqCst) > 1);
        assert_eq!(active.load(Ordering::SeqCst), 0);
        assert_eq!(completion_order.len(), harnesses.len());
        assert_ne!(completion_order, harnesses);

        let discovered_harnesses = discoveries
            .iter()
            .map(|(harness, _)| *harness)
            .collect::<Vec<_>>();
        assert_eq!(discovered_harnesses, harnesses);
    }

    #[test]
    fn harness_reports_preserve_all_harnesses_and_schema() {
        let discoveries = HarnessKind::ALL
            .into_iter()
            .map(|harness| {
                (
                    harness,
                    Err(DiscoveryError::ExecutableNotFound(
                        harness.binary_name().to_owned(),
                    )),
                )
            })
            .collect();

        let reports = harness_json_reports(discoveries);

        assert_eq!(DOCTOR_SCHEMA_VERSION, 5);
        assert_eq!(reports.len(), HarnessKind::ALL.len());
        assert_eq!(
            reports.iter().map(|report| report.id).collect::<Vec<_>>(),
            HarnessKind::ALL,
        );
        assert!(reports.iter().all(|report| {
            report.level == DiagnosticLevel::Info
                && !report.installed
                && report.error_code.is_none()
        }));
    }

    #[test]
    fn coding_model_summaries_are_sorted_and_preserve_capabilities() {
        let generic = CodingModelProfile::generic("future-model");
        let bundled = CodingModelProfile {
            id: "gemma4".to_owned(),
            display_name: "NaN · Gemma 4".to_owned(),
            description: "Opt-in reasoning · tools + vision · 256K".to_owned(),
            context_window: 262_144,
            max_output_tokens: 65_536,
            image_input: true,
            reasoning: ReasoningPolicy::Toggle {
                default_enabled: false,
            },
            source: ProfileSource::Bundled,
        };

        let summaries = coding_model_summaries(vec![bundled, generic]);
        let value = serde_json::to_value(summaries).expect("summaries should serialize");

        assert_eq!(
            value,
            serde_json::json!([
                {
                    "id": "future-model",
                    "contextWindow": 262_144,
                    "maxOutputTokens": 32_768,
                    "imageInput": false,
                    "reasoning": {"kind": "unknown"},
                    "source": "generic"
                },
                {
                    "id": "gemma4",
                    "contextWindow": 262_144,
                    "maxOutputTokens": 65_536,
                    "imageInput": true,
                    "reasoning": {"kind": "toggle", "defaultEnabled": false},
                    "source": "bundled"
                }
            ])
        );
    }

    #[test]
    fn model_catalog_text_is_capped_and_marks_generic_profiles() {
        let mut models = (0..10)
            .map(|index| CodingModelProfile::generic(&format!("model-{index:02}")))
            .collect::<Vec<_>>();
        models.reverse();

        let (catalog, generic_present) =
            model_catalog_text(&models).expect("non-empty catalog should render");

        assert_eq!(
            catalog,
            "model-00* · model-01* · model-02* · model-03* · model-04* · model-05* · model-06* · model-07* · +2 more"
        );
        assert!(generic_present);
    }

    #[test]
    fn model_catalog_text_is_absent_without_models() {
        assert_eq!(model_catalog_text(&[]), None);
    }
}
