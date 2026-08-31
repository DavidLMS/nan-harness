use super::documents::{
    get_yaml_path, prepare_exact_file, prepare_exact_file_removal, prepare_json,
    prepare_json_removal, prepare_text_block, prepare_yaml, prepare_yaml_removal,
};
use super::*;
use nan_harness_core::SecretValue;
use nan_harness_runtime::{ConfigOverrides, ConfigResolver, ProcessEnvironment};
use tempfile::tempdir;

#[test]
fn all_native_configurations_are_reversible_and_keep_secrets_out_of_receipts() {
    let root = tempdir().expect("temporary directory should be created");
    let home = root.path().join("home");
    let state = root.path().join("state");
    fs::create_dir_all(&home).expect("home should be created");
    let manager = ConfigurationManager::new(&state, &home);
    let models = test_models();

    for harness in SUPPORTED_HARNESSES {
        let plans = manager
            .plans_for(
                harness,
                "secret-value",
                "https://api.nan.test/v1",
                &models,
                "qwen3.6",
                ManagedSearchStatus {
                    policy: WebSearchPolicy::Auto,
                    managed: false,
                },
            )
            .expect("native configuration plan should build");
        let prepared =
            prepare_documents(&plans, None).expect("native configuration should prepare");
        let receipts = prepared
            .iter()
            .map(|document| document.receipt.clone())
            .collect::<Vec<_>>();
        let serialized =
            serde_json::to_string(&receipts).expect("configuration receipts should serialize");
        assert!(
            !serialized.contains("secret-value"),
            "{harness} leaked its credential into a receipt"
        );

        apply_prepared(&prepared).expect("native configuration should apply");
        assert!(
            receipts.iter().all(document_is_active),
            "{harness} configuration was not recognized as active"
        );
        let removals =
            prepare_removals(&receipts).expect("native configuration removal should prepare");
        apply_prepared(&removals).expect("native configuration should be removed");
        assert!(
            receipts
                .iter()
                .all(|receipt| !receipt_path(receipt).exists()),
            "{harness} left a newly created configuration document behind"
        );
    }
}

#[test]
fn integrated_configuration_lifecycle_covers_every_supported_harness() {
    let models = test_models();
    let config = ConfigResolver::resolve(
        &ProcessEnvironment,
        ConfigOverrides {
            provider_base_url: Some("https://api.nan.test/v1".to_owned()),
            nan_api_key: Some(
                SecretValue::new("secret-value").expect("test credential should be valid"),
            ),
        },
    )
    .expect("test configuration should resolve");

    for harness in SUPPORTED_HARNESSES {
        let root = tempdir().expect("temporary directory should be created");
        let home = root.path().join("home");
        let state = root.path().join("state");
        fs::create_dir_all(&home).expect("home should be created");
        let manager = ConfigurationManager::new(&state, &home);
        let expected_paths = manager
            .paths_for_search(harness, true)
            .expect("configuration paths should resolve");

        let change = manager
            .configure(
                harness,
                &config,
                &models,
                Some(if harness == HarnessKind::Aider {
                    WebSearchPolicy::Disabled
                } else {
                    WebSearchPolicy::Force
                }),
            )
            .expect("native configuration should apply");
        assert!(change.changed, "{harness} should change a clean home");
        assert_eq!(change.paths, expected_paths, "{harness} path list changed");
        assert!(
            change.paths.iter().all(|path| path.exists()),
            "{harness} omitted a written path"
        );
        assert!(manager.is_active(harness).expect("status should resolve"));
        assert!(
            !fs::read_to_string(state.join(STATE_FILE_NAME))
                .expect("receipt should be readable")
                .contains("secret-value"),
            "{harness} leaked its credential into the receipt"
        );
        for entry in fs::read_dir(&state).expect("state directory should be readable") {
            let path = entry.expect("state entry should be readable").path();
            if path.is_file() {
                assert!(
                    !fs::read_to_string(&path)
                        .expect("state file should be UTF-8")
                        .contains("secret-value"),
                    "{harness} leaked its credential into {}",
                    path.display()
                );
            }
        }
        assert_persistent_search_contract(harness, &home);

        if matches!(harness, HarnessKind::Pi | HarnessKind::PrimeAgent) {
            let models_path = change
                .paths
                .iter()
                .find(|path| path.file_name().is_some_and(|name| name == "models.json"))
                .expect("Pi-compatible configuration should include models.json");
            let catalog = fs::read_to_string(models_path)
                .expect("Pi-compatible model catalog should be readable");
            assert!(
                catalog.contains(r#""apiKey": "NAN_API_KEY""#),
                "{harness} should provide the required environment credential fallback"
            );
            assert!(
                !catalog.contains("secret-value"),
                "{harness} leaked its credential into the model catalog"
            );
        }
        if harness == HarnessKind::Omp {
            assert_omp_role_routing(&change.paths);
        }

        assert_eq!(
            manager
                .remove(harness)
                .expect("configuration should remove"),
            RemovalOutcome::Removed
        );
        assert!(
            change.paths.iter().all(|path| !path.exists()),
            "{harness} left a clean-home configuration behind"
        );
        assert!(
            !manager
                .is_configured(harness)
                .expect("status should resolve")
        );
    }
}

fn assert_omp_role_routing(paths: &[PathBuf]) {
    let config_path = paths
        .iter()
        .find(|path| {
            path.file_name()
                .is_some_and(|name| name == "config.yml" || name == "config.yaml")
        })
        .expect("OMP configuration should include its YAML settings");
    let settings = fs::read_to_string(config_path).expect("OMP configuration should be readable");
    for role in [
        "default", "smol", "slow", "vision", "plan", "designer", "commit", "tiny", "task",
        "advisor",
    ] {
        assert!(
            settings.contains(&format!("{role}: nan/qwen3.6")),
            "OMP role {role} should route through NaN"
        );
    }
}

#[test]
fn persistent_search_policy_preserves_external_search_and_transitions_safely() {
    let root = tempdir().expect("temporary directory should be created");
    let home = root.path().join("home");
    let state = root.path().join("state");
    let mcp_path = home.join(".cline/data/settings/mcp_settings.json");
    fs::create_dir_all(mcp_path.parent().expect("MCP path should have a parent"))
        .expect("MCP directory should be created");
    fs::write(
        &mcp_path,
        r#"{"mcpServers":{"brave-search":{"command":"brave-search"}}}"#,
    )
    .expect("external search should be written");
    let manager = ConfigurationManager::new(&state, &home);
    let config = test_config();
    let models = test_models();

    manager
        .configure(HarnessKind::Cline, &config, &models, None)
        .expect("auto configuration should preserve external search");
    let auto: Value = serde_json::from_slice(&fs::read(&mcp_path).expect("MCP config should read"))
        .expect("MCP config should parse");
    assert!(auto["mcpServers"].get("brave-search").is_some());
    assert!(auto["mcpServers"].get(SEARCH_MCP_ID).is_none());
    let receipt = manager
        .load_state()
        .expect("state should load")
        .harnesses
        .get(&HarnessKind::Cline.to_string())
        .expect("Cline receipt should exist")
        .clone();
    assert_eq!(receipt.search_policy, WebSearchPolicy::Auto);
    assert!(!receipt.search_managed);

    manager
        .configure(
            HarnessKind::Cline,
            &config,
            &models,
            Some(WebSearchPolicy::Force),
        )
        .expect("force should add managed search");
    let forced: Value =
        serde_json::from_slice(&fs::read(&mcp_path).expect("MCP config should read"))
            .expect("MCP config should parse");
    assert!(forced["mcpServers"].get("brave-search").is_some());
    assert!(forced["mcpServers"].get(SEARCH_MCP_ID).is_some());

    manager
        .configure(
            HarnessKind::Cline,
            &config,
            &models,
            Some(WebSearchPolicy::Disabled),
        )
        .expect("disabled policy should remove only managed search");
    let disabled: Value =
        serde_json::from_slice(&fs::read(&mcp_path).expect("MCP config should read"))
            .expect("MCP config should parse");
    assert!(disabled["mcpServers"].get("brave-search").is_some());
    assert!(disabled["mcpServers"].get(SEARCH_MCP_ID).is_none());
}

#[test]
fn persistent_auto_policy_survives_refresh_without_an_override() {
    let root = tempdir().expect("temporary directory should be created");
    let home = root.path().join("home");
    let state = root.path().join("state");
    fs::create_dir_all(&home).expect("home should be created");
    let manager = ConfigurationManager::new(&state, &home);
    let config = test_config();
    let models = test_models();

    manager
        .configure(HarnessKind::Cline, &config, &models, None)
        .expect("auto should configure search on a clean home");
    manager
        .configure(HarnessKind::Cline, &config, &models, None)
        .expect("refresh should preserve auto search");
    let receipt = manager
        .load_state()
        .expect("state should load")
        .harnesses
        .get(&HarnessKind::Cline.to_string())
        .expect("Cline receipt should exist")
        .clone();
    assert_eq!(receipt.search_policy, WebSearchPolicy::Auto);
    assert!(receipt.search_managed);
}

#[test]
fn pi_family_search_policy_uses_the_runtime_tool_inventory() {
    for (harness, relative_directory) in [
        (HarnessKind::Pi, ".pi/agent"),
        (HarnessKind::PrimeAgent, ".prime/agent"),
    ] {
        let root = tempdir().expect("temporary directory should be created");
        let home = root.path().join("home");
        let state = root.path().join("state");
        let directory = home.join(relative_directory);
        fs::create_dir_all(&directory).expect("Pi-compatible directory should be created");
        let settings_path = directory.join("settings.json");
        fs::write(&settings_path, br#"{"packages":["npm:pi-web-access"]}"#)
            .expect("package configuration should be written");
        let manager = ConfigurationManager::new(&state, &home);
        let config = test_config();
        let models = test_models();
        let extension_path = directory.join(PI_SEARCH_EXTENSION_FILE);

        manager
            .configure(harness, &config, &models, None)
            .expect("automatic search should install a runtime-aware fallback");
        let automatic = fs::read_to_string(&extension_path)
            .expect("automatic search extension should be readable");
        assert!(automatic.contains("const forceNanSearch = false"));
        assert!(automatic.contains("pi.getAllTools()"));
        assert!(automatic.contains("tool.name === \"web_search\""));
        let settings: Value = serde_json::from_slice(
            &fs::read(&settings_path).expect("package configuration should be readable"),
        )
        .expect("package configuration should remain valid JSON");
        assert_eq!(settings["packages"], json!(["npm:pi-web-access"]));

        manager
            .configure(harness, &config, &models, Some(WebSearchPolicy::Force))
            .expect("forced search should replace a package tool");
        let forced = fs::read_to_string(&extension_path)
            .expect("forced search extension should be readable");
        assert!(forced.contains("const forceNanSearch = true"));

        manager
            .configure(harness, &config, &models, Some(WebSearchPolicy::Disabled))
            .expect("disabled search should remove the managed extension");
        assert!(!extension_path.exists());
        assert!(manager.is_active(harness).expect("status should resolve"));
    }
}

#[test]
fn pi_native_refresh_migrates_the_managed_search_mcp_to_an_extension() {
    let root = tempdir().expect("temporary directory should be created");
    let home = root.path().join("home");
    let state_directory = root.path().join("state");
    let directory = home.join(".pi/agent");
    fs::create_dir_all(&directory).expect("Pi directory should be created");
    let mcp_path = directory.join("mcp.json");
    fs::write(
        &mcp_path,
        br#"{"mcpServers":{"user-owned":{"command":"user-search"}}}"#,
    )
    .expect("user MCP configuration should be written");
    let manager = ConfigurationManager::new(&state_directory, &home);
    let config = test_config();
    let models = test_models();

    let mut old_plans = pi_family_plans(
        &directory,
        "secret-value",
        "https://api.nan.test/v1",
        &models,
        "qwen3.6",
        ManagedSearchStatus {
            policy: WebSearchPolicy::Auto,
            managed: false,
        },
    );
    old_plans.truncate(3);
    old_plans.push(search_mcp_plan(
        mcp_path.clone(),
        "secret-value",
        "https://api.nan.test/v1",
        true,
    ));
    let prepared = prepare_documents(&old_plans, None).expect("old MCP setup should prepare");
    let documents = prepared
        .iter()
        .map(|document| document.receipt.clone())
        .collect();
    apply_prepared(&prepared).expect("old MCP setup should apply");
    let mut state = ConfigurationState::default();
    state.harnesses.insert(
        HarnessKind::Pi.to_string(),
        HarnessReceipt {
            credential_fingerprint: "old-fingerprint".to_owned(),
            model_ids: models.iter().map(|model| model.id.clone()).collect(),
            search_policy: WebSearchPolicy::Auto,
            search_managed: true,
            documents,
        },
    );
    manager
        .save_state(&state)
        .expect("old receipt should be saved");

    manager
        .configure(HarnessKind::Pi, &config, &models, None)
        .expect("refresh should migrate managed search");

    let mcp: Value = serde_json::from_slice(&fs::read(&mcp_path).expect("MCP config should read"))
        .expect("MCP config should remain valid JSON");
    assert!(mcp["mcpServers"].get("user-owned").is_some());
    assert!(mcp["mcpServers"].get(SEARCH_MCP_ID).is_none());
    let extension = fs::read_to_string(directory.join(PI_SEARCH_EXTENSION_FILE))
        .expect("runtime-aware extension should be installed");
    assert!(extension.contains("const forceNanSearch = false"));
    assert!(!extension.contains("secret-value"));
}

#[test]
fn reserved_search_collision_is_bypassed_only_when_search_is_disabled() {
    let root = tempdir().expect("temporary directory should be created");
    let home = root.path().join("home");
    let state = root.path().join("state");
    let mcp_path = home.join(".cline/data/settings/mcp_settings.json");
    fs::create_dir_all(mcp_path.parent().expect("MCP path should have a parent"))
        .expect("MCP directory should be created");
    fs::write(
        &mcp_path,
        r#"{"mcpServers":{"nan-search":{"command":"third-party"}}}"#,
    )
    .expect("collision should be written");
    let manager = ConfigurationManager::new(&state, &home);
    let config = test_config();
    let models = test_models();

    assert!(matches!(
        manager.configure(HarnessKind::Cline, &config, &models, None),
        Err(ConfigurationError::SearchPolicy(SearchPolicyError::McpNameCollision(path)))
            if path == mcp_path
    ));
    manager
        .configure(
            HarnessKind::Cline,
            &config,
            &models,
            Some(WebSearchPolicy::Disabled),
        )
        .expect("disabled search should preserve the collision untouched");
    assert!(
        fs::read_to_string(mcp_path)
            .expect("collision should remain readable")
            .contains("third-party")
    );
}

#[test]
fn force_search_rejects_aider_without_writing_configuration() {
    let root = tempdir().expect("temporary directory should be created");
    let home = root.path().join("home");
    let state = root.path().join("state");
    fs::create_dir_all(&home).expect("home should be created");
    let manager = ConfigurationManager::new(&state, &home);

    assert!(matches!(
        manager.configure(
            HarnessKind::Aider,
            &test_config(),
            &test_models(),
            Some(WebSearchPolicy::Force),
        ),
        Err(ConfigurationError::SearchPolicy(
            SearchPolicyError::UnsupportedHarness(HarnessKind::Aider)
        ))
    ));
    assert!(!state.join(STATE_FILE_NAME).exists());
}

#[test]
fn legacy_receipts_default_to_auto_without_claiming_search() {
    let receipt: HarnessReceipt = serde_json::from_value(json!({
        "credentialFingerprint": "fingerprint",
        "modelIds": ["qwen3.6"],
        "documents": []
    }))
    .expect("legacy receipt should deserialize");
    assert_eq!(receipt.search_policy, WebSearchPolicy::Auto);
    assert!(!receipt.search_managed);
}

#[test]
fn persistent_search_plugins_have_valid_source_syntax() {
    use std::io::Write as _;
    use std::process::{Command, Stdio};

    let mut node = match Command::new("node")
        .args(["--input-type=module", "--check"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => panic!("Node syntax check should start: {error}"),
    };
    node.stdin
        .take()
        .expect("Node stdin should be available")
        .write_all(openclaw_search_plugin().as_bytes())
        .expect("plugin source should write");
    let output = node
        .wait_with_output()
        .expect("Node syntax check should finish");
    assert!(
        output.status.success(),
        "OpenClaw plugin syntax failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    for mode in [PiSearchMode::Auto, PiSearchMode::Force] {
        let mut node = Command::new("node")
            .args(["--input-type=module", "--check"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("Node syntax check should start after the first successful invocation");
        node.stdin
            .take()
            .expect("Node stdin should be available")
            .write_all(render_pi_search_extension("https://api.nan.test/v1", mode).as_bytes())
            .expect("Pi extension source should write");
        let output = node
            .wait_with_output()
            .expect("Node syntax check should finish");
        assert!(
            output.status.success(),
            "Pi extension syntax failed in {mode:?} mode: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let mut python = match Command::new("python3")
        .args([
            "-c",
            "import sys; compile(sys.stdin.read(), 'provider.py', 'exec')",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => panic!("Python syntax check should start: {error}"),
    };
    python
        .stdin
        .take()
        .expect("Python stdin should be available")
        .write_all(hermes_search_provider().as_bytes())
        .expect("provider source should write");
    let output = python
        .wait_with_output()
        .expect("Python syntax check should finish");
    assert!(
        output.status.success(),
        "Hermes provider syntax failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn pi_search_extension_runtime_detection_respects_auto_and_force() {
    use std::fmt::Write as _;
    use std::io::Write as _;
    use std::process::{Command, Stdio};

    for (mode, existing_search, expected_registrations) in [
        (PiSearchMode::Auto, true, 0),
        (PiSearchMode::Auto, false, 1),
        (PiSearchMode::Force, true, 1),
    ] {
        let mut source = render_pi_search_extension("https://api.nan.test/v1", mode)
            .replacen(
                "import { Type } from \"@earendil-works/pi-ai\";",
                "const Type = new Proxy({}, { get: () => (...args) => args[0] ?? {} });",
                1,
            )
            .replacen(
                "export default function registerNanSearch",
                "function registerNanSearch",
                1,
            );
        let inventory = if existing_search {
            "[{ name: \"web_search\" }]"
        } else {
            "[]"
        };
        write!(
            source,
            r#"
let discover;
const registrations = [];
const pi = {{
  on(event, handler) {{
    if (event !== "resources_discover") throw new Error(`unexpected event: ${{event}}`);
    discover = handler;
  }},
  getAllTools() {{ return {inventory}; }},
  registerTool(tool) {{ registrations.push(tool); }}
}};
registerNanSearch(pi);
discover();
if (registrations.length !== {expected_registrations}) {{
  throw new Error(`expected {expected_registrations} registrations, got ${{registrations.length}}`);
}}
"#
        )
        .expect("runtime check source should render");

        let mut node = match Command::new("node")
            .args(["--input-type=module"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => panic!("Node runtime check should start: {error}"),
        };
        node.stdin
            .take()
            .expect("Node stdin should be available")
            .write_all(source.as_bytes())
            .expect("runtime check source should write");
        let output = node
            .wait_with_output()
            .expect("Node runtime check should finish");
        assert!(
            output.status.success(),
            "Pi runtime detection failed for {mode:?} with existing_search={existing_search}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn json_refresh_rotates_secrets_and_restores_previous_defaults() {
    let root = tempdir().expect("temporary directory should be created");
    let path = root.path().join("settings.json");
    fs::write(&path, br#"{"defaultModel":"user-model"}"#).expect("fixture should be written");
    let first = JsonPlan {
        path: path.clone(),
        entries: vec![
            override_json(&["defaultModel"], json!("qwen3.6")),
            exclusive_json(&["providers", "nan"], json!({"key": "first-secret"})),
        ],
    };
    let prepared = prepare_json(&first, None).expect("configuration should prepare");
    let first_receipt = json_receipt(&prepared);
    apply_prepared(&[prepared]).expect("configuration should apply");

    let refreshed = JsonPlan {
        path: path.clone(),
        entries: vec![
            override_json(&["defaultModel"], json!("qwen3.6")),
            exclusive_json(&["providers", "nan"], json!({"key": "second-secret"})),
        ],
    };
    let prepared =
        prepare_json(&refreshed, Some(&first_receipt)).expect("configuration should refresh");
    let refreshed_receipt = json_receipt(&prepared);
    assert!(
        !serde_json::to_string(&refreshed_receipt)
            .expect("receipt should serialize")
            .contains("second-secret")
    );
    apply_prepared(&[prepared]).expect("refresh should apply");
    let removal =
        prepare_json_removal(&refreshed_receipt).expect("configuration removal should prepare");
    apply_prepared(&[removal]).expect("configuration removal should apply");
    assert_eq!(
        fs::read_to_string(path).expect("user configuration should remain"),
        "{\n  \"defaultModel\": \"user-model\"\n}"
    );
}

#[test]
fn json_entries_can_be_added_and_removed_across_refreshes() {
    let root = tempdir().expect("temporary directory should be created");
    let path = root.path().join("settings.json");
    fs::write(&path, br#"{"userSearch":{"enabled":true}}"#).expect("fixture should be written");
    let inactive = JsonPlan {
        path: path.clone(),
        entries: Vec::new(),
    };
    let initial = prepare_json(&inactive, None).expect("empty plan should prepare");
    let inactive_receipt = json_receipt(&initial);
    assert_eq!(initial.original, initial.replacement);
    apply_prepared(&[initial]).expect("empty plan should apply");

    let active = JsonPlan {
        path: path.clone(),
        entries: vec![exclusive_json(
            &["mcpServers", "nan-search"],
            json!({"command": "nan-harness"}),
        )],
    };
    let enabled =
        prepare_json(&active, Some(&inactive_receipt)).expect("managed entry should be added");
    let enabled_receipt = json_receipt(&enabled);
    apply_prepared(&[enabled]).expect("managed entry should apply");

    let disabled =
        prepare_json(&inactive, Some(&enabled_receipt)).expect("managed entry should be removed");
    let disabled_receipt = json_receipt(&disabled);
    apply_prepared(&[disabled]).expect("managed entry removal should apply");
    assert!(document_is_active(&DocumentReceipt::Json(disabled_receipt)));
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(path).expect("user configuration should remain"))
            .expect("user configuration should stay valid"),
        json!({"userSearch": {"enabled": true}})
    );
}

#[test]
fn document_refresh_matches_receipts_by_path_and_accepts_new_documents() {
    let root = tempdir().expect("temporary directory should be created");
    let first = JsonPlan {
        path: root.path().join("first.json"),
        entries: vec![exclusive_json(&["managed"], json!(1))],
    };
    let second = JsonPlan {
        path: root.path().join("second.json"),
        entries: vec![exclusive_json(&["managed"], json!(2))],
    };
    let prepared = prepare_documents(&[DocumentPlan::Json(first.clone())], None)
        .expect("initial document should prepare");
    let receipts = prepared
        .iter()
        .map(|document| document.receipt.clone())
        .collect::<Vec<_>>();
    apply_prepared(&prepared).expect("initial document should apply");

    let refreshed = prepare_documents(
        &[DocumentPlan::Json(second), DocumentPlan::Json(first)],
        Some(&receipts),
    )
    .expect("new document should migrate alongside the existing receipt");
    assert_eq!(refreshed.len(), 2);
}

#[test]
fn yaml_refresh_merges_lists_and_restores_user_search() {
    let root = tempdir().expect("temporary directory should be created");
    let path = root.path().join("config.yaml");
    fs::write(
        &path,
        "plugins:\n  enabled: [web/tavily]\nweb:\n  search_backend: tavily\n",
    )
    .expect("fixture should be written");
    let plan = YamlPlan {
        path: path.clone(),
        entries: vec![
            YamlEntryPlan {
                path: vec!["plugins".to_owned(), "enabled".to_owned()],
                value: YamlValue::String("web/nan_harness".to_owned()),
                mode: YamlEntryMode::AppendUnique,
            },
            YamlEntryPlan {
                path: vec!["web".to_owned(), "search_backend".to_owned()],
                value: YamlValue::String("nan-harness".to_owned()),
                mode: YamlEntryMode::Override,
            },
        ],
        legacy_block: None,
    };
    let prepared = prepare_yaml(&plan, None).expect("YAML merge should prepare");
    let receipt = match &prepared.receipt {
        DocumentReceipt::Yaml(receipt) => receipt.clone(),
        _ => unreachable!(),
    };
    apply_prepared(&[prepared]).expect("YAML merge should apply");
    let active: YamlValue =
        serde_yaml_ng::from_slice(&fs::read(&path).expect("managed YAML should remain readable"))
            .expect("managed YAML should parse");
    assert_eq!(
        get_yaml_path(&active, &["plugins".to_owned(), "enabled".to_owned()]),
        Some(&YamlValue::Sequence(vec![
            YamlValue::String("web/tavily".to_owned()),
            YamlValue::String("web/nan_harness".to_owned())
        ]))
    );

    let removal = prepare_yaml_removal(&receipt).expect("YAML removal should prepare");
    apply_prepared(&[removal]).expect("YAML removal should apply");
    let restored: YamlValue =
        serde_yaml_ng::from_slice(&fs::read(path).expect("user YAML should remain readable"))
            .expect("restored YAML should parse");
    assert_eq!(
        restored,
        serde_yaml_ng::from_str::<YamlValue>(
            "plugins:\n  enabled: [web/tavily]\nweb:\n  search_backend: tavily\n"
        )
        .expect("fixture YAML should parse")
    );
}

#[test]
fn yaml_plan_migrates_a_managed_text_block() {
    let root = tempdir().expect("temporary directory should be created");
    let path = root.path().join("config.yaml");
    let legacy = TextBlockPlan {
        path: path.clone(),
        begin: "# begin".to_owned(),
        end: "# end".to_owned(),
        body: Some("model:\n  provider: custom".to_owned()),
        conflicting_keys: vec!["model:".to_owned()],
    };
    let prepared = prepare_text_block(&legacy, None).expect("legacy block should prepare");
    let receipt = prepared.receipt.clone();
    apply_prepared(&[prepared]).expect("legacy block should apply");

    let model = serde_yaml_ng::from_str::<YamlValue>("provider: custom\n")
        .expect("model YAML should parse");
    let migrated = YamlPlan {
        path: path.clone(),
        entries: vec![YamlEntryPlan {
            path: vec!["model".to_owned()],
            value: model,
            mode: YamlEntryMode::Exclusive,
        }],
        legacy_block: Some(LegacyTextBlock {
            begin: legacy.begin,
            end: legacy.end,
        }),
    };
    let prepared = prepare_documents(&[DocumentPlan::Yaml(migrated)], Some(&[receipt]))
        .expect("legacy block should migrate");
    assert!(matches!(prepared[0].receipt, DocumentReceipt::Yaml(_)));
    apply_prepared(&prepared).expect("migrated YAML should apply");
    assert!(
        !fs::read_to_string(&path)
            .expect("migrated YAML should be readable")
            .contains("# begin")
    );
}

#[test]
fn inactive_optional_files_preserve_unmanaged_content_and_conflict_when_enabled() {
    let root = tempdir().expect("temporary directory should be created");
    let path = root.path().join("plugin.js");
    fs::write(&path, "user plugin\n").expect("user plugin should be written");
    let inactive = ExactFilePlan {
        path: path.clone(),
        payload: None,
    };
    let prepared = prepare_exact_file(&inactive, None).expect("inactive file should prepare");
    let receipt = match &prepared.receipt {
        DocumentReceipt::ExactFile(receipt) => receipt.clone(),
        _ => unreachable!(),
    };
    apply_prepared(&[prepared]).expect("inactive file should preserve user content");
    let removal = prepare_exact_file_removal(&receipt).expect("inactive receipt should remove");
    apply_prepared(&[removal]).expect("inactive removal should preserve user content");
    assert_eq!(
        fs::read_to_string(&path).expect("user plugin should remain"),
        "user plugin\n"
    );

    let active = ExactFilePlan {
        path,
        payload: Some(b"managed plugin\n".to_vec()),
    };
    assert!(matches!(
        prepare_exact_file(&active, Some(&receipt)),
        Err(ConfigurationError::UnmanagedDocumentConflict(_))
    ));
}

#[test]
fn optional_text_blocks_disappear_when_search_is_disabled() {
    let root = tempdir().expect("temporary directory should be created");
    let path = root.path().join("patch.yml");
    let inactive = TextBlockPlan {
        path: path.clone(),
        begin: "# begin".to_owned(),
        end: "# end".to_owned(),
        body: None,
        conflicting_keys: Vec::new(),
    };
    let prepared = prepare_text_block(&inactive, None).expect("inactive block should prepare");
    let inactive_receipt = match &prepared.receipt {
        DocumentReceipt::TextBlock(receipt) => receipt.clone(),
        _ => unreachable!(),
    };
    apply_prepared(&[prepared]).expect("inactive block should apply");
    assert!(!path.exists());

    let active = TextBlockPlan {
        body: Some("- id: mcp-nan-search".to_owned()),
        ..inactive.clone()
    };
    let prepared =
        prepare_text_block(&active, Some(&inactive_receipt)).expect("active block should prepare");
    let active_receipt = match &prepared.receipt {
        DocumentReceipt::TextBlock(receipt) => receipt.clone(),
        _ => unreachable!(),
    };
    apply_prepared(&[prepared]).expect("active block should apply");
    assert!(path.exists());

    let prepared = prepare_text_block(&inactive, Some(&active_receipt))
        .expect("disabled block should prepare");
    apply_prepared(&[prepared]).expect("disabled block should apply");
    assert!(!path.exists());
}

#[test]
fn managed_documents_detect_manual_changes() {
    let root = tempdir().expect("temporary directory should be created");
    let path = root.path().join("settings.yaml");
    let plan = TextBlockPlan {
        path: path.clone(),
        begin: "# begin".to_owned(),
        end: "# end".to_owned(),
        body: Some("value: managed".to_owned()),
        conflicting_keys: vec!["value:".to_owned()],
    };
    let prepared = prepare_text_block(&plan, None).expect("block should prepare");
    let receipt = match &prepared.receipt {
        DocumentReceipt::TextBlock(receipt) => receipt.clone(),
        _ => unreachable!(),
    };
    apply_prepared(&[prepared]).expect("block should apply");
    fs::write(&path, "# begin\nvalue: changed\n# end\n").expect("managed block should be changed");
    assert!(matches!(
        prepare_text_block(&plan, Some(&receipt)),
        Err(ConfigurationError::ManagedDocumentChanged(_))
    ));
}

#[test]
fn confirmation_paths_include_native_credentials_catalogs_and_defaults() {
    let root = tempdir().expect("temporary directory should be created");
    let state = root.path().join("state");
    let home = root.path().join("home");
    let manager = ConfigurationManager::new(&state, &home);

    let cases = [
        (HarnessKind::OpenCode, vec!["auth.json", "opencode.json"]),
        (HarnessKind::QwenCode, vec![".env", "settings.json"]),
        (
            HarnessKind::DeepSeekHarness,
            vec![".credentials.yaml", "settings.yaml"],
        ),
        (
            HarnessKind::Aider,
            vec![
                ".aider.conf.yml",
                ".aider.model.metadata.json",
                ".aider.model.settings.yml",
            ],
        ),
    ];

    for (harness, expected_names) in cases {
        let paths = manager
            .paths_for_search(harness, true)
            .expect("configuration paths should resolve");
        let names = paths
            .iter()
            .filter_map(|path| path.file_name())
            .filter_map(|name| name.to_str())
            .collect::<BTreeSet<_>>();
        for expected in expected_names {
            assert!(
                names.contains(expected),
                "{harness} confirmation omitted {expected}: {paths:?}"
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn newly_written_native_configuration_is_owner_only() {
    use std::os::unix::fs::PermissionsExt as _;

    let root = tempdir().expect("temporary directory should be created");
    let path = root.path().join("auth.json");
    let plan = JsonPlan {
        path: path.clone(),
        entries: vec![exclusive_json(&["nan"], json!({"key": "secret-value"}))],
    };
    let prepared = prepare_json(&plan, None).expect("configuration should prepare");
    apply_prepared(&[prepared]).expect("configuration should apply");
    assert_eq!(
        fs::metadata(path)
            .expect("configuration metadata should exist")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

fn test_models() -> Vec<CodingModelProfile> {
    vec![
        CodingModelProfile::generic("qwen3.6"),
        CodingModelProfile::generic("future-model"),
    ]
}

fn test_config() -> ResolvedConfig {
    ConfigResolver::resolve(
        &ProcessEnvironment,
        ConfigOverrides {
            provider_base_url: Some("https://api.nan.test/v1".to_owned()),
            nan_api_key: Some(
                SecretValue::new("secret-value").expect("test credential should be valid"),
            ),
        },
    )
    .expect("test configuration should resolve")
}

fn assert_persistent_search_contract(harness: HarnessKind, home: &Path) {
    let paths = match harness {
        HarnessKind::OpenCode => vec![home.join(".config/opencode/opencode.json")],
        HarnessKind::Hermes => vec![
            home.join(".hermes/config.yaml"),
            home.join(".hermes/plugins/web/nan_harness/provider.py"),
        ],
        HarnessKind::Pi => vec![home.join(".pi/agent/extensions/nan-search.js")],
        HarnessKind::Omp => vec![home.join(".omp/agent/extensions/nan-search.mjs")],
        HarnessKind::PrimeAgent => {
            vec![home.join(".prime/agent/extensions/nan-search.js")]
        }
        HarnessKind::DeepSeekHarness => vec![home.join(".dsh/cordis.patch.yml")],
        HarnessKind::OpenClaw => vec![
            home.join(".openclaw/openclaw.json"),
            home.join(".openclaw/extensions/nan-harness-search/index.js"),
        ],
        HarnessKind::Cline => {
            vec![home.join(".cline/data/settings/mcp_settings.json")]
        }
        HarnessKind::QwenCode => vec![home.join(".qwen/mcp.json")],
        HarnessKind::KimiCode => vec![home.join(".kimi-code/mcp.json")],
        HarnessKind::Goose => vec![home.join(".config/goose/config.yaml")],
        HarnessKind::Aider => {
            let config = fs::read_to_string(home.join(".aider.conf.yml"))
                .expect("Aider configuration should be readable");
            assert!(!config.contains("nan-search"));
            return;
        }
        HarnessKind::ClaudeCode | HarnessKind::Codex | HarnessKind::Fx => unreachable!(),
    };
    let combined = paths
        .iter()
        .map(|path| {
            fs::read_to_string(path)
                .unwrap_or_else(|error| panic!("{} should be readable: {error}", path.display()))
        })
        .collect::<Vec<_>>()
        .join("\n");
    if matches!(harness, HarnessKind::Pi | HarnessKind::PrimeAgent) {
        assert!(combined.contains("pi.getAllTools()"));
        assert!(combined.contains("getApiKeyForProvider(\"nan\")"));
        assert!(!combined.contains("secret-value"));
    } else if harness == HarnessKind::Omp {
        assert!(combined.contains("ctx.invokeTool"));
        assert!(combined.contains("getApiKey(\"nan\")"));
        assert!(combined.contains("hybridProviders"));
        assert!(!combined.contains("secret-value"));
    } else {
        assert!(
            combined.contains("nan-search"),
            "{harness} did not activate the managed search contract: {paths:?}"
        );
        assert!(
            combined.contains("__search-mcp")
                || matches!(harness, HarnessKind::Hermes | HarnessKind::OpenClaw),
            "{harness} did not use the direct search MCP contract"
        );
    }
}

fn receipt_path(receipt: &DocumentReceipt) -> &Path {
    match receipt {
        DocumentReceipt::Json(receipt) => &receipt.path,
        DocumentReceipt::Yaml(receipt) => &receipt.path,
        DocumentReceipt::TextBlock(receipt) => &receipt.path,
        DocumentReceipt::ExactFile(receipt) => &receipt.path,
        DocumentReceipt::Toml(receipt) => &receipt.path,
    }
}

fn json_receipt(document: &PreparedDocument) -> JsonReceipt {
    match &document.receipt {
        DocumentReceipt::Json(receipt) => receipt.clone(),
        _ => panic!("expected a JSON receipt"),
    }
}
