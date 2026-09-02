use nan_harness_core::{
    CodingModelProfile, DesktopHarnessKind, DesktopTransport, HarnessKind, ProfileSource,
    ReasoningPolicy,
};
use nan_harness_runtime::desktop_compatibility::DesktopCompatibilityEvidence;
use serde::Serialize;

pub(crate) const DOCTOR_SCHEMA_VERSION: u8 = 5;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum DiagnosticLevel {
    Ok,
    Warning,
    Info,
    Error,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HarnessDoctorReport {
    pub(crate) schema_version: u8,
    pub(crate) harness: HarnessKind,
    pub(crate) level: DiagnosticLevel,
    pub(crate) installed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) minimum_supported_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) last_compatible_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) compatible_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) last_live_verified_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) live_verified_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) compatibility: Option<&'static str>,
    pub(crate) warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error_code: Option<&'static str>,
    pub(crate) safe_to_share: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SystemDoctorReport {
    pub(crate) schema_version: u8,
    pub(crate) nan_harness_version: &'static str,
    pub(crate) platform: PlatformReport,
    pub(crate) provider: ProviderReport,
    pub(crate) harnesses: Vec<HarnessReport>,
    pub(crate) experimental_harnesses: Vec<ExperimentalHarnessReport>,
    pub(crate) managed_configurations: IntegrationSection,
    pub(crate) telemetry: TelemetryReport,
    pub(crate) safe_to_share: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExperimentalHarnessReport {
    pub(crate) id: DesktopHarnessKind,
    pub(crate) level: DiagnosticLevel,
    pub(crate) platform: String,
    pub(crate) available: bool,
    pub(crate) evidence: DesktopCompatibilityEvidence,
    pub(crate) transport: DesktopTransport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) minimum_supported_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) last_compatible_version: Option<String>,
    pub(crate) compatible_at: String,
    pub(crate) safe_to_share: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExperimentalHarnessDoctorReport {
    pub(crate) schema_version: u8,
    pub(crate) harness: DesktopHarnessKind,
    pub(crate) experimental: bool,
    pub(crate) level: DiagnosticLevel,
    pub(crate) platform: String,
    pub(crate) available: bool,
    pub(crate) evidence: DesktopCompatibilityEvidence,
    pub(crate) transport: DesktopTransport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) minimum_supported_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) last_compatible_version: Option<String>,
    pub(crate) compatible_at: String,
    pub(crate) safe_to_share: bool,
}

impl SystemDoctorReport {
    pub(crate) fn has_errors(&self) -> bool {
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
pub(crate) struct PlatformReport {
    pub(crate) operating_system: &'static str,
    pub(crate) architecture: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderReport {
    pub(crate) level: DiagnosticLevel,
    pub(crate) credential: &'static str,
    pub(crate) api: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) coding_model_count: Option<usize>,
    pub(crate) coding_models: Vec<CodingModelSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) http_status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error_code: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodingModelSummary {
    pub(crate) id: String,
    pub(crate) context_window: u64,
    pub(crate) max_output_tokens: u64,
    pub(crate) image_input: bool,
    pub(crate) reasoning: ReasoningPolicy,
    pub(crate) source: ProfileSource,
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
pub(crate) struct HarnessReport {
    pub(crate) id: HarnessKind,
    pub(crate) level: DiagnosticLevel,
    pub(crate) installed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) minimum_supported_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) last_compatible_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) compatible_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) last_live_verified_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) live_verified_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) compatibility: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error_code: Option<&'static str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IntegrationSection {
    pub(crate) level: DiagnosticLevel,
    pub(crate) integrations: Vec<IntegrationReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error_code: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IntegrationReport {
    pub(crate) id: String,
    pub(crate) active: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TelemetryReport {
    pub(crate) level: DiagnosticLevel,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error_code: Option<&'static str>,
}

#[derive(Debug)]
pub(crate) struct TextSystemReport {
    pub(crate) provider: ProviderTextReport,
    pub(crate) harnesses: Vec<HarnessTextReport>,
    pub(crate) experimental_harnesses: Vec<ExperimentalTextReport>,
    pub(crate) managed_configurations: ConfigurationTextReport,
    pub(crate) telemetry: TelemetryTextReport,
}

#[derive(Debug)]
pub(crate) enum ProviderTextReport {
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
pub(crate) struct HarnessTextReport {
    pub(crate) harness: HarnessKind,
    pub(crate) status: HarnessTextStatus,
}

#[derive(Debug)]
pub(crate) enum HarnessTextStatus {
    Installed {
        version: String,
        level: &'static str,
        label: &'static str,
    },
    NotInstalled,
    Failed(&'static str),
}

#[derive(Debug)]
pub(crate) enum ExperimentalTextReport {
    Available {
        harness: DesktopHarnessKind,
        platform: String,
        evidence: DesktopCompatibilityEvidence,
        transport: DesktopTransport,
    },
    Failed {
        harness: DesktopHarnessKind,
        error: String,
    },
}

#[derive(Debug)]
pub(crate) enum ConfigurationTextReport {
    NoneConfigured,
    Configured(Vec<IntegrationReport>),
    Failed {
        subject: &'static str,
        status: &'static str,
        code: &'static str,
    },
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum TelemetryTextReport {
    State(bool),
    Failed,
}

#[derive(Debug)]
pub(crate) struct HarnessDetails {
    pub(crate) harness: HarnessKind,
    pub(crate) executable: String,
    pub(crate) detected_version: String,
    pub(crate) minimum_supported_version: String,
    pub(crate) last_compatible_version: String,
    pub(crate) compatible_at: String,
    pub(crate) last_live_verified_version: Option<String>,
    pub(crate) live_verified_at: Option<String>,
    pub(crate) compatibility: &'static str,
    pub(crate) warnings: Vec<String>,
}

pub(crate) fn coding_model_summaries(
    mut models: Vec<CodingModelProfile>,
) -> Vec<CodingModelSummary> {
    models.sort_by(|left, right| left.id.cmp(&right.id));
    models.into_iter().map(CodingModelSummary::from).collect()
}

pub(crate) fn model_catalog_text(models: &[CodingModelProfile]) -> Option<(String, bool)> {
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

#[cfg(test)]
mod tests {
    use super::*;

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
