use super::*;

pub(crate) fn write_profile_config(
    profile: &Path,
    base_url: &str,
    models: &[CodingModelProfile],
    selected_model: &str,
    web_search_enabled: bool,
) -> Result<(), HermesDesktopError> {
    let path = profile.join("config.yaml");
    reject_profile_symlink(&path)?;
    let existing = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == ErrorKind::NotFound => String::new(),
        Err(error) => return Err(HermesDesktopError::ReadProfileConfig(error)),
    };
    let model_block = format!(
        "model:\n  default: {}\n  provider: nan",
        yaml_string(selected_model)
    );
    let provider_block = render_hermes_desktop_provider_block(base_url, models, selected_model);
    let with_model = replace_top_level_block(&existing, "model", &model_block)?;
    let updated = replace_provider_entry(&with_model, "nan", &provider_block)?;
    write_private(&path, updated.as_bytes())?;
    configure_profile_reasoning(profile, base_url, models)?;
    configure_profile_search(profile, base_url, web_search_enabled)
}

fn configure_profile_reasoning(
    profile: &Path,
    base_url: &str,
    models: &[CodingModelProfile],
) -> Result<(), HermesDesktopError> {
    let source = nan_harness_adapters::render_hermes_model_provider(base_url, models);
    for (relative, content) in [
        ("plugins/model-providers/nan/__init__.py", source.as_str()),
        (
            "plugins/model-providers/nan/plugin.yaml",
            "name: nan-provider\nkind: model-provider\nversion: 1.0.0\ndescription: NaN model access\nauthor: NaN\n",
        ),
    ] {
        let path = checked_profile_path(profile, relative)?;
        let parent = path
            .parent()
            .ok_or(HermesDesktopError::InvalidProfilePath)?;
        nan_harness_private_fs::create_private_dir_all(parent)
            .map_err(HermesDesktopError::ProtectProfile)?;
        write_private(&path, content.as_bytes())?;
    }
    Ok(())
}

pub(crate) fn apply_context_override(
    profile: &Path,
    context_limit: Option<&ContextLimit>,
) -> Result<(), HermesDesktopError> {
    let Some(NativeContextLimit::HermesThreshold { threshold_tokens }) =
        context_limit.map(|limit| &limit.native)
    else {
        return Ok(());
    };
    let path = profile.join("config.yaml");
    let contents = fs::read_to_string(&path).map_err(HermesDesktopError::ReadProfileConfig)?;
    let mut document: yaml_serde::Value =
        yaml_serde::from_str(&contents).map_err(HermesDesktopError::ParseProfileConfig)?;
    merge_yaml_value(
        &mut document,
        yaml_serde::from_str(&format!(
            "compression:\n  enabled: true\n  threshold_tokens: {threshold_tokens}\n"
        ))
        .map_err(HermesDesktopError::ParseProfileConfig)?,
    );
    let rendered =
        yaml_serde::to_string(&document).map_err(HermesDesktopError::SerializeProfileConfig)?;
    write_private(&path, rendered.as_bytes())
}

pub(crate) fn configure_profile_search(
    profile: &Path,
    base_url: &str,
    enabled: bool,
) -> Result<(), HermesDesktopError> {
    let bridge_base_url = base_url
        .trim_end_matches('/')
        .strip_suffix("/v1")
        .unwrap_or(base_url);
    let files = hermes_search_provider_files();
    for file in files.iter().filter(|file| file.path != "config.yaml") {
        if !enabled {
            continue;
        }
        let path = checked_profile_path(profile, &file.path)?;
        let parent = path
            .parent()
            .ok_or(HermesDesktopError::InvalidProfilePath)?;
        nan_harness_private_fs::create_private_dir_all(parent)
            .map_err(HermesDesktopError::ProtectProfile)?;
        let rendered = file.content_template.replace(
            nan_harness_core::launch_plan::BRIDGE_BASE_URL_PLACEHOLDER,
            bridge_base_url,
        );
        write_private(&path, rendered.as_bytes())?;
    }

    let config_path = profile.join("config.yaml");
    let contents =
        fs::read_to_string(&config_path).map_err(HermesDesktopError::ReadProfileConfig)?;
    let mut document: yaml_serde::Value =
        yaml_serde::from_str(&contents).map_err(HermesDesktopError::ParseProfileConfig)?;
    let original = document.clone();
    if enabled {
        let template = files
            .iter()
            .find(|file| file.path == "config.yaml")
            .ok_or(HermesDesktopError::MissingSearchTemplate)?
            .content_template
            .replace(nan_harness_core::launch_plan::NAN_SEARCH_BLOCK_BEGIN, "")
            .replace(nan_harness_core::launch_plan::NAN_SEARCH_BLOCK_END, "");
        let template_patch: yaml_serde::Value =
            yaml_serde::from_str(&template).map_err(HermesDesktopError::ParseProfileConfig)?;
        merge_yaml_value(&mut document, template_patch);
    } else {
        remove_managed_search(&mut document);
    }
    if document == original {
        return Ok(());
    }
    let rendered =
        yaml_serde::to_string(&document).map_err(HermesDesktopError::SerializeProfileConfig)?;
    write_private(&config_path, rendered.as_bytes())
}

pub(crate) fn merge_yaml_value(base: &mut yaml_serde::Value, patch: yaml_serde::Value) {
    match (base, patch) {
        (yaml_serde::Value::Mapping(base), yaml_serde::Value::Mapping(patch)) => {
            for (key, value) in patch {
                if let Some(existing) = base.get_mut(&key) {
                    merge_yaml_value(existing, value);
                } else {
                    base.insert(key, value);
                }
            }
        }
        (yaml_serde::Value::Sequence(base), yaml_serde::Value::Sequence(patch)) => {
            for value in patch {
                if !base.contains(&value) {
                    base.push(value);
                }
            }
        }
        (base, patch) => *base = patch,
    }
}

pub(crate) fn remove_managed_search(document: &mut yaml_serde::Value) {
    let yaml_serde::Value::Mapping(root) = document else {
        return;
    };
    let plugins = yaml_serde::Value::String("plugins".to_owned());
    let enabled = yaml_serde::Value::String("enabled".to_owned());
    if let Some(yaml_serde::Value::Mapping(plugins)) = root.get_mut(&plugins)
        && let Some(yaml_serde::Value::Sequence(values)) = plugins.get_mut(&enabled)
    {
        values.retain(|value| value.as_str() != Some("web/nan_harness"));
    }
    let web = yaml_serde::Value::String("web".to_owned());
    let backend = yaml_serde::Value::String("search_backend".to_owned());
    if let Some(yaml_serde::Value::Mapping(web)) = root.get_mut(&web)
        && web.get(&backend).and_then(yaml_serde::Value::as_str) == Some("nan-harness")
    {
        web.remove(&backend);
    }
}

pub(crate) fn reject_profile_symlink(path: &Path) -> Result<(), HermesDesktopError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(HermesDesktopError::UnsafePluginPath)
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(HermesDesktopError::ReadProfileConfig(error)),
    }
}

pub(crate) fn checked_profile_path(
    profile: &Path,
    relative: &str,
) -> Result<PathBuf, HermesDesktopError> {
    let mut path = profile.to_path_buf();
    for component in Path::new(relative).components() {
        let std::path::Component::Normal(component) = component else {
            return Err(HermesDesktopError::InvalidProfilePath);
        };
        path.push(component);
        reject_profile_symlink(&path)?;
    }
    Ok(path)
}
