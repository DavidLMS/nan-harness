use super::support::{Fixture, MARKER, executable};
use nan_harness_test_support::conformance::assert_success;
use serde_yaml_ng::Value;

#[tokio::test]
#[ignore = "requires DSH 0.2.0-rc.2; NAN_REASONING_DSH_EXECUTABLE selects the binary"]
async fn deepseek_persistent_first_boot_refresh_remove_preserves_native_state() {
    let fixture = Fixture::new().await;
    fixture.configure("dsh").await;
    for _ in 0..2 {
        let output = fixture
            .command(executable("dsh"))
            .env("DSH_HOME", fixture.home.join(".dsh"))
            .env("DSH_TELEMETRY_DISABLED", "1")
            .args(["--profile", "headless", "Reply without tools."])
            .run()
            .await
            .unwrap();
        assert_success(&output);
        assert!(output.stdout.contains(MARKER));
    }
    fixture.assert_effort("qwen3.6", Some("high"));
    for operation in ["--status", "--refresh", "--status", "--remove"] {
        let output = fixture
            .command(env!("CARGO_BIN_EXE_nan-harness"))
            .args(["config", "dsh", operation])
            .run()
            .await
            .unwrap();
        assert_success(&output);
        if operation == "--status" {
            assert!(output.stdout.contains("configured, unchanged"));
        }
    }
    assert!(!fixture.home.join(".dsh/.credentials.yaml").exists());
    for profile in ["acp", "web", "headless", "sdk"] {
        assert!(
            !fixture
                .home
                .join(format!(".dsh/profiles/{profile}/cordis.patch.yml"))
                .exists()
        );
    }
    assert!(
        fixture
            .home
            .join(".dsh/profiles/headless/package.json")
            .exists()
    );
}

#[tokio::test]
#[ignore = "requires DSH 0.2.0-rc.2; NAN_REASONING_DSH_EXECUTABLE selects the binary"]
async fn deepseek_native_effort_map_reaches_wire_and_keeps_adaptive_models_unset() {
    for (model, effort, expected) in [
        ("qwen3.6", "off", Some("none")),
        ("qwen3.6", "low", Some("low")),
        ("qwen3.6", "max", Some("max")),
        ("gemma4", "off", Some("none")),
        ("glm5.3-flash", "high", Some("high")),
        ("glm5.3-flash", "max", Some("max")),
        ("deepseek-v4-flash", "", None),
    ] {
        let fixture = Fixture::new().await;
        fixture.configure("dsh").await;
        let path = fixture.home.join(".dsh/profiles/headless/cordis.patch.yml");
        let mut document: Value =
            serde_yaml_ng::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let provider = if ["qwen3.6", "gemma4"].contains(&model) {
            "nan-harness-budgeted"
        } else {
            "nan-harness"
        };
        for entry in document.as_sequence_mut().unwrap() {
            if entry["id"] == "agent-default-model" {
                entry["config"]["provider"] = Value::String(provider.to_owned());
                entry["config"]["model"] = Value::String(model.to_owned());
            } else if entry["id"] == "llm-pi-ai" && !effort.is_empty() {
                entry["config"]["providers"][provider]["reasoning"] =
                    Value::String(effort.to_owned());
            }
        }
        std::fs::write(path, serde_yaml_ng::to_string(&document).unwrap()).unwrap();
        let output = fixture
            .command(executable("dsh"))
            .env("DSH_HOME", fixture.home.join(".dsh"))
            .env("DSH_TELEMETRY_DISABLED", "1")
            .args(["--profile", "headless", "Reply without tools."])
            .run()
            .await
            .unwrap();
        assert_success(&output);
        assert!(output.stdout.contains(MARKER));
        fixture.assert_effort(model, expected);
    }
}

#[tokio::test]
#[ignore = "requires DSH 0.2.0-rc.2; NAN_REASONING_DSH_EXECUTABLE selects the binary"]
async fn deepseek_native_legacy_import_and_credential_rewrite_migrate_safely() {
    let fixture = Fixture::new().await;
    fixture.configure("dsh").await;
    install_legacy_fixture(&fixture);
    // DSH imports old settings after mounting defaults. The initial legacy launch
    // therefore fails locally without an official credential, before any network call.
    let imported = fixture
        .command(executable("dsh"))
        .env("DSH_HOME", fixture.home.join(".dsh"))
        .env("DSH_TELEMETRY_DISABLED", "1")
        .args(["--profile", "headless", "Reply without tools."])
        .run()
        .await
        .unwrap();
    assert!(!imported.status.success());
    assert!(imported.stderr.contains("MISSING_CREDENTIAL"));
    assert!(fixture.home.join(".dsh/settings.yaml.imported").exists());
    let output = fixture
        .command(executable("dsh"))
        .env("DSH_HOME", fixture.home.join(".dsh"))
        .env("DSH_TELEMETRY_DISABLED", "1")
        .args(["--profile", "headless", "Reply without tools."])
        .run()
        .await
        .unwrap();
    assert_success(&output);
    assert!(output.stdout.contains(MARKER));
    let credentials = fixture.home.join(".dsh/.credentials.yaml");
    let normalized: Value =
        serde_yaml_ng::from_slice(&std::fs::read(&credentials).unwrap()).unwrap();
    assert_eq!(normalized["version"], 1);
    assert!(normalized["refs"].get("NAN_API_KEY").is_some());
    for operation in ["--refresh", "--status", "--remove"] {
        let output = fixture
            .command(env!("CARGO_BIN_EXE_nan-harness"))
            .args(["config", "dsh", operation])
            .run()
            .await
            .unwrap();
        assert_success(&output);
        if operation == "--status" {
            assert!(output.stdout.contains("configured, unchanged"));
        }
    }
    let document =
        std::fs::read_to_string(fixture.home.join(".dsh/profiles/headless/cordis.patch.yml"))
            .unwrap();
    assert!(document.contains("personal"));
    assert!(!document.contains("nan-harness"));
    let normalized: Value =
        serde_yaml_ng::from_slice(&std::fs::read(credentials).unwrap()).unwrap();
    assert_eq!(normalized["refs"]["PERSONAL_API_KEY"], "synthetic-personal");
    assert!(normalized["refs"].get("NAN_API_KEY").is_none());
}

fn install_legacy_fixture(fixture: &Fixture) {
    use serde_json::json;
    use sha2::{Digest, Sha256};
    let hash = |value: &str| {
        Sha256::digest(value.as_bytes())
            .iter()
            .fold(String::new(), |mut rendered, byte| {
                use std::fmt::Write;
                write!(&mut rendered, "{byte:02x}").unwrap();
                rendered
            })
    };
    let directory = fixture.home.join(".dsh");
    let document: Value = serde_yaml_ng::from_slice(
        &std::fs::read(directory.join("profiles/headless/cordis.patch.yml")).unwrap(),
    )
    .unwrap();
    let mut legacy = serde_yaml_ng::Mapping::new();
    for row in document.as_sequence().unwrap() {
        legacy.insert(row["id"].clone(), row["config"].clone());
    }
    let mut legacy = Value::Mapping(legacy);
    legacy["agent-default-model"]["provider"] = Value::String("nan-harness".to_owned());
    legacy["llm-pi-ai"]["providers"]
        .as_mapping_mut()
        .unwrap()
        .remove(Value::String("nan-harness-budgeted".to_owned()));
    let mut personal = legacy["llm-pi-ai"]["providers"]["nan-harness"].clone();
    personal["apiKeyEnv"] = Value::String("PERSONAL_API_KEY".to_owned());
    personal["models"] = serde_yaml_ng::from_str("[{id: personal-model}]").unwrap();
    legacy["llm-pi-ai"]["providers"]["personal"] = personal;
    let begin = "# nan-harness:begin deepseek-provider";
    let end = "# nan-harness:end deepseek-provider";
    let body = format!(
        "{begin}\n{}{end}\n",
        serde_yaml_ng::to_string(&legacy).unwrap()
    );
    let settings = directory.join("settings.yaml");
    std::fs::write(&settings, &body).unwrap();
    for profile in ["acp", "web", "headless", "sdk"] {
        std::fs::remove_file(directory.join(format!("profiles/{profile}/cordis.patch.yml")))
            .unwrap();
    }
    let mut integrations = fixture.read_json(".nan-harness/integrations.json");
    integrations
        .as_object_mut()
        .unwrap()
        .remove("deepseekCordis");
    integrations["deepseekHarness"] = json!({"path": settings, "blockSha256": hash(&body), "createdFile": true, "addedSeparator": false});
    fixture.write_json(".nan-harness/integrations.json", &integrations);
    let begin = "# nan-harness:begin provider-credential";
    let end = "# nan-harness:end provider-credential";
    let body = format!("{begin}\nNAN_API_KEY: \"synthetic-native-reasoning-key\"\n{end}\n");
    let credentials = directory.join(".credentials.yaml");
    std::fs::write(
        &credentials,
        format!("PERSONAL_API_KEY: synthetic-personal\n{body}"),
    )
    .unwrap();
    let mut state = fixture.read_json(".nan-harness/configurations.json");
    let documents = state["harnesses"]["deepseek-harness"]["documents"]
        .as_array_mut()
        .unwrap();
    for document in documents {
        if document["path"] == credentials.to_string_lossy().as_ref() {
            *document = json!({"format":"text-block","path":credentials,"createdFile":false,"begin":begin,"end":end,"blockSha256":hash(&body),"active":true});
        }
    }
    fixture.write_json(".nan-harness/configurations.json", &state);
}
