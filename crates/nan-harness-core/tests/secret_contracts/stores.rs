use nan_harness_core::{SecretError, SecretRef, SecretStore, SecretValue};

const FIRST_VALUE: &str = "synthetic-secret-value-a";
const SECOND_VALUE: &str = "synthetic-secret-value-b";

#[test]
fn secret_values_require_content_and_redact_debug_output() {
    let value = SecretValue::new(FIRST_VALUE).expect("non-empty secret should be accepted");

    let observed = value.with_secret(str::to_owned);

    assert_eq!(observed, FIRST_VALUE);
    assert_eq!(format!("{value:?}"), "SecretValue([REDACTED])");
    assert!(matches!(SecretValue::new(""), Err(SecretError::EmptyValue)));
}

#[test]
fn secret_store_insert_replaces_and_reads_values() {
    let reference = SecretRef::new("nan_api_key").expect("reference should be valid");
    let mut store = SecretStore::new();

    assert!(!store.contains(&reference));
    store.insert(
        reference.clone(),
        SecretValue::new(FIRST_VALUE).expect("first secret should be valid"),
    );
    assert!(store.contains(&reference));
    assert_eq!(
        store.with_secret(&reference, str::to_owned),
        Ok(FIRST_VALUE.to_owned())
    );

    store.insert(
        reference.clone(),
        SecretValue::new(SECOND_VALUE).expect("second secret should be valid"),
    );
    assert_eq!(
        store.with_secret(&reference, str::to_owned),
        Ok(SECOND_VALUE.to_owned())
    );
}

#[test]
fn secret_store_missing_reference_error_is_typed() {
    let reference = SecretRef::new("missing_secret").expect("reference should be valid");
    let store = SecretStore::new();

    let error = store
        .with_secret(&reference, str::to_owned)
        .expect_err("missing reference should fail");

    assert_eq!(
        error,
        SecretError::MissingReference(reference.as_str().to_owned())
    );
    assert!(!store.contains(&reference));
}

#[test]
fn secret_store_debug_lists_references_without_values() {
    let reference = SecretRef::new("nan_api_key").expect("reference should be valid");
    let mut store = SecretStore::new();
    store.insert(
        reference,
        SecretValue::new(FIRST_VALUE).expect("secret should be valid"),
    );

    let debug = format!("{store:?}");

    assert!(debug.contains("nan_api_key"));
    assert!(!debug.contains(FIRST_VALUE));
}
