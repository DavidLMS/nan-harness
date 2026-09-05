use nan_harness_core::{SecretError, SecretRef};

#[test]
fn secret_refs_validate_exact_grammar_boundaries() {
    let maximum = format!("a{}", "z".repeat(63));
    let too_long = format!("a{}", "z".repeat(64));

    for reference in ["abc", maximum.as_str()] {
        SecretRef::new(reference).expect("boundary reference should be valid");
    }

    for reference in ["", "ab", too_long.as_str()] {
        assert!(matches!(
            SecretRef::new(reference),
            Err(SecretError::InvalidReference(invalid)) if invalid == reference
        ));
    }
}

#[test]
fn secret_refs_reject_invalid_characters_in_any_position() {
    for reference in ["ABC", "0bc", "a-c", "a b", "ábc"] {
        assert!(matches!(
            SecretRef::new(reference),
            Err(SecretError::InvalidReference(invalid)) if invalid == reference
        ));
    }
}

#[test]
fn secret_ref_display_and_debug_expose_the_reference_identity() {
    let reference = SecretRef::new("nan_api_key").expect("reference should be valid");

    assert_eq!(reference.as_str(), "nan_api_key");
    assert_eq!(reference.to_string(), "nan_api_key");
    assert_eq!(format!("{reference:?}"), r#"SecretRef("nan_api_key")"#);
}

#[test]
fn secret_ref_json_roundtrip_preserves_the_reference() {
    let reference: SecretRef =
        serde_json::from_str(r#""nan_api_key""#).expect("valid JSON reference should deserialize");

    let serialized = serde_json::to_string(&reference).expect("reference should serialize");

    assert_eq!(serialized, r#""nan_api_key""#);
}

#[test]
fn secret_ref_deserialization_rejects_invalid_references() {
    let error = serde_json::from_str::<SecretRef>(r#""AB""#)
        .expect_err("invalid JSON reference should be rejected");

    assert!(error.to_string().contains("secret references must match"));
}
