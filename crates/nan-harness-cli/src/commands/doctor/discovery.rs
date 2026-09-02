use crate::commands::configuration::ConfigurationManager;
use crate::commands::credentials::resolve_existing_config;
use crate::commands::pen_desktop;
use crate::commands::persistence::{
    PersistenceError, PersistenceManager, PersistentIntegration, discover_models,
};
use nan_harness_core::{CodingModelProfile, DesktopHarnessKind, HarnessKind};
use nan_harness_runtime::desktop_compatibility::{
    DesktopCompatibilityEntry, DesktopCompatibilityError, desktop_compatibility,
};
use nan_harness_runtime::{DiscoveryError, DiscoveryOptions, DiscoveryReport, discover_harness};
use nan_harness_telemetry::consent::TelemetrySettingsStore;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

pub(crate) const MODEL_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(10);
pub(crate) const HARNESS_DISCOVERY_CONCURRENCY: usize = 4;

pub(crate) type HarnessDiscovery = (HarnessKind, Result<DiscoveryReport, DiscoveryError>);
pub(crate) type DesktopDiscovery = (
    DesktopHarnessKind,
    Result<DesktopCompatibilityEntry, DesktopCompatibilityError>,
);

#[derive(Debug)]
pub(crate) enum ProviderDiscovery {
    NotConfigured,
    Invalid(&'static str),
    Models(Vec<CodingModelProfile>),
    NoModels,
    Status(u16),
    InvalidResponse,
    Unavailable(&'static str),
    Timeout,
}

#[derive(Debug)]
pub(crate) struct IntegrationStatus {
    pub(crate) id: String,
    pub(crate) active: bool,
}

#[derive(Debug)]
pub(crate) enum IntegrationDiscovery {
    Configured(Vec<IntegrationStatus>),
    Failed {
        subject: &'static str,
        status: &'static str,
        code: &'static str,
    },
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum TelemetryDiscovery {
    State(bool),
    Failed,
}

#[derive(Debug)]
pub(crate) struct SystemDiscovery {
    pub(crate) provider: ProviderDiscovery,
    pub(crate) harnesses: Vec<HarnessDiscovery>,
    pub(crate) experimental_harnesses: Vec<DesktopDiscovery>,
    pub(crate) managed_configurations: IntegrationDiscovery,
    pub(crate) telemetry: TelemetryDiscovery,
}

pub(crate) async fn system() -> SystemDiscovery {
    SystemDiscovery {
        provider: provider().await,
        harnesses: all_harnesses().await,
        experimental_harnesses: DesktopHarnessKind::ALL
            .into_iter()
            .map(|kind| (kind, desktop_compatibility(kind)))
            .collect(),
        managed_configurations: integrations(),
        telemetry: telemetry(),
    }
}

pub(crate) fn one_harness(
    harness: HarnessKind,
    executable: Option<&std::path::Path>,
    allow_unsupported: bool,
    allow_untested: bool,
) -> Result<DiscoveryReport, DiscoveryError> {
    discover_harness(
        harness,
        executable,
        DiscoveryOptions {
            allow_unsupported,
            allow_untested,
        },
    )
}

pub(crate) fn one_experimental(
    kind: DesktopHarnessKind,
) -> Result<DesktopCompatibilityEntry, DesktopCompatibilityError> {
    desktop_compatibility(kind)
}

async fn all_harnesses() -> Vec<HarnessDiscovery> {
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
    F: Fn(HarnessKind) -> Result<DiscoveryReport, DiscoveryError> + Send + Sync + 'static,
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

async fn provider() -> ProviderDiscovery {
    let config = match resolve_existing_config(None) {
        Ok(Some(config)) => config,
        Ok(None) => return ProviderDiscovery::NotConfigured,
        Err(error) => return ProviderDiscovery::Invalid(error.code()),
    };

    match tokio::time::timeout(MODEL_DISCOVERY_TIMEOUT, discover_models(&config)).await {
        Ok(Ok(models)) => ProviderDiscovery::Models(models),
        Ok(Err(PersistenceError::NoModels)) => ProviderDiscovery::NoModels,
        Ok(Err(PersistenceError::ModelDiscoveryStatus(status))) => {
            ProviderDiscovery::Status(status)
        }
        Ok(Err(PersistenceError::ParseModels(_))) => ProviderDiscovery::InvalidResponse,
        Ok(Err(error)) => ProviderDiscovery::Unavailable(error.code()),
        Err(_) => ProviderDiscovery::Timeout,
    }
}

fn integrations() -> IntegrationDiscovery {
    let configuration_manager = match ConfigurationManager::from_environment() {
        Ok(manager) => manager,
        Err(error) => {
            return IntegrationDiscovery::Failed {
                subject: "Configuration state",
                status: "unavailable",
                code: error.code(),
            };
        }
    };
    let manager = match PersistenceManager::from_environment() {
        Ok(manager) => manager,
        Err(error) => {
            return IntegrationDiscovery::Failed {
                subject: "Integration state",
                status: "unavailable",
                code: error.code(),
            };
        }
    };
    let integrations = match manager.configured_integrations() {
        Ok(integrations) => integrations,
        Err(error) => {
            return IntegrationDiscovery::Failed {
                subject: "Integration state",
                status: "unreadable",
                code: error.code(),
            };
        }
    };
    let native = match configuration_manager.configured_harnesses() {
        Ok(configurations) => configurations,
        Err(error) => {
            return IntegrationDiscovery::Failed {
                subject: "Configuration state",
                status: "unreadable",
                code: error.code(),
            };
        }
    };
    let pen_configured = match pen_desktop::persistent_configuration_exists() {
        Ok(configured) => configured,
        Err(error) => {
            return IntegrationDiscovery::Failed {
                subject: "Pen Desktop configuration state",
                status: "unreadable",
                code: error.code(),
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
    if pen_configured {
        reports.insert(
            "pen-desktop".to_owned(),
            pen_desktop::persistent_configuration_active().unwrap_or(false),
        );
    }
    IntegrationDiscovery::Configured(
        reports
            .into_iter()
            .map(|(id, active)| IntegrationStatus { id, active })
            .collect(),
    )
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

fn telemetry() -> TelemetryDiscovery {
    match TelemetrySettingsStore::from_environment().and_then(|store| store.load()) {
        Ok(settings) => TelemetryDiscovery::State(settings.enabled()),
        Err(_) => TelemetryDiscovery::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Barrier, Mutex};

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
}
