use super::support::{
    assert_direct_secret, context, plan, with_search_block, without_search_block,
};
use nan_harness_adapters::OpenClawAdapter;
use nan_harness_core::HarnessKind;
use nan_harness_core::MediaSelection;
use nan_harness_core::launch_plan::{
    BRIDGE_BASE_URL_PLACEHOLDER, MEDIA_CREDENTIAL_ENVIRONMENT, MEDIA_PROVIDER_BASE_URL_PLACEHOLDER,
    OPENCLAW_MODEL_ALIASES_PLACEHOLDER, OPENCLAW_MODEL_CATALOG_PLACEHOLDER,
};

#[test]
fn openclaw_merges_user_configuration_without_persisting_the_nan_secret() {
    let plan = plan(
        &OpenClawAdapter,
        &context(HarnessKind::OpenClaw, Vec::new()),
    );
    let overlay = plan
        .configuration_overlays
        .first()
        .expect("OpenClaw overlay should exist");
    let config_file = overlay
        .files
        .iter()
        .find(|file| file.path == "nan-harness.json")
        .expect("nan-harness configuration should exist");
    let config: serde_json::Value =
        serde_json::from_str(&without_search_block(&config_file.content_template))
            .expect("OpenClaw configuration without search should be JSON");
    let search_rendered = with_search_block(&config_file.content_template);
    let search_config: serde_json::Value = serde_json::from_str(&search_rendered)
        .expect("OpenClaw configuration with search should be JSON");
    let search_plugin = overlay
        .files
        .iter()
        .find(|file| file.path == "plugins/nan-harness-search/index.js")
        .expect("OpenClaw search plugin should exist");

    assert_eq!(plan.process.arguments, ["chat"]);
    assert_eq!(overlay.source_path, "{runtime:user_home}/.openclaw");
    assert_eq!(config["$include"], "./openclaw.json");
    assert_eq!(
        config["models"]["providers"]["nan"]["apiKey"],
        serde_json::json!({
            "id": "NAN_API_KEY",
            "provider": "default",
            "source": "env"
        })
    );
    assert_eq!(
        config["agents"]["defaults"]["models"],
        OPENCLAW_MODEL_ALIASES_PLACEHOLDER
    );
    assert_eq!(
        config["models"]["providers"]["nan"]["models"],
        OPENCLAW_MODEL_CATALOG_PLACEHOLDER
    );
    assert_eq!(
        search_config["tools"]["web"]["search"]["provider"],
        "nan-harness"
    );
    assert_eq!(
        search_config["plugins"]["load"]["paths"][0],
        "{artifact:openclaw-config}/plugins/nan-harness-search"
    );
    assert!(
        search_plugin
            .content_template
            .contains("registerWebSearchProvider")
    );
    assert!(
        search_plugin
            .content_template
            .contains(BRIDGE_BASE_URL_PLACEHOLDER)
    );
    assert!(
        !overlay
            .files
            .iter()
            .any(|file| file.content_template.contains("nan-secret-value"))
    );
    assert_direct_secret(&plan, "NAN_API_KEY");
}

#[test]
fn openclaw_media_plugin_registers_speech_audio_and_image_contracts() {
    let mut context = context(HarnessKind::OpenClaw, Vec::new());
    context.media = MediaSelection::all();
    let plan = plan(&OpenClawAdapter, &context);
    let overlay = plan
        .configuration_overlays
        .first()
        .expect("OpenClaw overlay should exist");
    let config_file = overlay
        .files
        .iter()
        .find(|file| file.path == "nan-harness.json")
        .expect("OpenClaw configuration should exist");
    let rendered_config = without_search_block(&config_file.content_template)
        .replace("{NAN_SEARCH_BLOCK_BEGIN}", "")
        .replace("{NAN_SEARCH_BLOCK_END}", "");
    let config: serde_json::Value = serde_json::from_str(&rendered_config)
        .expect("OpenClaw media configuration should be JSON");
    let search_config: serde_json::Value =
        serde_json::from_str(&with_search_block(&config_file.content_template))
            .expect("OpenClaw media configuration with search should be JSON");
    assert_eq!(config["tts"]["provider"], "nan-harness");
    assert_eq!(config["tools"]["media"]["models"][0]["model"], "whisper-1");
    assert_eq!(
        config["agents"]["defaults"]["mediaModels"]["image"]["primary"],
        "nan-harness/flux-2-klein"
    );
    assert!(
        search_config["plugins"]["load"]["paths"]
            .as_array()
            .is_some_and(|paths| {
                paths
                    .iter()
                    .any(|path| path == "{artifact:openclaw-config}/plugins/nan-harness-search")
                    && paths
                        .iter()
                        .any(|path| path == "{artifact:openclaw-config}/plugins/nan-harness-media")
            })
    );
    assert_eq!(
        search_config["tools"]["web"]["search"]["provider"],
        "nan-harness"
    );
    let plugin = overlay
        .files
        .iter()
        .find(|file| file.path == "plugins/nan-harness-media/index.js")
        .expect("OpenClaw media plugin should exist");
    assert!(plugin.content_template.contains("registerSpeechProvider"));
    assert!(
        plugin
            .content_template
            .contains("registerMediaUnderstandingProvider")
    );
    assert!(
        plugin
            .content_template
            .contains("registerImageGenerationProvider")
    );
    assert!(plugin.content_template.contains("NAN_MEDIA_API_KEY"));
    assert_eq!(
        plan.environment
            .secrets
            .get(MEDIA_CREDENTIAL_ENVIRONMENT)
            .map(nan_harness_core::SecretRef::as_str),
        Some("nan_api_key")
    );
    assert!(
        plugin
            .content_template
            .contains(MEDIA_PROVIDER_BASE_URL_PLACEHOLDER)
    );
    assert!(
        !plugin
            .content_template
            .contains("{PROVIDER_BASE_URL_PLACEHOLDER}")
    );
}

#[test]
fn openclaw_scopes_media_models_to_audio_only_when_transcription_is_enabled() {
    for tts in [false, true] {
        for stt in [false, true] {
            for image in [false, true] {
                let mut context = context(HarnessKind::OpenClaw, Vec::new());
                context.media = MediaSelection {
                    tts,
                    stt,
                    image,
                    image_model: None,
                };
                let plan = plan(&OpenClawAdapter, &context);
                let config_file = plan.configuration_overlays[0]
                    .files
                    .iter()
                    .find(|file| file.path == "nan-harness.json")
                    .expect("OpenClaw configuration should exist");
                for rendered in [
                    without_search_block(&config_file.content_template),
                    with_search_block(&config_file.content_template),
                ] {
                    let config: serde_json::Value = serde_json::from_str(&rendered)
                        .expect("OpenClaw configuration should be JSON");
                    if stt {
                        assert_eq!(
                            config["tools"]["media"],
                            serde_json::json!({
                                "audio": {"enabled": true},
                                "models": [{
                                    "type": "provider",
                                    "provider": "nan-harness",
                                    "model": "whisper-1",
                                    "capabilities": ["audio"]
                                }]
                            })
                        );
                    } else {
                        assert!(config["tools"].get("media").is_none());
                    }
                }
            }
        }
    }
}
