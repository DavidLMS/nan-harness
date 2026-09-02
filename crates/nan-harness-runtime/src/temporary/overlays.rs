use super::TemporaryError;
use super::formats::{
    merge_json_objects, merge_toml_tables, merge_yaml_mappings, parse_json_object,
    parse_toml_table, parse_yaml_mapping, relocate_hook_state_keys,
};
use super::paths::{ensure_mode, invalid_artifact, path_exists, render_user_home};
use super::platform::{link_entry, restrict_directory};
use nan_harness_core::launch_plan::{
    ConfigurationOverlay, OverlayFilePolicy, TemporaryArtifactMode,
};
use nan_harness_private_fs::{create_private_dir, open_private_new};
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{ErrorKind, Write as _};
use std::path::{Component, Path, PathBuf};

pub(super) fn materialize_overlay(
    overlay: &ConfigurationOverlay,
    source: &Path,
    target: &Path,
    render: &impl Fn(&str, &str) -> Result<String, TemporaryError>,
    user_home: &Path,
) -> Result<(), TemporaryError> {
    let replacements = overlay
        .files
        .iter()
        .filter(|file| {
            file.policy != OverlayFilePolicy::Preserve || !path_exists(&source.join(&file.path))
        })
        .map(|file| PathBuf::from(&file.path))
        .collect::<BTreeSet<_>>();
    mirror_directory(source, target, Path::new(""), &replacements, &overlay.id)?;

    for file in &overlay.files {
        let path = target.join(&file.path);
        if file.policy == OverlayFilePolicy::Preserve && path_exists(&path) {
            continue;
        }
        let source_path = source.join(&file.path);
        if file.policy == OverlayFilePolicy::CopyBinary {
            if !path_exists(&source_path) {
                continue;
            }
            ensure_mode(&overlay.id, file.mode, TemporaryArtifactMode::OwnerFile)?;
            create_private_parents(target, path.parent(), &overlay.id)?;
            let mut source_file =
                File::open(&source_path).map_err(|source| TemporaryError::Materialize {
                    artifact_id: overlay.id.clone(),
                    source,
                })?;
            let mut target_file =
                open_private_new(&path).map_err(|source| TemporaryError::Materialize {
                    artifact_id: overlay.id.clone(),
                    source,
                })?;
            std::io::copy(&mut source_file, &mut target_file).map_err(|source| {
                TemporaryError::Materialize {
                    artifact_id: overlay.id.clone(),
                    source,
                }
            })?;
            continue;
        }
        ensure_mode(&overlay.id, file.mode, TemporaryArtifactMode::OwnerFile)?;
        create_private_parents(target, path.parent(), &overlay.id)?;
        let content = overlay_file_content(
            overlay,
            file,
            &source_path,
            &path,
            target,
            render,
            user_home,
        )?;
        let mut target_file =
            open_private_new(&path).map_err(|source| TemporaryError::Materialize {
                artifact_id: overlay.id.clone(),
                source,
            })?;
        target_file
            .write_all(content.as_bytes())
            .map_err(|source| TemporaryError::Materialize {
                artifact_id: overlay.id.clone(),
                source,
            })?;
    }
    Ok(())
}

fn overlay_file_content(
    overlay: &ConfigurationOverlay,
    file: &nan_harness_core::launch_plan::OverlayFile,
    source_path: &Path,
    target_path: &Path,
    overlay_path: &Path,
    render: &impl Fn(&str, &str) -> Result<String, TemporaryError>,
    user_home: &Path,
) -> Result<String, TemporaryError> {
    if file.policy == OverlayFilePolicy::Copy && path_exists(source_path) {
        return fs::read_to_string(source_path)
            .map_err(|source| overlay_error(&overlay.id, source));
    }
    let rendered = render(&overlay.id, &file.content_template)?;
    let rendered = render_user_home(&rendered, user_home);
    let overlay_placeholder = format!("{{artifact:{}}}", overlay.id);
    let rendered = rendered.replace(&overlay_placeholder, &overlay_path.to_string_lossy());
    if rendered.contains("{artifact:") {
        return Err(invalid_artifact(
            &overlay.id,
            "content contains an unresolved artifact placeholder",
        ));
    }
    match file.policy {
        OverlayFilePolicy::MergeJson => {
            let mut base = if path_exists(source_path) {
                let content = fs::read_to_string(source_path)
                    .map_err(|source| overlay_error(&overlay.id, source))?;
                parse_json_object(&overlay.id, "source", &content)?
            } else {
                serde_json::Map::new()
            };
            let patch = parse_json_object(&overlay.id, "patch", &rendered)?;
            merge_json_objects(&mut base, patch);
            serde_json::to_string_pretty(&serde_json::Value::Object(base)).map_err(|error| {
                invalid_artifact(
                    &overlay.id,
                    format!("could not serialize merged JSON overlay: {error}"),
                )
            })
        }
        OverlayFilePolicy::MergeToml => {
            let mut base = if path_exists(source_path) {
                let content = fs::read_to_string(source_path)
                    .map_err(|source| overlay_error(&overlay.id, source))?;
                parse_toml_table(&overlay.id, "source", &content)?
            } else {
                toml::Table::new()
            };
            let patch = parse_toml_table(&overlay.id, "patch", &rendered)?;
            merge_toml_tables(&mut base, patch);
            relocate_hook_state_keys(&mut base, source_path, target_path);
            toml::to_string(&toml::Value::Table(base)).map_err(|error| {
                invalid_artifact(
                    &overlay.id,
                    format!("could not serialize merged TOML overlay: {error}"),
                )
            })
        }
        OverlayFilePolicy::MergeYaml => {
            let mut base = if path_exists(source_path) {
                let content = fs::read_to_string(source_path)
                    .map_err(|source| overlay_error(&overlay.id, source))?;
                parse_yaml_mapping(&overlay.id, &content)?
            } else {
                serde_yaml_ng::Mapping::new()
            };
            let patch = parse_yaml_mapping(&overlay.id, &rendered)?;
            merge_yaml_mappings(&mut base, patch);
            serde_yaml_ng::to_string(&serde_yaml_ng::Value::Mapping(base))
                .map_err(|_| invalid_artifact(&overlay.id, "NH-TEMP-YAML-002"))
        }
        OverlayFilePolicy::Replace
        | OverlayFilePolicy::Preserve
        | OverlayFilePolicy::Copy
        | OverlayFilePolicy::CopyBinary => Ok(rendered),
    }
}

fn mirror_directory(
    source: &Path,
    target: &Path,
    relative: &Path,
    replacements: &BTreeSet<PathBuf>,
    overlay_id: &str,
) -> Result<(), TemporaryError> {
    fs::create_dir(target).map_err(|source| overlay_error(overlay_id, source))?;
    restrict_directory(target)?;

    let metadata = match fs::metadata(source) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(overlay_error(overlay_id, error)),
    };
    if !metadata.is_dir() {
        return Err(invalid_artifact(
            overlay_id,
            format!("overlay source '{}' is not a directory", source.display()),
        ));
    }

    let entries = fs::read_dir(source).map_err(|source| overlay_error(overlay_id, source))?;
    for entry in entries {
        let entry = entry.map_err(|source| overlay_error(overlay_id, source))?;
        let name = entry.file_name();
        let child_relative = relative.join(&name);
        let replaces_child = replacements.contains(&child_relative);
        let replaces_descendant = replacements.iter().any(|replacement| {
            replacement != &child_relative && replacement.starts_with(&child_relative)
        });
        if replaces_child {
            continue;
        }

        let child_target = target.join(&name);
        if replaces_descendant {
            mirror_directory(
                &entry.path(),
                &child_target,
                &child_relative,
                replacements,
                overlay_id,
            )?;
        } else {
            link_entry(&entry.path(), &child_target)
                .map_err(|source| overlay_error(overlay_id, source))?;
        }
    }
    Ok(())
}

fn create_private_parents(
    overlay_root: &Path,
    parent: Option<&Path>,
    overlay_id: &str,
) -> Result<(), TemporaryError> {
    let Some(parent) = parent else {
        return Ok(());
    };
    let relative = parent
        .strip_prefix(overlay_root)
        .map_err(|_| invalid_artifact(overlay_id, "overlay file escaped its temporary root"))?;
    let mut current = overlay_root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(invalid_artifact(
                overlay_id,
                "overlay file path contains an unsafe component",
            ));
        };
        current.push(name);
        match create_private_dir(&current) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                match fs::metadata(&current) {
                    Ok(metadata) if metadata.is_dir() => {}
                    Ok(_) => return Err(overlay_error(overlay_id, error)),
                    Err(source) => return Err(overlay_error(overlay_id, source)),
                }
            }
            Err(source) => return Err(overlay_error(overlay_id, source)),
        }
    }
    Ok(())
}

fn overlay_error(overlay_id: &str, source: std::io::Error) -> TemporaryError {
    TemporaryError::MirrorOverlay {
        overlay_id: overlay_id.to_owned(),
        source,
    }
}
