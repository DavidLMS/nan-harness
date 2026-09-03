use crate::temporary::{TemporaryError, TemporaryWorkspace};
use nan_harness_core::{
    CodingModelProfile, LaunchPlan, SecretError, SecretRef, SecretStore, SecretValue,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod catalogs;
mod pipeline;
mod values;

#[cfg(test)]
use catalogs::{
    aider_model_settings, cline_model_catalog, deepseek_model_catalog, goose_model_catalog,
    hermes_model_catalog, kimi_code_model_catalog, openclaw_model_catalog, opencode_model_catalog,
    pi_model_catalog, qwen_code_model_catalog, render_model_catalogs,
};
#[cfg(test)]
use values::{join_goose_config_paths, render_nan_search_blocks};

pub(crate) struct BridgePreparation {
    pub(crate) base_url: String,
    pub(crate) client_base_url: Option<String>,
    pub(crate) chat_url: Option<String>,
    pub(crate) session_token_ref: SecretRef,
    pub(crate) session_token: Arc<SecretValue>,
    pub(crate) claude_available_models: Vec<String>,
    pub(crate) codex_model_catalog: Option<String>,
    pub(crate) web_search_enabled: bool,
}

pub(crate) struct PreparedLaunch {
    arguments: Vec<String>,
    public_environment: BTreeMap<String, String>,
    runtime_secrets: BTreeMap<SecretRef, Arc<SecretValue>>,
    workspace: TemporaryWorkspace,
}

#[derive(Debug, thiserror::Error)]
pub enum PreparedError {
    #[error(transparent)]
    Temporary(#[from] TemporaryError),
    #[error("launch references unknown temporary artifact '{0}'")]
    UnknownArtifact(String),
    #[error("launch contains unresolved placeholder '{0}'")]
    UnresolvedPlaceholder(String),
    #[error("could not materialize the live NaN model catalog: {0}")]
    ModelCatalog(String),
    #[error("NH-PREPARED-ENV-001")]
    InvalidEnvironmentPathList,
}

impl PreparedLaunch {
    pub(crate) fn prepare(
        plan: &LaunchPlan,
        provider_base_url: &str,
        bridge: Option<BridgePreparation>,
        model_catalog: Option<&[CodingModelProfile]>,
    ) -> Result<Self, PreparedError> {
        pipeline::prepare(plan, provider_base_url, bridge, model_catalog)
    }

    pub(crate) fn arguments(&self) -> &[String] {
        &self.arguments
    }

    pub(crate) fn public_environment(&self) -> &BTreeMap<String, String> {
        &self.public_environment
    }

    pub(crate) fn with_secret<T>(
        &self,
        provider_secrets: &SecretStore,
        reference: &SecretRef,
        operation: impl FnOnce(&str) -> T,
    ) -> Result<T, SecretError> {
        if let Some(value) = self.runtime_secrets.get(reference) {
            Ok(value.with_secret(operation))
        } else {
            provider_secrets.with_secret(reference, operation)
        }
    }

    pub(crate) fn temporary_root(&self, has_artifacts: bool) -> Option<PathBuf> {
        has_artifacts.then(|| self.workspace.root().to_path_buf())
    }

    pub(crate) fn artifact_path(&self, artifact_id: &str) -> Option<PathBuf> {
        self.workspace.path(artifact_id).map(Path::to_path_buf)
    }
}

pub(crate) use pipeline::requires_model_catalog;

#[cfg(test)]
mod tests {
    use super::{
        BridgePreparation, PreparedLaunch, join_goose_config_paths, render_nan_search_blocks,
        requires_model_catalog,
    };
    use nan_harness_core::launch_plan::{
        ArtifactLifecycle, CLAUDE_MODEL_PICKER_PLACEHOLDER, CLAUDE_MODEL_PRESENTATIONS_PLACEHOLDER,
        ConfigurationOverlay, LaunchPlan, OPENCODE_MODEL_CATALOG_PLACEHOLDER, OverlayFile,
        OverlayFilePolicy, PI_MODEL_CATALOG_PLACEHOLDER, SELECTED_MODEL_CAPABILITIES_PLACEHOLDER,
        TemporaryArtifactMode,
    };
    use nan_harness_core::model::ReasoningPolicy;
    use nan_harness_core::{
        CodingModelProfile, ProfileSource, SecretRef, SecretValue, coding_model_profile,
    };
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn model(id: &str) -> CodingModelProfile {
        CodingModelProfile {
            id: id.to_owned(),
            display_name: format!("NaN · {id}"),
            description: "test model".to_owned(),
            context_window: 262_144,
            max_output_tokens: 32_768,
            image_input: false,
            reasoning: ReasoningPolicy::Unknown,
            source: ProfileSource::Generic,
        }
    }

    #[test]
    fn search_blocks_render_atomically() {
        let template = "before{runtime:nan_search:begin},search{runtime:nan_search:end}after";

        assert_eq!(
            render_nan_search_blocks(template, true).expect("enabled block"),
            "before,searchafter"
        );
        assert_eq!(
            render_nan_search_blocks(template, false).expect("disabled block"),
            "beforeafter"
        );
        assert!(render_nan_search_blocks("{runtime:nan_search:begin}open", true).is_err());
        assert!(
            render_nan_search_blocks(
                "{runtime:nan_search:begin}{runtime:nan_search:begin}nested{runtime:nan_search:end}{runtime:nan_search:end}",
                true,
            )
            .is_err()
        );
    }

    #[test]
    fn goose_search_config_preserves_existing_additional_layers() {
        let existing =
            std::env::join_paths(["first.yaml", "second.yaml"]).expect("fixture paths should join");
        let joined = join_goose_config_paths(Some(existing.as_os_str()), "nan-search.yaml")
            .expect("Goose config paths should join");
        let paths = std::env::split_paths(&joined).collect::<Vec<_>>();

        assert_eq!(
            paths,
            ["first.yaml", "second.yaml", "nan-search.yaml"]
                .into_iter()
                .map(PathBuf::from)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn prepared_search_overlay_resolves_to_valid_enabled_or_disabled_json() {
        for enabled in [false, true] {
            let source_home = tempfile::tempdir().expect("empty search source");
            let source =
                include_str!("../../nan-harness-core/tests/fixtures/launch-plan.direct.json");
            let mut plan: LaunchPlan = serde_json::from_str(source).expect("fixture should parse");
            plan.configuration_overlays.push(ConfigurationOverlay {
                id: "search-home".to_owned(),
                path_hint: "search-home".to_owned(),
                source_path: source_home.path().to_string_lossy().into_owned(),
                files: vec![OverlayFile {
                    path: "mcp.json".to_owned(),
                    mode: TemporaryArtifactMode::OwnerFile,
                    content_template: concat!(
                        "{{runtime:nan_search:begin}\"mcpServers\":{\"nan-search\":{",
                        "\"command\":\"nan-harness\",\"args\":[\"__search-mcp\",",
                        "\"--endpoint\",\"{runtime:bridge_base_url}/v1/search\",",
                        "\"--token-env\",\"NAN_API_KEY\"]}}{runtime:nan_search:end}}"
                    )
                    .to_owned(),
                    policy: OverlayFilePolicy::MergeJson,
                }],
                lifecycle: ArtifactLifecycle::Launch,
            });
            let token_ref = SecretRef::new("nan_api_key").expect("secret reference");
            let prepared = PreparedLaunch::prepare(
                &plan,
                "https://api.nan.builders/v1",
                Some(BridgePreparation {
                    base_url: "http://127.0.0.1:3210".to_owned(),
                    client_base_url: Some("http://127.0.0.1:3210/v1".to_owned()),
                    chat_url: None,
                    session_token_ref: token_ref,
                    session_token: Arc::new(
                        SecretValue::new("local-session-token").expect("session token"),
                    ),
                    claude_available_models: Vec::new(),
                    codex_model_catalog: None,
                    web_search_enabled: enabled,
                }),
                None,
            )
            .expect("search overlay should prepare");
            let path = prepared
                .artifact_path("search-home")
                .expect("search overlay path")
                .join("mcp.json");
            let value: serde_json::Value = serde_json::from_str(
                &fs::read_to_string(path).expect("rendered search overlay should be readable"),
            )
            .expect("rendered search overlay should be JSON");
            if enabled {
                let server = &value["mcpServers"]["nan-search"];
                assert_eq!(server["command"], "nan-harness");
                assert_eq!(server["args"][2], "http://127.0.0.1:3210/v1/search");
            } else {
                assert!(value["mcpServers"].get("nan-search").is_none());
            }
        }
    }

    fn known_models() -> Vec<CodingModelProfile> {
        [
            "qwen3.6",
            "deepseek-v4-flash",
            "mimo-v2.5",
            "gemma4",
            "glm5.2",
        ]
        .into_iter()
        .map(|id| coding_model_profile(id).expect("known coding model"))
        .collect()
    }

    fn claude_settings_template() -> String {
        claude_settings_template_for("anthropic/nan/qwen3.6")
    }

    fn claude_settings_template_for(model: &str) -> String {
        serde_json::json!({
            "availableModels": "{runtime:claude_available_models}",
            "model": model,
            "env": {
                "ANTHROPIC_MODEL": model,
                CLAUDE_MODEL_PRESENTATIONS_PLACEHOLDER: ""
            }
        })
        .to_string()
    }

    fn claude_model_picker_settings_template_for(model: &str) -> String {
        serde_json::json!({
            "availableModels": "{runtime:claude_available_models}",
            "model": model,
            "modelPicker": CLAUDE_MODEL_PICKER_PLACEHOLDER,
            "env": {
                "ANTHROPIC_MODEL": model,
                CLAUDE_MODEL_PRESENTATIONS_PLACEHOLDER: ""
            }
        })
        .to_string()
    }

    #[test]
    fn claude_picker_slots_come_from_the_discovered_catalog() {
        let models = [
            coding_model_profile("qwen3.6").expect("known coding model"),
            coding_model_profile("mimo-v2.5").expect("known coding model"),
        ];
        let rendered = super::render_model_catalogs(
            &claude_settings_template(),
            "https://nan.invalid/v1",
            "qwen3.6",
            Some(&models),
        )
        .expect("Claude settings should render");
        let settings: serde_json::Value =
            serde_json::from_str(&rendered).expect("rendered settings should be valid JSON");
        let environment = settings["env"]
            .as_object()
            .expect("settings should keep an env object");

        assert!(!environment.contains_key(CLAUDE_MODEL_PRESENTATIONS_PLACEHOLDER));
        assert_eq!(environment["ANTHROPIC_MODEL"], "anthropic/nan/qwen3.6");
        assert_eq!(
            environment["ANTHROPIC_DEFAULT_OPUS_MODEL"],
            "anthropic/nan/qwen3.6"
        );
        assert_eq!(
            environment["ANTHROPIC_DEFAULT_OPUS_MODEL_NAME"],
            "NaN · Qwen 3.6"
        );
        assert_eq!(
            environment["ANTHROPIC_DEFAULT_SONNET_MODEL"],
            "anthropic/nan/mimo-v2.5"
        );
        assert!(
            !environment.contains_key("ANTHROPIC_DEFAULT_HAIKU_MODEL"),
            "slots without a discovered model must stay unset"
        );
        assert!(!environment.contains_key("ANTHROPIC_CUSTOM_MODEL_OPTION"));
        assert!(
            !rendered.contains("deepseek"),
            "a model missing from discovery must never reach the picker"
        );
    }

    #[test]
    fn claude_picker_puts_the_selected_model_first() {
        let models = known_models();
        let rendered = super::render_model_catalogs(
            &claude_settings_template(),
            "https://nan.invalid/v1",
            "gemma4",
            Some(&models),
        )
        .expect("Claude settings should render");
        let settings: serde_json::Value =
            serde_json::from_str(&rendered).expect("rendered settings should be valid JSON");
        let environment = &settings["env"];

        assert_eq!(
            environment["ANTHROPIC_DEFAULT_OPUS_MODEL"],
            "anthropic/nan/gemma4"
        );
        let slots = [
            "ANTHROPIC_DEFAULT_OPUS_MODEL",
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            "ANTHROPIC_CUSTOM_MODEL_OPTION",
        ]
        .map(|slot| environment[slot].as_str().expect("slot should be filled"));
        assert_eq!(
            slots.iter().collect::<BTreeSet<_>>().len(),
            slots.len(),
            "picker slots must not repeat a model"
        );
    }

    #[test]
    fn claude_curated_picker_prioritizes_glm_over_gemma() {
        let rendered = super::render_model_catalogs(
            &claude_settings_template(),
            "https://nan.invalid/v1",
            "qwen3.6",
            Some(&known_models()),
        )
        .expect("Claude settings should render");
        let settings: serde_json::Value =
            serde_json::from_str(&rendered).expect("rendered settings should be valid JSON");
        let environment = &settings["env"];

        assert_eq!(
            environment["ANTHROPIC_CUSTOM_MODEL_OPTION"],
            "anthropic/nan/glm5.2"
        );
        assert_eq!(
            environment["ANTHROPIC_CUSTOM_MODEL_OPTION_NAME"],
            "NaN · GLM 5.2"
        );
        assert!(
            !environment
                .as_object()
                .expect("environment object")
                .values()
                .any(|value| value.as_str() == Some("anthropic/nan/gemma4")),
            "Gemma must yield the fourth curated slot to GLM"
        );
    }

    #[test]
    fn claude_gateway_mode_preserves_qwen_auto_alias() {
        let mut models = known_models();
        models.push(model("future-model"));
        let rendered = super::render_model_catalogs(
            &claude_settings_template_for("opus"),
            "https://nan.invalid/v1",
            "qwen3.6",
            Some(&models),
        )
        .expect("Claude settings should render");
        let settings: serde_json::Value =
            serde_json::from_str(&rendered).expect("rendered settings should be valid JSON");
        let environment = settings["env"]
            .as_object()
            .expect("settings should keep an env object");

        assert_eq!(settings["model"], "opus");
        assert_eq!(environment["ANTHROPIC_MODEL"], "opus");
        assert_eq!(
            environment["ANTHROPIC_DEFAULT_OPUS_MODEL"],
            "anthropic/nan/qwen3.6"
        );
        assert_eq!(
            environment["ANTHROPIC_DEFAULT_OPUS_MODEL_NAME"],
            "NaN · Qwen 3.6"
        );
        for absent in [
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            "ANTHROPIC_CUSTOM_MODEL_OPTION",
        ] {
            assert!(
                !environment.contains_key(absent),
                "gateway mode must not consume the {absent} presentation slot"
            );
        }
        assert!(!environment.contains_key(CLAUDE_MODEL_PRESENTATIONS_PLACEHOLDER));
    }

    #[test]
    fn claude_model_picker_exposes_standard_and_eligible_1m_variants() {
        let models = [
            coding_model_profile("qwen3.6").expect("known coding model"),
            coding_model_profile("deepseek-v4-flash").expect("known coding model"),
            coding_model_profile("glm5.2").expect("known coding model"),
            model("future-model"),
        ];
        let rendered = super::render_model_catalogs(
            &claude_model_picker_settings_template_for("anthropic/nan/deepseek-v4-flash"),
            "https://nan.invalid/v1",
            "deepseek-v4-flash",
            Some(&models),
        )
        .expect("Claude modelPicker settings should render");
        let settings: serde_json::Value =
            serde_json::from_str(&rendered).expect("rendered settings should be valid JSON");

        assert_eq!(settings["model"], "anthropic/nan/deepseek-v4-flash");
        assert_eq!(settings["modelPicker"]["replaceBuiltInOptions"], true);
        assert_eq!(
            settings["modelPicker"]["options"],
            serde_json::json!([
                {
                    "model": "opus",
                    "label": "NaN · Qwen 3.6",
                    "description": "Standard context · 256K"
                },
                {
                    "model": "anthropic/nan/deepseek-v4-flash",
                    "label": "NaN · DeepSeek V4 Flash",
                    "description": "Standard context · 256K"
                },
                {
                    "model": "anthropic/nan/deepseek-v4-flash[1m]",
                    "label": "NaN · DeepSeek V4 Flash (1M)",
                    "description": "Extended context · 1M"
                },
                {
                    "model": "anthropic/nan/glm5.2",
                    "label": "NaN · GLM 5.2",
                    "description": "Standard context · 256K"
                },
                {
                    "model": "anthropic/nan/future-model",
                    "label": "NaN · future-model",
                    "description": "Standard context · 256K"
                }
            ])
        );
        let environment = settings["env"].as_object().expect("environment object");
        assert_eq!(
            environment["ANTHROPIC_DEFAULT_OPUS_MODEL"],
            "anthropic/nan/qwen3.6"
        );
        assert!(!environment.contains_key("ANTHROPIC_DEFAULT_SONNET_MODEL"));
        assert!(!rendered.contains(CLAUDE_MODEL_PICKER_PLACEHOLDER));
        assert!(!rendered.contains(CLAUDE_MODEL_PRESENTATIONS_PLACEHOLDER));
    }

    #[test]
    fn new_nan_models_keep_claude_in_gateway_discovery_mode() {
        let models = [
            coding_model_profile("qwen3.6").expect("known coding model"),
            coding_model_profile("qwen3.8-flash").expect("known coding model"),
            coding_model_profile("glm5.3-flash").expect("known coding model"),
            coding_model_profile("glm5.3").expect("known coding model"),
        ];
        let rendered = super::render_model_catalogs(
            &claude_settings_template(),
            "https://nan.invalid/v1",
            "qwen3.6",
            Some(&models),
        )
        .expect("Claude settings should render");
        let settings: serde_json::Value =
            serde_json::from_str(&rendered).expect("rendered settings should be valid JSON");
        let environment = settings["env"].as_object().expect("environment object");

        assert_eq!(
            environment["ANTHROPIC_DEFAULT_OPUS_MODEL"],
            "anthropic/nan/qwen3.6"
        );
        for absent in [
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            "ANTHROPIC_CUSTOM_MODEL_OPTION",
        ] {
            assert!(!environment.contains_key(absent));
        }
        assert!(!rendered.contains("qwen3.8-flash"));
        assert!(!rendered.contains("glm5.3-flash"));
    }

    #[test]
    fn model_catalog_rendering_deduplicates_ids_stably() {
        let models = [model("qwen3.6"), model("qwen3.6"), model("mimo-v2.5")];
        let template = format!(
            r#"{{"opencode":{OPENCODE_MODEL_CATALOG_PLACEHOLDER},"pi":{PI_MODEL_CATALOG_PLACEHOLDER}}}"#
        );
        let rendered = super::render_model_catalogs(
            &template,
            "https://api.nan.builders/v1",
            "qwen3.6",
            Some(&models),
        )
        .expect("catalogs should render");
        let value: serde_json::Value =
            serde_json::from_str(&rendered).expect("rendered catalogs should be JSON");

        assert_eq!(value["opencode"].as_object().expect("map").len(), 2);
        assert_eq!(value["pi"].as_object().expect("map").len(), 2);
        assert_eq!(
            value["opencode"]
                .as_object()
                .expect("map")
                .keys()
                .collect::<Vec<_>>(),
            &[&"mimo-v2.5".to_owned(), &"qwen3.6".to_owned()]
        );
    }

    #[test]
    fn catalog_placeholders_in_arguments_trigger_live_discovery() {
        let source = include_str!("../../nan-harness-core/tests/fixtures/launch-plan.direct.json");
        let mut plan: LaunchPlan = serde_json::from_str(source).expect("fixture should parse");
        plan.process.arguments = vec![OPENCODE_MODEL_CATALOG_PLACEHOLDER.to_owned()];

        assert!(requires_model_catalog(&plan));
    }

    #[test]
    fn model_catalog_placeholders_in_arguments_are_rendered() {
        let source = include_str!("../../nan-harness-core/tests/fixtures/launch-plan.direct.json");
        let mut plan: LaunchPlan = serde_json::from_str(source).expect("fixture should parse");
        plan.process.arguments = vec![OPENCODE_MODEL_CATALOG_PLACEHOLDER.to_owned()];
        let models = [model("qwen3.6")];

        let prepared =
            PreparedLaunch::prepare(&plan, "https://api.nan.builders/v1", None, Some(&models))
                .expect("argument catalog should render");

        assert!(prepared.arguments()[0].contains("qwen3.6"));
        assert!(!prepared.arguments()[0].contains(OPENCODE_MODEL_CATALOG_PLACEHOLDER));
    }

    #[test]
    fn native_reasoning_catalogs_are_model_aware() {
        let mut models = known_models();
        models.extend([
            coding_model_profile("qwen3.8-flash").expect("known coding model"),
            coding_model_profile("glm5.3-flash").expect("known coding model"),
            coding_model_profile("glm5.3").expect("known coding model"),
        ]);
        let opencode = super::opencode_model_catalog(&models);
        assert_eq!(opencode["qwen3.6"]["reasoning"], true);
        assert_eq!(opencode["qwen3.6"]["defaultVariant"], "thinking");
        assert_eq!(opencode["gemma4"]["defaultVariant"], "no-thinking");
        assert_eq!(
            opencode["deepseek-v4-flash"]["variants"]["high"]["reasoningEffort"],
            "high"
        );
        assert_eq!(opencode["glm5.2"]["reasoning"], true);
        assert_eq!(
            opencode["glm5.2"]["variants"]["high"]["reasoningEffort"],
            "high"
        );
        assert_eq!(opencode["qwen3.8-flash"]["reasoning"], true);
        assert!(opencode["qwen3.8-flash"].get("defaultVariant").is_none());
        assert_eq!(opencode["glm5.3-flash"]["reasoning"], true);
        assert_eq!(
            opencode["glm5.3-flash"]["variants"]["high"]["reasoningEffort"],
            "high"
        );
        assert_eq!(opencode["glm5.3"]["reasoning"], true);
        assert_eq!(
            opencode["glm5.3"]["variants"]["high"]["reasoningEffort"],
            "high"
        );

        let qwen = super::qwen_code_model_catalog(&models, "https://nan.invalid/v1");
        let by_id = |id: &str| {
            qwen.as_array()
                .expect("catalog")
                .iter()
                .find(|entry| entry["id"] == id)
                .expect("model")
        };
        assert_eq!(
            by_id("qwen3.6")["generationConfig"]["samplingParams"]["enable_thinking"],
            true
        );
        assert_eq!(
            by_id("gemma4")["generationConfig"]["samplingParams"]["enable_thinking"],
            false
        );
        assert!(
            by_id("deepseek-v4-flash")["generationConfig"]["samplingParams"]
                .get("reasoning_effort")
                .is_none()
        );
        assert_eq!(
            by_id("deepseek-v4-flash")["generationConfig"]["modalities"]["image"],
            true
        );
        assert_eq!(
            by_id("qwen3.6")["generationConfig"]["samplingParams"]["max_tokens"],
            65_536
        );
        assert_eq!(
            by_id("qwen3.8-flash")["generationConfig"]["contextWindowSize"],
            1_000_000
        );
        assert_eq!(
            by_id("qwen3.8-flash")["generationConfig"]["modalities"]["image"],
            true
        );
        assert!(
            by_id("qwen3.8-flash")["generationConfig"]["samplingParams"]
                .get("enable_thinking")
                .is_none()
        );
        assert_eq!(
            by_id("glm5.3-flash")["generationConfig"]["modalities"]["image"],
            true
        );
        assert_eq!(
            by_id("glm5.3")["generationConfig"]["contextWindowSize"],
            1_000_000
        );
        assert_eq!(
            by_id("glm5.3")["generationConfig"]["modalities"]["image"],
            true
        );
    }

    #[test]
    fn metadata_and_capabilities_do_not_claim_reasoning_for_every_model() {
        let models = known_models();
        let openclaw = super::openclaw_model_catalog(&models);
        let by_id = |id: &str| {
            openclaw
                .as_array()
                .expect("catalog")
                .iter()
                .find(|entry| entry["id"] == id)
                .expect("model")
        };
        assert_eq!(by_id("mimo-v2.5")["reasoning"], true);
        assert_eq!(by_id("glm5.2")["reasoning"], true);

        let selected = super::render_model_catalogs(
            SELECTED_MODEL_CAPABILITIES_PLACEHOLDER,
            "https://nan.invalid/v1",
            "glm5.2",
            Some(&models),
        )
        .expect("selected capabilities");
        assert_eq!(selected, "thinking");

        let kimi = super::kimi_code_model_catalog(&models, "qwen3.6").expect("Kimi catalog");
        assert!(kimi.contains("thinking"));
        let glm_section = kimi
            .split("[models.\"nan/glm5.2\"]")
            .nth(1)
            .expect("glm section");
        assert!(
            glm_section
                .lines()
                .take(8)
                .any(|line| line.contains("thinking"))
        );

        let pi = super::pi_model_catalog(&models);
        assert_eq!(pi["qwen3.6"]["reasoningPolicy"]["kind"], "toggle");
        assert_eq!(pi["glm5.2"]["reasoningPolicy"]["kind"], "effort");

        let cline = super::cline_model_catalog(&models);
        assert_eq!(cline["qwen3.6"]["reasoningControl"], "metadata-only");
        assert_eq!(cline["glm5.2"]["reasoningPolicy"]["kind"], "effort");

        let goose = super::goose_model_catalog(&models);
        assert!(
            goose
                .as_array()
                .expect("Goose catalog")
                .iter()
                .all(|entry| {
                    entry["reasoning_control"] == "passthrough"
                        && entry.get("reasoning_policy").is_some()
                })
        );

        let deepseek = super::deepseek_model_catalog(&models).expect("DeepSeek catalog");
        assert!(deepseek.contains("id: \"mimo-v2.5\""));
        assert!(deepseek.contains("reasoning: true"));
        let glm_section = deepseek
            .split("id: \"glm5.2\"")
            .nth(1)
            .expect("DeepSeek GLM section");
        assert!(glm_section.contains("reasoning: true"));

        let hermes = super::hermes_model_catalog(&models);
        assert!(
            hermes
                .as_array()
                .expect("Hermes IDs only")
                .iter()
                .all(serde_json::Value::is_string)
        );
    }

    #[test]
    fn aider_sets_reasoning_effort_for_effort_capable_models() {
        let mut models = known_models();
        models.extend([
            coding_model_profile("qwen3.8-flash").expect("known coding model"),
            coding_model_profile("glm5.3-flash").expect("known coding model"),
            coding_model_profile("glm5.3").expect("known coding model"),
        ]);
        let settings = super::aider_model_settings(&models);
        let by_name = |name: &str| {
            settings
                .as_array()
                .expect("settings")
                .iter()
                .find(|entry| entry["name"] == name)
                .expect("model")
        };
        assert_eq!(
            by_name("openai/deepseek-v4-flash")["reasoning_effort"],
            "medium"
        );
        assert_eq!(by_name("openai/glm5.2")["reasoning_effort"], "medium");
        assert_eq!(by_name("openai/glm5.3-flash")["reasoning_effort"], "medium");
        assert_eq!(by_name("openai/glm5.3")["reasoning_effort"], "medium");
        assert!(by_name("openai/qwen3.6").get("reasoning_effort").is_none());
        assert!(
            by_name("openai/mimo-v2.5")
                .get("reasoning_effort")
                .is_none()
        );
    }
}
