use super::{
    CredentialManager, CredentialSource, VERIFICATION_CACHE_SCHEMA_VERSION, VERIFICATION_CACHE_TTL,
    VerificationReceipt, is_rejected, render_first_harness_hint, render_missing_credential_hint,
    resolve_or_onboard_with, verification_cache_is_current, verify_cached_at,
};
use crate::commands::persistence::PersistenceError;
use nan_harness_core::SecretValue;
use nan_harness_runtime::{ConfigOverrides, ConfigResolver, EnvironmentSource, ProcessEnvironment};
use nan_harness_test_support::scripted_provider::{ProviderScenario, ScriptedProvider};
use std::collections::BTreeMap;

#[derive(Default)]
struct TestEnvironment(BTreeMap<String, String>);

impl EnvironmentSource for TestEnvironment {
    fn value(&self, name: &str) -> Option<String> {
        self.0.get(name).cloned()
    }
}

#[test]
fn missing_credential_hint_includes_api_url_once() {
    let hint = render_missing_credential_hint();

    assert_eq!(hint, "Get one at https://nan.builders/");
    assert_eq!(hint.matches("https://nan.builders/").count(), 1);
}

#[test]
fn first_harness_hint_only_renders_for_initial_onboarding() {
    assert_eq!(
        render_first_harness_hint(true),
        Some("Start your first harness with: nanh pi")
    );
    assert_eq!(render_first_harness_hint(false), None);
}

#[tokio::test]
async fn prompted_credentials_are_verified_saved_and_reused() {
    let provider = ScriptedProvider::start(ProviderScenario::inventory("unused"))
        .await
        .expect("scripted provider should start");
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let manager = CredentialManager::file_backend(directory.path().to_path_buf());
    let environment = TestEnvironment::default();

    let config = resolve_or_onboard_with(
        &environment,
        &manager,
        Some(provider.base_url().to_owned()),
        true,
        || SecretValue::new("nan-test-key").map_err(super::CredentialError::Secret),
    )
    .await
    .expect("interactive onboarding should succeed");
    config
        .secrets
        .with_secret(&config.provider_credential_ref, |value| {
            assert_eq!(value, "nan-test-key");
        })
        .expect("resolved credential should exist");

    let reused = resolve_or_onboard_with(
        &environment,
        &manager,
        Some(provider.base_url().to_owned()),
        false,
        || panic!("a saved credential must not prompt"),
    )
    .await
    .expect("saved credential should resolve non-interactively");
    reused
        .secrets
        .with_secret(&reused.provider_credential_ref, |value| {
            assert_eq!(value, "nan-test-key");
        })
        .expect("reused credential should exist");
    assert_eq!(
        manager
            .load()
            .expect("saved credential should load")
            .map(|(_, source)| source),
        Some(CredentialSource::PrivateFile)
    );
    assert!(
        manager
            .remove_saved()
            .expect("credential should be removed")
    );
    assert!(!manager.has_saved().expect("receipt should be removed"));

    provider.shutdown().await.expect("provider should stop");
}

#[tokio::test]
async fn current_verification_receipt_skips_model_discovery() {
    let provider = ScriptedProvider::start(ProviderScenario::inventory("unused"))
        .await
        .expect("scripted provider should start");
    let config = ConfigResolver::resolve(
        &ProcessEnvironment,
        ConfigOverrides {
            provider_base_url: Some(provider.base_url().to_owned()),
            nan_api_key: Some(SecretValue::new("nan-test-key").expect("test key should be valid")),
        },
    )
    .expect("test configuration should resolve");
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let cache_path = directory.path().join("credential-verification.json");
    let fingerprint =
        super::credential_fingerprint(&config).expect("test credential should have a fingerprint");
    std::fs::write(
        &cache_path,
        serde_json::to_vec(&VerificationReceipt {
            schema_version: VERIFICATION_CACHE_SCHEMA_VERSION,
            provider_base_url: config.provider_base_url.clone(),
            credential_fingerprint: fingerprint.clone(),
            verified_at_unix_seconds: super::unix_time().expect("system time should be available"),
        })
        .expect("receipt should serialize"),
    )
    .expect("receipt should be written");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&cache_path, std::fs::Permissions::from_mode(0o644))
            .expect("verification receipt should be made permissive");
    }
    #[cfg(windows)]
    nan_harness_test_support::windows_acl::make_permissive_file(&cache_path)
        .expect("verification receipt DACL should be made permissive");

    let model_catalog = verify_cached_at(&config, &cache_path, &fingerprint)
        .await
        .expect("current receipt should verify");
    assert!(model_catalog.is_none());
    assert_eq!(provider.model_requests(), 0);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(
            std::fs::metadata(&cache_path)
                .expect("verification receipt metadata should exist")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    #[cfg(windows)]
    nan_harness_test_support::windows_acl::assert_private_file(&cache_path)
        .expect("verification receipt DACL should be repaired");
    provider.shutdown().await.expect("provider should stop");
}

#[tokio::test]
async fn missing_and_expired_receipts_return_fresh_model_catalogs() {
    let provider = ScriptedProvider::start(ProviderScenario::inventory("unused"))
        .await
        .expect("scripted provider should start");
    let config = ConfigResolver::resolve(
        &ProcessEnvironment,
        ConfigOverrides {
            provider_base_url: Some(provider.base_url().to_owned()),
            nan_api_key: Some(SecretValue::new("nan-test-key").expect("test key should be valid")),
        },
    )
    .expect("test configuration should resolve");
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let fingerprint =
        super::credential_fingerprint(&config).expect("test credential should have a fingerprint");

    let missing_path = directory.path().join("missing-verification.json");
    let missing_models = verify_cached_at(&config, &missing_path, &fingerprint)
        .await
        .expect("missing receipt should trigger verification")
        .expect("fresh verification should return its model catalog");
    assert!(!missing_models.is_empty());
    assert_eq!(provider.model_requests(), 1);
    assert!(
        verification_cache_is_current(&missing_path, &config.provider_base_url, &fingerprint)
            .expect("renewed receipt should load")
    );

    let expired_path = directory.path().join("expired-verification.json");
    std::fs::write(
        &expired_path,
        serde_json::to_vec(&VerificationReceipt {
            schema_version: VERIFICATION_CACHE_SCHEMA_VERSION,
            provider_base_url: config.provider_base_url.clone(),
            credential_fingerprint: fingerprint.clone(),
            verified_at_unix_seconds: super::unix_time()
                .expect("system time should be available")
                .saturating_sub(VERIFICATION_CACHE_TTL.as_secs()),
        })
        .expect("expired receipt should serialize"),
    )
    .expect("expired receipt should be written");
    let expired_models = verify_cached_at(&config, &expired_path, &fingerprint)
        .await
        .expect("expired receipt should trigger verification")
        .expect("fresh verification should return its model catalog");
    assert!(!expired_models.is_empty());
    assert_eq!(provider.model_requests(), 2);
    assert!(
        verification_cache_is_current(&expired_path, &config.provider_base_url, &fingerprint)
            .expect("renewed receipt should load")
    );
    provider.shutdown().await.expect("provider should stop");
}

#[test]
fn private_credentials_use_owner_only_permissions() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let manager = CredentialManager::file_backend(directory.path().to_path_buf());
    manager
        .save("nan-test-key")
        .expect("credential should be saved");

    let credential_path = directory.path().join("nan-api-key");
    let receipt_path = directory.path().join("credential.json");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;

        let mode = |path: &std::path::Path| {
            std::fs::metadata(path)
                .expect("private credential file should exist")
                .permissions()
                .mode()
                & 0o777
        };
        assert_eq!(mode(&credential_path), 0o600);
        assert_eq!(mode(&receipt_path), 0o600);
        std::fs::set_permissions(&credential_path, std::fs::Permissions::from_mode(0o644))
            .expect("credential should be made permissive");
        std::fs::set_permissions(&receipt_path, std::fs::Permissions::from_mode(0o644))
            .expect("receipt should be made permissive");
    }
    #[cfg(windows)]
    {
        use nan_harness_test_support::windows_acl::{assert_private_file, make_permissive_file};

        assert_private_file(&credential_path)
            .expect("credential should have a private protected DACL");
        assert_private_file(&receipt_path).expect("receipt should have a private protected DACL");

        make_permissive_file(&credential_path).expect("credential ACL should be made permissive");
        make_permissive_file(&receipt_path).expect("receipt ACL should be made permissive");
    }

    let (api_key, source) = manager
        .load()
        .expect("permissive credentials should be repaired")
        .expect("saved credential should remain available");
    assert_eq!(source, CredentialSource::PrivateFile);
    api_key.with_secret(|value| assert_eq!(value, "nan-test-key"));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;

        let mode = |path: &std::path::Path| {
            std::fs::metadata(path)
                .expect("repaired credential file should exist")
                .permissions()
                .mode()
                & 0o777
        };
        assert_eq!(mode(&credential_path), 0o600);
        assert_eq!(mode(&receipt_path), 0o600);
    }
    #[cfg(windows)]
    {
        use nan_harness_test_support::windows_acl::assert_private_file;

        assert_private_file(&credential_path)
            .expect("credential read should restore a private protected DACL");
        assert_private_file(&receipt_path)
            .expect("receipt read should restore a private protected DACL");
    }
}

#[test]
fn missing_private_credentials_remain_absent() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let manager = CredentialManager::file_backend(directory.path().to_path_buf());

    assert!(
        manager
            .load()
            .expect("missing credentials should not fail")
            .is_none()
    );
}

#[test]
fn permission_repair_warnings_are_fixed_and_path_free() {
    for (warning, expected) in [
        (
            super::SAVED_KEY_REPAIR_WARNING,
            "warning: restored private permissions on the saved NaN API key.",
        ),
        (
            super::CREDENTIAL_METADATA_REPAIR_WARNING,
            "warning: restored private permissions on NaN credential metadata.",
        ),
        (
            super::VERIFICATION_RECEIPT_REPAIR_WARNING,
            "warning: restored private permissions on the NaN verification receipt.",
        ),
    ] {
        assert_eq!(
            super::private_file_repair_warning(
                nan_harness_private_fs::PrivateFileReadStatus::AlreadyPrivate,
                warning,
            ),
            None,
            "already-private fixtures must not emit a repair warning"
        );
        assert_eq!(
            super::private_file_repair_warning(
                nan_harness_private_fs::PrivateFileReadStatus::Repaired,
                warning,
            ),
            Some(expected)
        );
    }
}

#[test]
fn verification_cache_is_scoped_to_endpoint_key_and_one_hour() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let path = directory.path().join("credential-verification.json");
    let now = super::unix_time().expect("system time should be available");
    let write = |verified_at_unix_seconds| {
        std::fs::write(
            &path,
            serde_json::to_vec(&VerificationReceipt {
                schema_version: VERIFICATION_CACHE_SCHEMA_VERSION,
                provider_base_url: "https://api.nan.test/v1".to_owned(),
                credential_fingerprint: "fingerprint-a".to_owned(),
                verified_at_unix_seconds,
            })
            .expect("verification receipt should serialize"),
        )
        .expect("verification receipt should be written");
    };

    write(now);
    assert!(
        verification_cache_is_current(&path, "https://api.nan.test/v1", "fingerprint-a")
            .expect("fresh cache should load")
    );
    assert!(
        !verification_cache_is_current(&path, "https://other.nan.test/v1", "fingerprint-a")
            .expect("endpoint mismatch should load")
    );
    assert!(
        !verification_cache_is_current(&path, "https://api.nan.test/v1", "fingerprint-b")
            .expect("credential mismatch should load")
    );

    write(now.saturating_sub(VERIFICATION_CACHE_TTL.as_secs()));
    assert!(
        !verification_cache_is_current(&path, "https://api.nan.test/v1", "fingerprint-a")
            .expect("expired cache should load")
    );
}

#[test]
fn only_provider_authentication_statuses_trigger_key_recovery() {
    for status in [401, 403] {
        assert!(is_rejected(&super::CredentialError::Verification(
            PersistenceError::ModelDiscoveryStatus(status)
        )));
    }
    for status in [400, 408, 429, 500, 503] {
        assert!(!is_rejected(&super::CredentialError::Verification(
            PersistenceError::ModelDiscoveryStatus(status)
        )));
    }
}
