use super::placeholders::validate_template_placeholders;
use super::{invalid, unsafe_resource};
use crate::error::PlanError;
use crate::launch_plan::{
    ARTIFACT_PLACEHOLDER_PREFIX, CODEX_HOME_PLACEHOLDER, ConfigurationOverlay, LaunchPlan,
    TemporaryArtifact, TemporaryArtifactKind, TemporaryArtifactMode, USER_HOME_PLACEHOLDER,
};
use nan_harness_i18n::DiagnosticText;
use nan_harness_i18n::messages as detail_messages;
use std::collections::BTreeSet;
use std::path::{Component, Path};

pub(super) fn validate_artifacts(plan: &LaunchPlan) -> Result<(), PlanError> {
    let mut ids = BTreeSet::new();
    for artifact in &plan.temporary_artifacts {
        if !ids.insert(artifact.id.clone()) {
            return Err(PlanError::UnsafeTemporaryArtifact {
                artifact_id: artifact.id.clone(),
                reason: DiagnosticText::new(detail_messages::detail_artifact_ids_must_be_unique),
            });
        }
        if !is_valid_artifact_id(&artifact.id) {
            return unsafe_artifact(
                artifact,
                DiagnosticText::new(detail_messages::detail_id_must_match_a_z_a_z0_9_2_63),
            );
        }
        if !is_safe_path_hint(&artifact.path_hint) {
            return unsafe_artifact(
                artifact,
                DiagnosticText::new(
                    detail_messages::detail_pathhint_must_be_one_relative_path_component,
                ),
            );
        }
        match (artifact.kind, artifact.mode, &artifact.content_template) {
            (TemporaryArtifactKind::File, TemporaryArtifactMode::OwnerFile, Some(_))
            | (TemporaryArtifactKind::Directory, TemporaryArtifactMode::OwnerDirectory, None) => {}
            _ => {
                return unsafe_artifact(
                    artifact,
                    DiagnosticText::new(detail_messages::detail_files_require_mode_0600_and_content_directories_require_mode_0700_and_no_content),
                );
            }
        }
        validate_template_placeholders(plan, &artifact.id, artifact.content_template.as_deref())?;
    }

    ids.extend(
        plan.configuration_overlays
            .iter()
            .map(|overlay| overlay.id.clone()),
    );
    ids.extend(plan.launch_scoped_files.iter().map(|file| file.id.clone()));

    for (field, value) in plan
        .process
        .arguments
        .iter()
        .map(|value| ("process.arguments", value))
        .chain(
            plan.environment
                .public
                .values()
                .map(|value| ("environment.public", value)),
        )
    {
        let artifact_ids = artifact_placeholders(value).ok_or_else(|| PlanError::InvalidField {
            field,
            message: DiagnosticText::new(|locale| {
                detail_messages::detail_contains_malformed_artifact_placeholder_value(
                    locale,
                    &(value),
                )
            }),
        })?;
        for artifact_id in artifact_ids {
            if !ids.contains(artifact_id) {
                return invalid(
                    field,
                    DiagnosticText::new(|locale| {
                        detail_messages::detail_references_unknown_temporary_artifact_artifact_id(
                            locale,
                            &(artifact_id),
                        )
                    }),
                );
            }
        }
    }
    Ok(())
}

pub(super) fn validate_configuration_overlays(plan: &LaunchPlan) -> Result<(), PlanError> {
    let mut ids = plan
        .temporary_artifacts
        .iter()
        .map(|artifact| artifact.id.clone())
        .collect::<BTreeSet<_>>();
    for overlay in &plan.configuration_overlays {
        validate_overlay_identity(&mut ids, overlay)?;
        validate_overlay_paths(overlay)?;
        validate_overlay_files(plan, overlay)?;
    }
    Ok(())
}

fn validate_overlay_identity(
    ids: &mut BTreeSet<String>,
    overlay: &ConfigurationOverlay,
) -> Result<(), PlanError> {
    if !ids.insert(overlay.id.clone()) {
        return Err(PlanError::UnsafeTemporaryArtifact {
            artifact_id: overlay.id.clone(),
            reason: DiagnosticText::new(
                detail_messages::detail_temporary_resource_ids_must_be_unique,
            ),
        });
    }
    if !is_valid_artifact_id(&overlay.id) {
        return unsafe_resource(
            &overlay.id,
            DiagnosticText::new(detail_messages::detail_id_must_match_a_z_a_z0_9_2_63),
        );
    }
    Ok(())
}

fn validate_overlay_paths(overlay: &ConfigurationOverlay) -> Result<(), PlanError> {
    if !is_safe_path_hint(&overlay.path_hint) {
        return unsafe_resource(
            &overlay.id,
            DiagnosticText::new(
                detail_messages::detail_pathhint_must_be_one_relative_path_component,
            ),
        );
    }
    if !is_safe_user_home_path(&overlay.source_path) {
        return unsafe_resource(
            &overlay.id,
            DiagnosticText::new(detail_messages::detail_sourcepath_must_use_an_approved_runtime_home_or_a_safe_user_home_path),
        );
    }
    Ok(())
}

fn validate_overlay_files(
    plan: &LaunchPlan,
    overlay: &ConfigurationOverlay,
) -> Result<(), PlanError> {
    let mut paths = BTreeSet::new();
    for file in &overlay.files {
        validate_overlay_file_path(&mut paths, &overlay.id, &file.path)?;
        if file.mode != TemporaryArtifactMode::OwnerFile {
            return unsafe_resource(
                &overlay.id,
                DiagnosticText::new(detail_messages::detail_overlay_files_require_mode_0600),
            );
        }
        validate_template_placeholders(plan, &overlay.id, Some(&file.content_template))?;
    }
    Ok(())
}

fn validate_overlay_file_path(
    paths: &mut BTreeSet<String>,
    overlay_id: &str,
    path: &str,
) -> Result<(), PlanError> {
    if !is_safe_relative_path(path) {
        return unsafe_resource(
            overlay_id,
            DiagnosticText::new(
                detail_messages::detail_overlay_file_paths_must_be_relative_and_safe,
            ),
        );
    }
    let file_path = Path::new(path);
    if paths.iter().any(|existing: &String| {
        let existing_path = Path::new(existing);
        existing_path.starts_with(file_path) || file_path.starts_with(existing_path)
    }) {
        return unsafe_resource(
            overlay_id,
            DiagnosticText::new(
                detail_messages::detail_overlay_file_paths_cannot_contain_one_another,
            ),
        );
    }
    if !paths.insert(path.to_owned()) {
        return unsafe_resource(
            overlay_id,
            DiagnosticText::new(detail_messages::detail_overlay_file_paths_must_be_unique),
        );
    }
    Ok(())
}

pub(super) fn validate_launch_scoped_files(plan: &LaunchPlan) -> Result<(), PlanError> {
    let mut ids = plan
        .temporary_artifacts
        .iter()
        .map(|artifact| artifact.id.clone())
        .chain(
            plan.configuration_overlays
                .iter()
                .map(|overlay| overlay.id.clone()),
        )
        .collect::<BTreeSet<_>>();
    let mut paths = BTreeSet::new();
    for file in &plan.launch_scoped_files {
        if !ids.insert(file.id.clone()) {
            return Err(PlanError::UnsafeTemporaryArtifact {
                artifact_id: file.id.clone(),
                reason: DiagnosticText::new(
                    detail_messages::detail_temporary_resource_ids_must_be_unique,
                ),
            });
        }
        if !is_valid_artifact_id(&file.id) {
            return unsafe_resource(
                &file.id,
                DiagnosticText::new(detail_messages::detail_id_must_match_a_z_a_z0_9_2_63),
            );
        }
        if !is_safe_user_home_path(&file.directory) {
            return unsafe_resource(
                &file.id,
                DiagnosticText::new(detail_messages::detail_directory_must_use_an_approved_runtime_home_or_a_safe_user_home_path),
            );
        }
        if !is_safe_path_hint(&file.file_name) {
            return unsafe_resource(
                &file.id,
                DiagnosticText::new(
                    detail_messages::detail_filename_must_be_one_relative_path_component,
                ),
            );
        }
        if !file.ownership_prefix.starts_with("nan-harness-")
            || !file.file_name.starts_with(&file.ownership_prefix)
            || !is_safe_path_hint(&file.ownership_prefix)
        {
            return unsafe_resource(
                &file.id,
                DiagnosticText::new(
                    detail_messages::detail_ownershipprefix_must_use_a_safe_nan_harness_namespace,
                ),
            );
        }
        if file.mode != TemporaryArtifactMode::OwnerFile {
            return unsafe_resource(
                &file.id,
                DiagnosticText::new(detail_messages::detail_launch_scoped_files_require_mode_0600),
            );
        }
        if !paths.insert((file.directory.clone(), file.file_name.clone())) {
            return unsafe_resource(
                &file.id,
                DiagnosticText::new(
                    detail_messages::detail_launch_scoped_file_paths_must_be_unique,
                ),
            );
        }
        validate_template_placeholders(plan, &file.id, Some(&file.content_template))?;
    }
    Ok(())
}

fn artifact_placeholders(mut value: &str) -> Option<Vec<&str>> {
    let mut placeholders = Vec::new();
    while let Some(start) = value.find(ARTIFACT_PLACEHOLDER_PREFIX) {
        let remainder = &value[start + ARTIFACT_PLACEHOLDER_PREFIX.len()..];
        let end = remainder.find('}')?;
        let artifact_id = &remainder[..end];
        if artifact_id.is_empty() || artifact_id.contains(['{', '}']) {
            return None;
        }
        placeholders.push(artifact_id);
        value = &remainder[end + 1..];
    }
    Some(placeholders)
}

fn unsafe_artifact(
    artifact: &TemporaryArtifact,
    reason: impl Into<DiagnosticText>,
) -> Result<(), PlanError> {
    unsafe_resource(&artifact.id, reason)
}

fn is_valid_artifact_id(value: &str) -> bool {
    let mut characters = value.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    (3..=64).contains(&value.len())
        && first.is_ascii_lowercase()
        && characters.all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || character == '_'
                || character == '-'
        })
}

fn is_safe_user_home_path(value: &str) -> bool {
    if matches!(value, USER_HOME_PLACEHOLDER | CODEX_HOME_PLACEHOLDER) {
        return true;
    }
    value
        .strip_prefix(USER_HOME_PLACEHOLDER)
        .and_then(|suffix| suffix.strip_prefix('/'))
        .is_some_and(is_safe_relative_path)
}

fn is_safe_relative_path(value: &str) -> bool {
    let path = Path::new(value);
    !value.is_empty()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn is_safe_path_hint(value: &str) -> bool {
    let mut components = Path::new(value).components();
    matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
}
