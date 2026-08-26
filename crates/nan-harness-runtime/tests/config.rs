use nan_harness_core::SecretValue;
use nan_harness_runtime::config::{
    ConfigError, ConfigOverrides, ConfigResolver, DEFAULT_PROVIDER_BASE_URL, EnvironmentSource,
};
use std::collections::BTreeMap;

#[derive(Default)]
struct TestEnvironment(BTreeMap<String, String>);

impl EnvironmentSource for TestEnvironment {
    fn value(&self, name: &str) -> Option<String> {
        self.0.get(name).cloned()
    }
}

#[test]
fn configuration_uses_override_environment_and_default_precedence() {
    let environment = TestEnvironment(BTreeMap::from([
        ("NAN_API_KEY".to_owned(), "environment-key".to_owned()),
        (
            "NAN_BASE_URL".to_owned(),
            "https://environment.example/v1".to_owned(),
        ),
    ]));
    let resolved = ConfigResolver::resolve(
        &environment,
        ConfigOverrides {
            provider_base_url: Some("https://override.example/v1".to_owned()),
            nan_api_key: Some(SecretValue::new("override-key").expect("valid secret")),
        },
    )
    .expect("configuration should resolve");

    assert_eq!(resolved.provider_base_url, "https://override.example/v1");
    assert_eq!(resolved.provider_credential_ref.as_str(), "nan_api_key");
    resolved
        .secrets
        .with_secret(&resolved.provider_credential_ref, |value| {
            assert_eq!(value, "override-key");
        })
        .expect("resolved secret should exist");

    let defaults = ConfigResolver::resolve(
        &TestEnvironment(BTreeMap::from([(
            "NAN_API_KEY".to_owned(),
            "environment-key".to_owned(),
        )])),
        ConfigOverrides::default(),
    )
    .expect("default URL should resolve");
    assert_eq!(defaults.provider_base_url, DEFAULT_PROVIDER_BASE_URL);
}

#[test]
fn configuration_rejects_missing_credentials_and_invalid_urls() {
    let missing = ConfigResolver::resolve(&TestEnvironment::default(), ConfigOverrides::default());
    assert_eq!(
        missing.expect_err("missing key should fail").code(),
        "NH-CONFIG-001"
    );

    let invalid = ConfigResolver::resolve(
        &TestEnvironment::default(),
        ConfigOverrides {
            provider_base_url: Some("file:///tmp/provider".to_owned()),
            nan_api_key: Some(SecretValue::new("key").expect("valid secret")),
        },
    );
    assert!(matches!(invalid, Err(ConfigError::InvalidProviderBaseUrl)));
}

#[test]
fn provider_base_url_accepts_https_and_loopback_http() {
    for value in [
        "https://api.example.com",
        "http://127.0.0.1:8080",
        "http://localhost:3000",
        "http://[::1]:9000",
    ] {
        let resolved = ConfigResolver::resolve(
            &TestEnvironment::default(),
            ConfigOverrides {
                provider_base_url: Some(value.to_owned()),
                nan_api_key: Some(SecretValue::new("key").expect("valid secret")),
            },
        )
        .unwrap_or_else(|error| panic!("{value} should be accepted: {error:?}"));
        assert_eq!(resolved.provider_base_url, value);
    }
}

#[test]
fn provider_base_url_rejects_remote_plaintext_http_and_invalid_urls() {
    for value in [
        "http://api.example.com",
        "ftp://api.example.com",
        "",
        "https://api.example.com/with whitespace",
    ] {
        let result = ConfigResolver::resolve(
            &TestEnvironment::default(),
            ConfigOverrides {
                provider_base_url: Some(value.to_owned()),
                nan_api_key: Some(SecretValue::new("key").expect("valid secret")),
            },
        );
        assert!(
            matches!(result, Err(ConfigError::InvalidProviderBaseUrl)),
            "{value:?} should be rejected"
        );
    }
}
