use super::metadata::{CodingModelMetadata, KNOWN_CODING_MODELS};
use super::profile::{CodingModelProfile, ProfileSource};
use super::qualification::QualificationStatus;
use super::resolution::{ModelAvailability, ResolvedModel};
use crate::HarnessKind;
use std::collections::{BTreeMap, BTreeSet};

#[must_use]
pub fn known_coding_model(model_id: &str) -> Option<&'static CodingModelMetadata> {
    KNOWN_CODING_MODELS
        .iter()
        .find(|model| model.id == model_id)
}

#[must_use]
pub fn coding_model_profile(model_id: &str) -> Option<CodingModelProfile> {
    if !super::resolution::is_valid_provider_model_id(model_id)
        || super::resolution::is_known_non_coding_model(model_id)
    {
        return None;
    }
    Some(known_coding_model(model_id).map_or_else(
        || CodingModelProfile::generic(model_id),
        CodingModelProfile::from,
    ))
}

#[must_use]
pub fn coding_models_from_provider_ids(
    provider_ids: impl IntoIterator<Item = String>,
) -> Vec<CodingModelProfile> {
    let available = provider_ids
        .into_iter()
        .filter(|model_id| super::resolution::is_valid_provider_model_id(model_id))
        .collect::<BTreeSet<_>>();
    let mut models = KNOWN_CODING_MODELS
        .iter()
        .filter(|metadata| available.contains(metadata.id))
        .map(CodingModelProfile::from)
        .collect::<Vec<_>>();
    models.extend(
        available
            .into_iter()
            .filter(|model_id| known_coding_model(model_id).is_none())
            .filter_map(|model_id| coding_model_profile(&model_id)),
    );
    models
}

#[derive(Debug, Default)]
pub struct ModelCatalog {
    profiles: BTreeMap<String, super::schema::ModelProfile>,
}

impl ModelCatalog {
    #[must_use]
    pub fn new(profiles: impl IntoIterator<Item = super::schema::ModelProfile>) -> Self {
        Self {
            profiles: profiles
                .into_iter()
                .map(|profile| (profile.id.clone(), profile))
                .collect(),
        }
    }

    #[must_use]
    pub fn resolve_explicit(
        &self,
        requested_id: &str,
        harness: HarnessKind,
        discovered_ids: &BTreeSet<String>,
    ) -> ResolvedModel {
        let availability = if discovered_ids.contains(requested_id) {
            ModelAvailability::Discovered
        } else {
            ModelAvailability::ExplicitUndiscovered
        };

        let Some(profile) = self.profiles.get(requested_id) else {
            let mut warnings = vec![
                "This model has no bundled capability profile and will use conservative defaults."
                    .to_owned(),
            ];
            push_unique(&mut warnings, availability_warning(availability));
            return ResolvedModel {
                requested_id: requested_id.to_owned(),
                resolved_id: requested_id.to_owned(),
                reasoning_selection: None,
                availability,
                profile_source: ProfileSource::Generic,
                qualification: QualificationStatus::Unknown,
                warnings,
            };
        };

        let qualification = profile.qualification.for_harness(harness).status;
        let mut warnings = profile.warnings.clone();
        if availability == ModelAvailability::ExplicitUndiscovered {
            push_unique(&mut warnings, availability_warning(availability));
        }
        if qualification != QualificationStatus::Qualified {
            push_unique(
                &mut warnings,
                format!("Model '{requested_id}' is not qualified for {harness}."),
            );
        }

        ResolvedModel {
            requested_id: requested_id.to_owned(),
            resolved_id: profile.id.clone(),
            reasoning_selection: None,
            availability,
            profile_source: profile.source,
            qualification,
            warnings,
        }
    }
}

fn availability_warning(availability: ModelAvailability) -> String {
    match availability {
        ModelAvailability::Discovered => String::new(),
        ModelAvailability::ExplicitUndiscovered => {
            "The requested model was not returned by live discovery for this credential.".to_owned()
        }
    }
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !value.is_empty() && !values.contains(&value) {
        values.push(value);
    }
}
