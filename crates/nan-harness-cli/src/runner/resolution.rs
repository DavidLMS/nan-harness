use super::models::LaunchModel;
#[allow(clippy::wildcard_imports)]
use super::*;

pub(super) fn required_config(
    config: Option<&commands::credentials::ResolvedLaunchConfig>,
) -> Result<&commands::credentials::ResolvedLaunchConfig, CliError> {
    config.ok_or(CliError::CredentialInvariant)
}

pub(super) fn generate_launch_id() -> Result<LaunchId, CliError> {
    let mut bytes = [0_u8; 12];
    getrandom::fill(&mut bytes).map_err(CliError::Random)?;
    let mut suffix = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut suffix, "{byte:02x}");
    }
    LaunchId::new(format!("launch_{suffix}")).map_err(CliError::InvalidPlan)
}

#[derive(Debug)]
pub(super) struct ExplicitModelResolution {
    pub(super) model: ResolvedModel,
    pub(super) catalog: Vec<CodingModelProfile>,
    pub(super) warning: Option<String>,
    pub(super) undiscovered: bool,
}

pub(super) fn offline_requested_model(model: &LaunchModel) -> Result<ResolvedModel, CliError> {
    let profile = valid_model_profile(&model.id)?;
    let warnings = (profile.source == ProfileSource::Generic)
        .then(|| {
            format!(
                "model '{}' has no bundled capability profile; using conservative defaults.",
                model.id
            )
        })
        .into_iter()
        .collect();
    Ok(resolved_model(
        model,
        &profile,
        ModelAvailability::Discovered,
        warnings,
    ))
}

pub(super) fn resolve_explicit_model(
    _kind: HarnessKind,
    model: &LaunchModel,
    discovered: &[CodingModelProfile],
) -> Result<ExplicitModelResolution, CliError> {
    let fallback_profile = valid_model_profile(&model.id)?;
    let live_profile = discovered.iter().find(|profile| profile.id == model.id);
    let profile = live_profile
        .cloned()
        .unwrap_or_else(|| fallback_profile.clone());
    let undiscovered = live_profile.is_none();
    let generic = known_coding_model(&model.id).is_none();
    let available = discovered
        .iter()
        .map(|profile| profile.id.clone())
        .collect::<Vec<_>>();
    let warning = explicit_model_warning(&model.id, generic, undiscovered, &available);
    let warnings = warning
        .as_deref()
        .and_then(|value| value.strip_prefix("warning: "))
        .map(str::to_owned)
        .into_iter()
        .collect();
    let mut catalog = discovered.to_vec();
    if undiscovered {
        catalog.push(fallback_profile);
    }
    Ok(ExplicitModelResolution {
        model: resolved_model(
            model,
            &profile,
            if undiscovered {
                ModelAvailability::ExplicitUndiscovered
            } else {
                ModelAvailability::Discovered
            },
            warnings,
        ),
        catalog,
        warning,
        undiscovered,
    })
}

pub(super) fn valid_model_profile(model: &str) -> Result<CodingModelProfile, CliError> {
    if !is_valid_provider_model_id(model) {
        return Err(invalid_model_error());
    }
    coding_model_profile(model).ok_or_else(invalid_model_error)
}

pub(super) fn invalid_model_error() -> CliError {
    CliError::InvalidPlan(PlanError::InvalidField {
        field: "model",
        message: "model ID is invalid".to_owned(),
    })
}

pub(super) fn resolved_model(
    model: &LaunchModel,
    profile: &CodingModelProfile,
    availability: ModelAvailability,
    warnings: Vec<String>,
) -> ResolvedModel {
    ResolvedModel {
        requested_id: model.id.clone(),
        resolved_id: model.id.clone(),
        reasoning_selection: model.reasoning,
        availability,
        profile_source: profile.source,
        qualification: if profile.source == ProfileSource::Bundled {
            QualificationStatus::Qualified
        } else {
            QualificationStatus::Unknown
        },
        warnings,
    }
}

pub(super) fn explicit_model_warning(
    model: &str,
    generic: bool,
    undiscovered: bool,
    available: &[String],
) -> Option<String> {
    let mut warning = match (generic, undiscovered) {
        (true, false) => format!(
            "warning: model '{model}' has no bundled capability profile; using conservative defaults."
        ),
        (false, true) => format!(
            "warning: model '{model}' was not returned by live discovery for this credential; attempting it because you selected it explicitly."
        ),
        (true, true) => format!(
            "warning: model '{model}' was not returned by live discovery and has no bundled capability profile; attempting it with conservative defaults because you selected it explicitly."
        ),
        (false, false) => return None,
    };
    if undiscovered && let Some(suggestion) = near_model_match(model, available) {
        let _ = write!(warning, " Did you mean '{suggestion}'?");
    }
    Some(warning)
}

pub(crate) fn near_model_match(requested: &str, available: &[String]) -> Option<String> {
    let requested = normalize_model_id(requested);
    if requested.is_empty() {
        return None;
    }
    let mut best: Option<(usize, &str)> = None;
    let mut tied = false;
    for candidate in available {
        let normalized = normalize_model_id(candidate);
        if normalized.is_empty() {
            continue;
        }
        let distance = edit_distance(requested.as_bytes(), normalized.as_bytes());
        match best {
            None => {
                best = Some((distance, candidate));
                tied = false;
            }
            Some((best_distance, _)) if distance < best_distance => {
                best = Some((distance, candidate));
                tied = false;
            }
            Some((best_distance, _)) if distance == best_distance => tied = true,
            Some(_) => {}
        }
    }
    let (distance, candidate) = best?;
    (!tied && distance.saturating_mul(4) <= requested.len()).then(|| candidate.to_owned())
}

pub(super) fn normalize_model_id(value: &str) -> String {
    value
        .bytes()
        .filter(u8::is_ascii_alphanumeric)
        .map(|byte| byte.to_ascii_lowercase())
        .map(char::from)
        .collect()
}

pub(super) fn edit_distance(left: &[u8], right: &[u8]) -> usize {
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    let mut current = vec![0; right.len() + 1];
    for (left_index, left_byte) in left.iter().enumerate() {
        current[0] = left_index + 1;
        for (right_index, right_byte) in right.iter().enumerate() {
            let substitution = previous[right_index] + usize::from(left_byte != right_byte);
            current[right_index + 1] = (current[right_index] + 1)
                .min(previous[right_index + 1] + 1)
                .min(substitution);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}
