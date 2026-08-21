#![forbid(unsafe_code)]

pub mod adapter;
pub mod error;
pub mod harness;
pub mod launch_plan;
pub mod model;
pub mod secret;

pub use adapter::{HarnessAdapter, PlanContext, build_validated_plan};
pub use error::{ErrorCategory, PlanError};
pub use harness::{
    CompatibilityManifest, DetectedHarness, HarnessCapability, HarnessCompatibility, HarnessKind,
    RuntimeCompatibility, VersionStatus,
};
pub use launch_plan::{LaunchPlan, LaunchPlanValidator, TransportKind};
pub use model::{
    CLAUDE_AUTO_MODE_COMPATIBILITY_ALIAS, CLAUDE_AUTO_MODE_PROVIDER_MODEL_ID, CodingModelMetadata,
    CodingModelProfile, GENERIC_CODING_MODEL_CONTEXT_WINDOW, GENERIC_CODING_MODEL_DESCRIPTION,
    GENERIC_CODING_MODEL_MAX_OUTPUT_TOKENS, KNOWN_CODING_MODELS, KNOWN_NON_CODING_MODELS,
    ModelAvailability, ModelCatalog, ModelProfile, ProfileSource, QualificationStatus,
    ReasoningEffort, ReasoningParameter, ReasoningPolicy, ReasoningSelection, ResolvedModel,
    claude_gateway_model_id, coding_model_profile, coding_models_from_provider_ids,
    is_known_non_coding_model, is_valid_provider_model_id, known_coding_model,
};
pub use secret::{SecretError, SecretRef, SecretStore, SecretValue};
