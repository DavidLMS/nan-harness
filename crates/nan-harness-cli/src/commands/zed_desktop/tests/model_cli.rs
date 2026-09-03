use super::super::process::validate_passthrough_arguments;
use super::super::{ZedDesktopError, extract_semver, select_model};
use super::fixtures::{generic_model, model};
use nan_harness_core::ReasoningPolicy;

#[test]
fn reserved_lifecycle_arguments_are_rejected() {
    for argument in [
        "--foreground",
        "--wait",
        "-w",
        "--user-data-dir",
        "--user-data-dir=/tmp/zed",
    ] {
        assert!(matches!(
            validate_passthrough_arguments(&[argument.to_owned()]),
            Err(ZedDesktopError::ReservedArgument)
        ));
    }
    validate_passthrough_arguments(&["--new".to_owned(), "file.rs".to_owned()])
        .expect("ordinary Zed arguments should pass");
}

#[test]
fn model_selection_and_version_parsing_are_deterministic() {
    let models = vec![
        model("other", "Other", 1, 1, false, ReasoningPolicy::Unknown),
        generic_model(),
    ];
    assert_eq!(
        select_model(&models, None).expect("default should exist"),
        "qwen3.6"
    );
    assert_eq!(
        select_model(&models, Some("other")).expect("model should exist"),
        "other"
    );
    assert!(matches!(
        select_model(&models, Some("missing")),
        Err(ZedDesktopError::ModelUnavailable { .. })
    ));
    assert_eq!(
        extract_semver("Zed 0.205.4 stable"),
        Some(semver::Version::new(0, 205, 4))
    );
    assert_eq!(extract_semver("unparseable"), None);
}
