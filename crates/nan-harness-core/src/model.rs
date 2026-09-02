mod catalog;
mod metadata;
mod profile;
mod qualification;
mod reasoning;
mod resolution;
mod schema;

#[cfg(test)]
mod tests;

pub use catalog::{
    ModelCatalog, coding_model_profile, coding_models_from_provider_ids, known_coding_model,
};
pub use metadata::{
    CLAUDE_AUTO_MODE_COMPATIBILITY_ALIAS, CLAUDE_AUTO_MODE_PROVIDER_MODEL_ID,
    CLAUDE_GATEWAY_MODEL_PREFIX, CodingModelMetadata, GENERIC_CODING_MODEL_CONTEXT_WINDOW,
    GENERIC_CODING_MODEL_DESCRIPTION, GENERIC_CODING_MODEL_MAX_OUTPUT_TOKENS, KNOWN_CODING_MODELS,
    KNOWN_NON_CODING_MODELS,
};
pub use profile::{CodingModelProfile, ProfileSource};
pub use qualification::{
    ModelQualification, QualificationMatrix, QualificationStatus, QualificationTransport,
};
pub use reasoning::{
    ReasoningEffort, ReasoningHint, ReasoningParameter, ReasoningPolicy, ReasoningSelection,
};
pub use resolution::{
    ModelAvailability, ResolvedModel, claude_gateway_model_id, is_known_non_coding_model,
    is_valid_provider_model_id,
};
pub use schema::{
    ChatMaxTokensField, InputModality, ModelCapabilities, ModelCompatibility, ModelLimits,
    ModelProfile,
};
