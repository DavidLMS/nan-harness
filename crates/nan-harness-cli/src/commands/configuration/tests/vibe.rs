use super::*;

const USER_CONFIG: &str = r#"# Keep my preferences.
active_model = "mistral-large"  # my default
enable_telemetry = true

[[providers]]
name = "mistral"
api_base = "https://api.mistral.ai/v1"
api_key_env_var = "MISTRAL_API_KEY"

[[models]]
name = "mistral-large"
provider = "mistral"
alias = "mistral-large"

[[models]]
name = "qwen3.6"
provider = "local"
alias = "local-qwen"

[[mcp_servers]]
name = "user-search"
transport = "stdio"
command = "user-search"

[tools.bash]
permission = "ask"
"#;

const USER_ENV: &str = "# my keys\nMISTRAL_API_KEY=\"user-secret\"\nOTHER=value\n";

fn paths(manager: &ConfigurationManager) -> (PathBuf, PathBuf) {
    (
        manager.paths.vibe_directory.join("config.toml"),
        manager.paths.vibe_directory.join(".env"),
    )
}

fn parse(path: &Path) -> Value {
    toml_edit::de::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

fn names(config: &Value, array: &str) -> Vec<String> {
    config[array]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["name"].as_str().unwrap().to_owned())
        .collect()
}

fn configure(manager: &ConfigurationManager, policy: WebSearchPolicy) {
    manager
        .configure(
            HarnessKind::MistralVibe,
            &test_config(),
            &test_models(),
            Some(policy),
        )
        .unwrap();
}

fn seeded(root: &Path) -> ConfigurationManager {
    let manager = ConfigurationManager::new(&root.join("state"), &root.join("home"));
    let (config, env) = paths(&manager);
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    fs::write(config, USER_CONFIG).unwrap();
    fs::write(env, USER_ENV).unwrap();
    manager
}

#[test]
fn vibe_configure_refresh_rotation_and_removal_preserve_user_settings() {
    let root = tempdir().unwrap();
    let manager = seeded(root.path());
    let (config_path, env_path) = paths(&manager);

    configure(&manager, WebSearchPolicy::Force);
    assert!(manager.is_active(HarnessKind::MistralVibe).unwrap());
    let config = parse(&config_path);
    assert_eq!(config["active_model"].as_str(), Some("qwen3.6"));
    assert_eq!(config["enable_telemetry"].as_bool(), Some(true));
    assert_eq!(names(&config, "providers"), ["mistral", "nan"]);
    assert_eq!(
        names(&config, "models"),
        ["mistral-large", "qwen3.6", "qwen3.6", "future-model"]
    );
    assert_eq!(names(&config, "mcp_servers"), ["user-search", "nan-search"]);
    let provider = &config["providers"][1];
    assert_eq!(
        provider["api_base"].as_str(),
        Some("https://api.nan.test/v1")
    );
    assert_eq!(
        provider["api_key_env_var"].as_str(),
        Some("NAN_VIBE_API_KEY")
    );
    assert_eq!(provider["api_style"].as_str(), Some("openai"));
    assert_eq!(provider["backend"].as_str(), Some("generic"));
    assert_eq!(
        provider["reasoning_field_name"].as_str(),
        Some("reasoning_content")
    );
    let model = &config["models"][2];
    assert_eq!(model["provider"].as_str(), Some("nan"));
    assert_eq!(model["thinking"].as_str(), Some("off"));
    let search = &config["mcp_servers"][1];
    assert_eq!(search["command"].as_str(), Some("nan-harness"));
    assert_eq!(search["args"][0].as_str(), Some("__search-mcp"));
    assert_eq!(config["tools"]["bash"]["permission"].as_str(), Some("ask"));
    let rendered = fs::read_to_string(&config_path).unwrap();
    assert!(rendered.contains("# Keep my preferences."));
    assert!(rendered.contains("# my default"));

    let env = fs::read_to_string(&env_path).unwrap();
    assert!(env.starts_with(USER_ENV));
    assert!(env.contains("NAN_VIBE_API_KEY=\"secret-value\""));
    assert!(!rendered.contains("secret-value"));
    assert!(
        !fs::read_to_string(&manager.paths.state_path)
            .unwrap()
            .contains("secret-value")
    );

    // Refresh with a rotated key, a new base URL, new models and search disabled.
    let rotated = ConfigResolver::resolve(
        &ProcessEnvironment,
        ConfigOverrides {
            provider_base_url: Some("https://rotated.nan.test/v1".to_owned()),
            nan_api_key: Some(SecretValue::new("rotated-key").unwrap()),
        },
    )
    .unwrap();
    manager
        .configure(
            HarnessKind::MistralVibe,
            &rotated,
            &[CodingModelProfile::generic("replacement")],
            Some(WebSearchPolicy::Disabled),
        )
        .unwrap();
    assert!(manager.is_active(HarnessKind::MistralVibe).unwrap());
    let config = parse(&config_path);
    assert_eq!(config["active_model"].as_str(), Some("replacement"));
    assert_eq!(names(&config, "providers"), ["mistral", "nan"]);
    assert_eq!(
        names(&config, "models"),
        ["mistral-large", "qwen3.6", "replacement"]
    );
    assert_eq!(names(&config, "mcp_servers"), ["user-search"]);
    assert_eq!(
        config["providers"][1]["api_base"].as_str(),
        Some("https://rotated.nan.test/v1")
    );
    let env = fs::read_to_string(&env_path).unwrap();
    assert!(env.contains("NAN_VIBE_API_KEY=\"rotated-key\""));
    assert!(!env.contains("secret-value"));
    assert!(env.starts_with(USER_ENV));

    // A no-op refresh reports no change and keeps bytes stable.
    let before = fs::read(&config_path).unwrap();
    let change = manager
        .configure(
            HarnessKind::MistralVibe,
            &rotated,
            &[CodingModelProfile::generic("replacement")],
            Some(WebSearchPolicy::Disabled),
        )
        .unwrap();
    assert!(!change.changed);
    assert_eq!(fs::read(&config_path).unwrap(), before);

    assert_eq!(
        manager.remove_all().unwrap(),
        vec![(HarnessKind::MistralVibe, RemovalOutcome::Removed)]
    );
    assert_eq!(fs::read_to_string(&config_path).unwrap(), USER_CONFIG);
    assert_eq!(fs::read_to_string(&env_path).unwrap(), USER_ENV);
    assert!(!manager.is_configured(HarnessKind::MistralVibe).unwrap());
}

#[test]
fn vibe_removal_keeps_entries_the_user_added_and_deletes_files_it_created() {
    let root = tempdir().unwrap();
    let manager = ConfigurationManager::new(&root.path().join("state"), &root.path().join("home"));
    let (config_path, env_path) = paths(&manager);
    configure(&manager, WebSearchPolicy::Force);
    assert!(config_path.exists() && env_path.exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        for path in [&config_path, &env_path] {
            let mode = fs::metadata(path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode & 0o077, 0, "{} must be owner-only", path.display());
        }
    }
    manager.remove(HarnessKind::MistralVibe).unwrap();
    assert!(!config_path.exists());
    assert!(!env_path.exists());

    configure(&manager, WebSearchPolicy::Disabled);
    let mut text = fs::read_to_string(&config_path).unwrap();
    text.push_str("\n[[models]]\nname = \"later\"\nprovider = \"other\"\nalias = \"later\"\n");
    fs::write(&config_path, text).unwrap();
    fs::write(
        &env_path,
        format!("LATER=1\n{}", fs::read_to_string(&env_path).unwrap()),
    )
    .unwrap();
    assert!(manager.is_active(HarnessKind::MistralVibe).unwrap());
    manager.remove(HarnessKind::MistralVibe).unwrap();
    let config = parse(&config_path);
    assert_eq!(names(&config, "models"), ["later"]);
    assert!(config.get("providers").is_none());
    assert!(config.get("active_model").is_none());
    assert_eq!(fs::read_to_string(&env_path).unwrap(), "LATER=1\n");
}

#[test]
fn vibe_conflicts_and_user_edits_prevent_publication() {
    for conflict in [
        "provider",
        "model-provider",
        "model-alias",
        "mcp",
        "env",
        "malformed",
        "inline-array",
        "edited-active-model",
        "edited-provider",
    ] {
        let root = tempdir().unwrap();
        let manager =
            ConfigurationManager::new(&root.path().join("state"), &root.path().join("home"));
        let (config_path, env_path) = paths(&manager);
        fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        let write = |path: &Path, contents: &str| fs::write(path, contents).unwrap();
        match conflict {
            "provider" => write(&config_path, "[[providers]]\nname = \"nan\"\n"),
            "model-provider" => write(
                &config_path,
                "[[models]]\nname = \"mine\"\nprovider = \"nan\"\nalias = \"mine\"\n",
            ),
            "model-alias" => write(
                &config_path,
                "[[models]]\nname = \"other\"\nprovider = \"x\"\nalias = \"qwen3.6\"\n",
            ),
            "mcp" => write(&config_path, "[[mcp_servers]]\nname = \"nan-search\"\n"),
            "env" => write(&env_path, "NAN_VIBE_API_KEY=user-owned\n"),
            "malformed" => write(&config_path, "[[providers\nprivate-sentinel"),
            "inline-array" => write(&config_path, "providers = []\n"),
            _ => {
                configure(&manager, WebSearchPolicy::Disabled);
                let configured = fs::read_to_string(&config_path).unwrap();
                let edited = if conflict == "edited-active-model" {
                    configured.replace("active_model = \"qwen3.6\"", "active_model = \"mine\"")
                } else {
                    configured.replace("https://api.nan.test/v1", "https://elsewhere.test/v1")
                };
                assert_ne!(edited, configured);
                write(&config_path, &edited);
                assert_eq!(
                    manager.inspect(HarnessKind::MistralVibe).unwrap(),
                    Some(ConfigurationHealth::Changed)
                );
            }
        }
        let snapshot = |manager: &ConfigurationManager| {
            [&config_path, &env_path, &manager.paths.state_path].map(|path| fs::read(path).ok())
        };
        let before = snapshot(&manager);
        let result = manager.configure(
            HarnessKind::MistralVibe,
            &test_config(),
            &test_models(),
            Some(WebSearchPolicy::Force),
        );
        let error = result.expect_err(conflict).to_string();
        assert!(!error.contains("secret-value"), "{conflict}: {error}");
        if conflict.starts_with("edited") {
            assert!(manager.remove(HarnessKind::MistralVibe).is_err());
        }
        assert_eq!(snapshot(&manager), before, "{conflict}");
    }
}

#[test]
fn vibe_home_must_be_absolute_and_leaves_other_harnesses_usable() {
    let root = tempdir().unwrap();
    let mut manager = ConfigurationManager::new(&root.path().join("state"), root.path());
    manager.paths.vibe_directory = PathBuf::from("relative/vibe");
    assert!(
        manager
            .configure(
                HarnessKind::MistralVibe,
                &test_config(),
                &test_models(),
                Some(WebSearchPolicy::Disabled),
            )
            .is_err()
    );
    assert!(!manager.is_configured(HarnessKind::MistralVibe).unwrap());
}

#[test]
fn vibe_confirmation_paths_list_the_config_and_credential_files() {
    let root = tempdir().unwrap();
    let manager = ConfigurationManager::new(&root.path().join("state"), root.path());
    let paths = manager
        .paths_for_search(HarnessKind::MistralVibe, true)
        .unwrap();
    let names = paths
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(names, [".env", "config.toml"]);
}
