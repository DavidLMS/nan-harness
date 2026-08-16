use nan_harness_core::{LaunchPlan, SecretRef, SecretStore, SecretValue};
use static_assertions::assert_not_impl_any;
use std::fmt;

const DIRECT_PLAN: &str = include_str!("fixtures/launch-plan.direct.json");

assert_not_impl_any!(SecretValue: serde::Serialize, Clone, fmt::Display);

#[test]
fn secret_debug_output_is_redacted() {
    let value = SecretValue::new("nan-secret-value").expect("secret should be accepted");
    assert_eq!(format!("{value:?}"), "SecretValue([REDACTED])");
}

#[test]
fn secret_store_exposes_only_reference_names() {
    let reference = SecretRef::new("nan_api_key").expect("reference should be valid");
    let mut store = SecretStore::new();
    store.insert(
        reference,
        SecretValue::new("nan-secret-value").expect("secret should be accepted"),
    );

    let debug = format!("{store:?}");
    assert!(debug.contains("nan_api_key"));
    assert!(!debug.contains("nan-secret-value"));
}

#[test]
fn serialized_plans_contain_references_but_never_secret_values() {
    let plan: LaunchPlan = serde_json::from_str(DIRECT_PLAN).expect("fixture should match Rust");
    let serialized = serde_json::to_string(&plan).expect("plan should serialize");

    assert!(serialized.contains("nan_api_key"));
    assert!(!serialized.contains("nan-secret-value"));
}
