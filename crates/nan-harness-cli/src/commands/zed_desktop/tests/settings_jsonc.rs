use super::super::ZedDesktopError;
use super::super::documents::{patch_settings, read_optional};
use super::fixtures::{GATEWAY_URL, fixture_paths, model, parse_jsonc};
use nan_harness_core::{ReasoningEffort, ReasoningPolicy};
use serde_json::json;
use std::fs;

type ErrorMatcher = fn(&ZedDesktopError) -> bool;

#[test]
fn jsonc_patch_preserves_user_content_and_builds_the_live_catalog() {
    let source = br#"{
  // Keep this comment and the official provider.
  "language_models": {
    "openai": { "api_url": "https://user.example/v1" },
  },
  "agent": {
    "default_model": { "provider": "openai", "model": "user-model" },
  },
  "theme": "One Dark",
}
"#;
    let models = vec![
        model(
            "qwen3.6",
            "NaN Qwen",
            262_144,
            32_768,
            true,
            ReasoningPolicy::Effort {
                supported: [
                    ReasoningEffort::Low,
                    ReasoningEffort::Medium,
                    ReasoningEffort::High,
                ],
                default: ReasoningEffort::Medium,
            },
        ),
        model(
            "account-specific-model",
            "NaN Custom",
            65_536,
            8_192,
            false,
            ReasoningPolicy::Toggle {
                default_enabled: true,
            },
        ),
        model(
            "always-reasoning",
            "NaN Always",
            32_768,
            4_096,
            false,
            ReasoningPolicy::AlwaysOn,
        ),
    ];

    let patched =
        patch_settings(Some(source), GATEWAY_URL, &models, "qwen3.6").expect("JSONC should patch");
    let rendered = String::from_utf8(patched.contents.clone()).expect("settings should be UTF-8");
    let value = parse_jsonc(&patched.contents);
    let available = value["language_models"]["openai_compatible"]["nan"]["available_models"]
        .as_array()
        .expect("catalog should be an array");

    assert!(rendered.contains("// Keep this comment and the official provider."));
    assert_eq!(
        value["language_models"]["openai"]["api_url"],
        "https://user.example/v1"
    );
    assert_eq!(
        value["language_models"]["openai_compatible"]["nan"]["api_url"],
        GATEWAY_URL
    );
    assert_eq!(available.len(), models.len());
    assert_eq!(available[0]["name"], "qwen3.6");
    assert_eq!(available[0]["display_name"], "NaN Qwen");
    assert_eq!(available[0]["max_tokens"], 262_144);
    assert_eq!(available[0]["max_output_tokens"], 32_768);
    assert_eq!(available[0]["reasoning_effort"], "medium");
    // Zed 1.18 rejects partial capability objects even for documented defaults.
    assert_eq!(
        available[0]["capabilities"],
        json!({
            "tools": true,
            "images": true,
            "parallel_tool_calls": false,
            "prompt_cache_key": false,
            "chat_completions": true,
            "interleaved_reasoning": false,
            "max_tokens_parameter": true,
        })
    );
    assert!(available[1].get("reasoning_effort").is_none());
    assert!(available[2].get("reasoning_effort").is_none());
    assert_eq!(
        value["agent"]["default_model"],
        json!({"provider": "nan", "model": "qwen3.6"})
    );
    assert_eq!(
        patched.previous_default_model,
        Some(json!({"provider": "openai", "model": "user-model"}))
    );
}

#[test]
fn malformed_or_foreign_settings_fail_closed() {
    let model = model(
        "qwen3.6",
        "NaN Qwen",
        1_024,
        256,
        false,
        ReasoningPolicy::Unknown,
    );
    let cases: &[(&[u8], ErrorMatcher)] = &[
        (b"[]", |error| {
            matches!(error, ZedDesktopError::SettingsRootNotObject)
        }),
        (br#"{"language_models": false}"#, |error| {
            matches!(
                error,
                ZedDesktopError::SettingsFieldNotObject("language_models")
            )
        }),
        (
            br#"{"language_models":{"openai_compatible":[]}}"#,
            |error| {
                matches!(
                    error,
                    ZedDesktopError::SettingsFieldNotObject("language_models.openai_compatible")
                )
            },
        ),
        (br#"{"agent":false}"#, |error| {
            matches!(error, ZedDesktopError::SettingsFieldNotObject("agent"))
        }),
        (br#"{"agent":{"default_model":"model"}}"#, |error| {
            matches!(error, ZedDesktopError::InvalidDefaultModel)
        }),
        (
            br#"{"language_models":{"openai_compatible":{"nan":{}}}}"#,
            |error| matches!(error, ZedDesktopError::UnmanagedProviderConflict),
        ),
    ];

    for (source, expected) in cases {
        let error = patch_settings(
            Some(source),
            GATEWAY_URL,
            std::slice::from_ref(&model),
            "qwen3.6",
        )
        .expect_err("unsafe settings should fail");
        assert!(expected(&error), "unexpected error: {error:?}");
    }
}

#[cfg(unix)]
#[test]
fn symlinked_settings_are_rejected_without_touching_the_target() {
    use std::os::unix::fs::symlink;

    let fixture = fixture_paths();
    let target = fixture.root.path().join("real-settings.json");
    fs::create_dir_all(
        fixture
            .paths
            .settings
            .parent()
            .expect("settings parent should exist"),
    )
    .expect("settings parent should be created");
    fs::write(&target, b"{\"theme\":\"safe\"}\n").expect("target should be written");
    symlink(&target, &fixture.paths.settings).expect("symlink should be created");

    let error = read_optional(&fixture.paths.settings).expect_err("symlink should fail");
    assert!(matches!(error, ZedDesktopError::State(_)));
    assert_eq!(
        fs::read(&target).expect("target should remain"),
        b"{\"theme\":\"safe\"}\n"
    );
}
