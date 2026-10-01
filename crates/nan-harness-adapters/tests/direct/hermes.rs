use super::support::{assert_direct_secret, context, plan, without_search_block};
use nan_harness_adapters::{HermesAdapter, hermes_command_provider_config};
use nan_harness_core::HarnessKind;
use nan_harness_core::MediaSelection;
use nan_harness_core::launch_plan::{
    BRIDGE_BASE_URL_PLACEHOLDER, HERMES_MODEL_CATALOG_PLACEHOLDER, MEDIA_CREDENTIAL_ENVIRONMENT,
    MEDIA_PROVIDER_BASE_URL_PLACEHOLDER, NAN_SEARCH_BLOCK_BEGIN, OverlayFilePolicy,
    PROVIDER_BASE_URL_PLACEHOLDER,
};

#[test]
fn hermes_loads_a_launch_scoped_nan_provider_without_hiding_user_state() {
    let plan = plan(
        &HermesAdapter,
        &context(HarnessKind::Hermes, vec!["--tui".to_owned()]),
    );

    assert_eq!(
        plan.process.arguments,
        ["--provider", "nan", "--model", "qwen3.6", "--tui"]
    );
    let overlay = plan
        .configuration_overlays
        .first()
        .expect("Hermes home overlay should exist");
    let plugin = overlay
        .files
        .iter()
        .find(|file| file.path.ends_with("__init__.py"))
        .expect("NaN provider plugin should exist");
    let search_provider = overlay
        .files
        .iter()
        .find(|file| file.path.ends_with("web/nan_harness/provider.py"))
        .expect("NaN search provider should exist");
    let search_config = overlay
        .files
        .iter()
        .find(|file| file.path == "config.yaml")
        .expect("Hermes search config should exist");
    assert_eq!(overlay.source_path, "{runtime:user_home}/.hermes");
    assert_eq!(
        plan.environment.public.get("HERMES_HOME"),
        Some(&"{artifact:hermes-home}".to_owned())
    );
    assert!(
        plugin
            .content_template
            .contains(PROVIDER_BASE_URL_PLACEHOLDER)
    );
    assert!(
        search_provider
            .content_template
            .contains(BRIDGE_BASE_URL_PLACEHOLDER)
    );
    assert!(search_provider.content_template.contains("maxResults"));
    assert!(
        search_config
            .content_template
            .contains(NAN_SEARCH_BLOCK_BEGIN)
    );
    assert_eq!(search_config.policy, OverlayFilePolicy::MergeYaml);
    assert!(
        plugin
            .content_template
            .contains(HERMES_MODEL_CATALOG_PLACEHOLDER)
    );
    assert!(plan.environment.remove.contains("OPENAI_BASE_URL"));
    assert_direct_secret(&plan, "NAN_API_KEY");
}

#[test]
fn hermes_media_overlay_contains_independent_native_providers() {
    let mut context = context(HarnessKind::Hermes, Vec::new());
    context.media = MediaSelection::all();
    let plan = plan(&HermesAdapter, &context);
    let overlay = plan
        .configuration_overlays
        .first()
        .expect("Hermes home overlay should exist");
    let config_template = overlay
        .files
        .iter()
        .find(|file| file.path == "config.yaml")
        .expect("Hermes config should exist")
        .content_template
        .as_str();
    let config = without_search_block(config_template);
    let config: serde_json::Value = serde_json::from_str(&config).expect("valid media config");
    assert_eq!(config["stt"]["provider"], "nan-whisper");
    assert_eq!(config["tts"]["provider"], "nan-kokoro");
    assert_eq!(config["image_gen"]["provider"], "nan-harness");
    assert_eq!(
        config["plugins"]["enabled"],
        serde_json::json!(["image_gen/nan_harness"])
    );
    let image_plugin = overlay
        .files
        .iter()
        .find(|file| file.path.ends_with("image_gen/nan_harness/provider.py"))
        .expect("Hermes image provider should exist");
    assert!(
        image_plugin
            .content_template
            .contains(MEDIA_PROVIDER_BASE_URL_PLACEHOLDER)
    );
    assert!(
        !image_plugin
            .content_template
            .contains("{PROVIDER_BASE_URL_PLACEHOLDER}")
    );
    assert_eq!(
        plan.environment
            .secrets
            .get(MEDIA_CREDENTIAL_ENVIRONMENT)
            .map(nan_harness_core::SecretRef::as_str),
        Some("nan_api_key")
    );
}

#[test]
fn hermes_command_providers_use_the_native_string_contract() {
    let tts = hermes_command_provider_config("tts", "kokoro", "https://api.nan.test/v1");
    let stt = hermes_command_provider_config("stt", "whisper-1", "https://api.nan.test/v1");

    for provider in [&tts, &stt] {
        assert_eq!(provider["type"], "command");
        assert!(provider["command"].is_string());
        assert!(provider["command"].as_str().is_some_and(|command| {
            command.contains("{input_path}") && command.contains("{output_path}")
        }));
        assert!(!provider["command"].is_array());
        assert_eq!(
            provider["env_passthrough"],
            serde_json::json!([MEDIA_CREDENTIAL_ENVIRONMENT, "NAN_API_KEY"])
        );
    }
    assert!(
        tts["command"]
            .as_str()
            .is_some_and(|command| { command.contains("{voice}") && command.contains("{format}") })
    );
    assert_eq!(tts["format"], "mp3");
    let quote = if cfg!(windows) { '"' } else { '\'' };
    for provider in [&tts, &stt] {
        let command = provider["command"]
            .as_str()
            .expect("command should be a string");
        for (flag, value) in [
            ("provider-base-url", "https://api.nan.test/v1"),
            ("input", "{input_path}"),
            ("output", "{output_path}"),
        ] {
            assert!(command.contains(&format!("--{flag} {quote}{value}{quote}")));
        }
    }
}
