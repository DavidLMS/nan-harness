//! Shared conformance contract constants.

pub const CONFORMANCE_SCHEMA_VERSION: u8 = 2;
pub(crate) const LEGACY_CONFORMANCE_SCHEMA_VERSION: u8 = 1;
pub const TEST_CREDENTIAL: &str = "nan-harness-conformance-test-credential";
pub const INVENTORY_MARKER: &str = "NAN_HARNESS_CONFORMANCE_INVENTORY_OK";
pub const SENTINEL_MARKER: &str = "NAN_HARNESS_CONFORMANCE_SENTINEL_OK";
pub const ROUND_TRIP_MARKER: &str = "NAN_HARNESS_CONFORMANCE_ROUND_TRIP_OK";
pub const EXTERNAL_MARKER: &str = "NAN_HARNESS_CONFORMANCE_EXTERNAL_OK";

pub(crate) const MAX_DURATION_MILLISECONDS: u64 = 86_400_000;
pub(crate) const MAX_REPORT_SCENARIOS: usize = 4;
pub(crate) const MAX_REPORT_CHECKS: usize = 8;
pub(crate) const MAX_REPORT_OBSERVATIONS: usize = 1;
pub(crate) const MAX_REPORT_NAME_BYTES: usize = 64;
pub(crate) const PUBLISHED_SCENARIO_NAMES: [&str; 4] = [
    "inventory",
    "tool-round-trip",
    "sentinel",
    "external-prerequisite",
];
pub(crate) const WRAPPER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(90);
pub(crate) const KIMI_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(40);
pub(crate) const PROVIDER_CLEANUP_MARGIN: std::time::Duration = std::time::Duration::from_secs(2);
pub(crate) const PRIME_STATUS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
pub(crate) const PRIME_TERM_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
pub(crate) const PRIME_KILL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

pub(crate) const HERMES_OPTIONAL_CREDENTIALS_CLEARED: &[(&str, &str)] = &[
    ("BFL_API_KEY", ""),
    ("ELEVENLABS_API_KEY", ""),
    ("FAL_KEY", ""),
    ("OPENAI_API_KEY", ""),
    ("XAI_API_KEY", ""),
];

pub(crate) const OPENCLAW_MEDIA_CREDENTIALS_CLEARED: &[(&str, &str)] = &[
    ("AZURE_OPENAI_API_KEY", ""),
    ("BFL_API_KEY", ""),
    ("DEEPINFRA_API_KEY", ""),
    ("FAL_KEY", ""),
    ("GEMINI_API_KEY", ""),
    ("GOOGLE_API_KEY", ""),
    ("MINIMAX_API_KEY", ""),
    ("OPENAI_API_KEY", ""),
    ("OPENROUTER_API_KEY", ""),
    ("VYDRA_API_KEY", ""),
    ("XAI_API_KEY", ""),
];
