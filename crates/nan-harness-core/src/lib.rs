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
    CompatibilityManifest, DetectedHarness, HarnessCompatibility, HarnessKind, VersionStatus,
};
pub use launch_plan::{LaunchPlan, LaunchPlanValidator, TransportKind};
pub use model::{
    CLAUDE_AUTO_MODE_COMPATIBILITY_ALIAS, CLAUDE_AUTO_MODE_PROVIDER_MODEL_ID, ModelAvailability,
    ModelCatalog, ModelProfile, ProfileSource, QualificationStatus, ResolvedModel,
    claude_gateway_model_id,
};
pub use secret::{SecretError, SecretRef, SecretStore, SecretValue};
