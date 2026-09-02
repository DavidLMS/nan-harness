mod arguments;
mod constants;
mod helpers;
mod inventory;
mod prime_cleanup;
mod registry;
mod report;
mod runner;

pub use constants::{
    CONFORMANCE_SCHEMA_VERSION, EXTERNAL_MARKER, INVENTORY_MARKER, ROUND_TRIP_MARKER,
    SENTINEL_MARKER, TEST_CREDENTIAL,
};
pub use helpers::{
    assert_file, assert_inventory, assert_success, call, tool_names, tool_result,
    tool_result_failed, write_fixture,
};
pub use registry::{
    HarnessRegistration, RegistryError, conformance_command, harness_registration,
    harness_registry, validate_harness_registry,
};
pub use report::{
    ConformanceCheck, ConformanceObservation, ConformanceObservationKind, ConformanceOutcome,
    ConformanceReport, ConformanceScenario, ConformanceStatus, ReportShapeError,
};
pub use runner::{ConformanceError, PublishedConformanceRunner};

#[cfg(test)]
#[path = "conformance/tests.rs"]
mod tests;

#[cfg(test)]
pub(crate) use crate::manifest::embedded_manifest;
#[cfg(test)]
pub(crate) use crate::scripted_provider::ScriptedToolCall;
#[cfg(test)]
pub(crate) use arguments::{RunKind, headless_arguments};
#[cfg(test)]
pub(crate) use helpers::scenario;
#[cfg(test)]
pub(crate) use inventory::{inventory_drift_fingerprint, inventory_matches, round_trip_probe};
#[cfg(all(test, unix))]
pub(crate) use prime_cleanup::signal_prime_targets_now;
#[cfg(test)]
pub(crate) use prime_cleanup::{
    PrimeCleanupTargets, owned_prime_pids_from_status, prime_status_path,
};
