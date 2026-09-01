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
    configure_profile_search(profile, base_url, web_search_enabled)
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
    let mut document: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&contents).map_err(HermesDesktopError::ParseProfileConfig)?;
    let original = document.clone();
    if enabled {
        let template = files
            .iter()
            .find(|file| file.path == "config.yaml")
            .ok_or(HermesDesktopError::MissingSearchTemplate)?
            .content_template
            .replace(nan_harness_core::launch_plan::NAN_SEARCH_BLOCK_BEGIN, "")
            .replace(nan_harness_core::launch_plan::NAN_SEARCH_BLOCK_END, "");
        let template_patch: serde_yaml_ng::Value =
            serde_yaml_ng::from_str(&template).map_err(HermesDesktopError::ParseProfileConfig)?;
        merge_yaml_value(&mut document, template_patch);
    } else {
        remove_managed_search(&mut document);
    }
    if document == original {
        return Ok(());
    }
    let rendered =
        serde_yaml_ng::to_string(&document).map_err(HermesDesktopError::SerializeProfileConfig)?;
    write_private(&config_path, rendered.as_bytes())
}

pub(crate) fn merge_yaml_value(base: &mut serde_yaml_ng::Value, patch: serde_yaml_ng::Value) {
    match (base, patch) {
        (serde_yaml_ng::Value::Mapping(base), serde_yaml_ng::Value::Mapping(patch)) => {
            for (key, value) in patch {
                if let Some(existing) = base.get_mut(&key) {
                    merge_yaml_value(existing, value);
                } else {
                    base.insert(key, value);
                }
            }
        }
        (serde_yaml_ng::Value::Sequence(base), serde_yaml_ng::Value::Sequence(patch)) => {
            for value in patch {
                if !base.contains(&value) {
                    base.push(value);
                }
            }
        }
        (base, patch) => *base = patch,
    }
}

pub(crate) fn remove_managed_search(document: &mut serde_yaml_ng::Value) {
    let serde_yaml_ng::Value::Mapping(root) = document else {
        return;
    };
    let plugins = serde_yaml_ng::Value::String("plugins".to_owned());
    let enabled = serde_yaml_ng::Value::String("enabled".to_owned());
    if let Some(serde_yaml_ng::Value::Mapping(plugins)) = root.get_mut(&plugins)
        && let Some(serde_yaml_ng::Value::Sequence(values)) = plugins.get_mut(&enabled)
    {
        values.retain(|value| value.as_str() != Some("web/nan_harness"));
    }
    let web = serde_yaml_ng::Value::String("web".to_owned());
    let backend = serde_yaml_ng::Value::String("search_backend".to_owned());
    if let Some(serde_yaml_ng::Value::Mapping(web)) = root.get_mut(&web)
        && web.get(&backend).and_then(serde_yaml_ng::Value::as_str) == Some("nan-harness")
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
