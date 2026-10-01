use super::*;
use nan_harness_core::ImageModel;

#[test]
fn image_model_configuration_survives_refresh_and_restores_user_settings() {
    for harness in [HarnessKind::Hermes, HarnessKind::OpenClaw] {
        let root = tempdir().expect("workspace");
        let home = root.path().join("home");
        let manager = ConfigurationManager::new(&root.path().join("state"), &home);
        let (path, original) = match harness {
            HarnessKind::Hermes => (
                home.join(".hermes/config.yaml"),
                "image_gen:\n  provider: external\n  model: user-model\nplugins:\n  entries:\n    image_gen/nan_harness:\n      allow_tool_override: false\n",
            ),
            _ => (
                home.join(".openclaw/openclaw.json"),
                r#"{"agents":{"defaults":{"mediaModels":{"image":{"primary":"external/user-model"}}}}}"#,
            ),
        };
        fs::create_dir_all(path.parent().expect("parent")).expect("home");
        fs::write(&path, original).expect("user settings");
        let selected = MediaSelection {
            image: true,
            image_model: Some(ImageModel::QwenImage21),
            ..MediaSelection::none()
        };
        for requested in [Some(selected), None] {
            let change = manager
                .configure_with_media(
                    harness,
                    &test_config(),
                    &test_models(),
                    Some(WebSearchPolicy::Disabled),
                    requested,
                )
                .expect("configure images");
            assert_eq!(change.media.image_model, Some(ImageModel::QwenImage21));
            let content = fs::read_to_string(&path).expect("native config");
            if harness == HarnessKind::Hermes {
                let value: YamlValue = serde_yaml_ng::from_str(&content).expect("YAML");
                assert_eq!(value["image_gen"]["model"], "qwen-image-2.1");
                assert_eq!(
                    value["plugins"]["entries"]["image_gen/nan_harness"]["allow_tool_override"],
                    true
                );
            } else {
                let value: Value = serde_json::from_str(&content).expect("JSON");
                assert_eq!(
                    value["agents"]["defaults"]["mediaModels"]["image"]["primary"],
                    "nan-harness/qwen-image-2.1"
                );
            }
        }
        manager
            .remove(harness)
            .expect("remove managed configuration");
        let restored = fs::read_to_string(&path).expect("restored config");
        if harness == HarnessKind::Hermes {
            assert_eq!(
                serde_yaml_ng::from_str::<YamlValue>(&restored).unwrap(),
                serde_yaml_ng::from_str::<YamlValue>(original).unwrap()
            );
        } else {
            assert_eq!(
                serde_json::from_str::<Value>(&restored).unwrap(),
                serde_json::from_str::<Value>(original).unwrap()
            );
        }
    }
}
