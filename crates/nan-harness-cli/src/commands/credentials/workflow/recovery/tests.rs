use super::recover_rejected_credential_with;
use crate::commands::credentials::{CredentialError, CredentialManager, CredentialSource};
use nan_harness_core::SecretValue;
use nan_harness_runtime::{ConfigOverrides, ConfigResolver, EnvironmentSource};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::watch;

#[derive(Default)]
struct TestEnvironment(BTreeMap<String, String>);

impl EnvironmentSource for TestEnvironment {
    fn value(&self, name: &str) -> Option<String> {
        self.0.get(name).cloned()
    }
}

struct ModelProvider {
    base_url: String,
    stop: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
}

impl ModelProvider {
    async fn start(responses: impl IntoIterator<Item = (&'static str, u16)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("model provider should bind");
        let address = listener
            .local_addr()
            .expect("model provider address should be available");
        let responses = Arc::new(responses.into_iter().collect::<Vec<_>>());
        let (stop, mut stopped) = watch::channel(false);
        let task = tokio::spawn({
            let responses = Arc::clone(&responses);
            async move {
                let mut index = 0;
                loop {
                    tokio::select! {
                        result = listener.accept() => {
                            let (mut stream, _) = result.expect("model provider should accept");
                            let request = read_request_headers(&mut stream).await;
                            assert!(request.starts_with("GET /v1/models "));
                            let (expected_key, status) = responses.get(index)
                                .copied().expect("unexpected model request");
                            assert!(request.to_ascii_lowercase().contains(&format!(
                                "authorization: bearer {expected_key}"
                            )));
                            index += 1;
                            let body = if status == 200 {
                                r#"{"data":[{"id":"qwen3.6"}]}"#
                            } else {
                                "{}"
                            };
                            let response = format!(
                                "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                                body.len()
                            );
                            stream.write_all(response.as_bytes()).await.expect("response should write");
                        }
                        changed = stopped.changed() => {
                            if changed.is_err() || *stopped.borrow() {
                                assert_eq!(index, responses.len(), "all scripted requests must be consumed");
                                break;
                            }
                        }
                    }
                }
            }
        });
        Self {
            base_url: format!("http://{address}/v1"),
            stop,
            task,
        }
    }

    async fn shutdown(self) {
        let _ = self.stop.send(true);
        self.task.await.expect("model provider should stop");
    }
}

async fn read_request_headers(stream: &mut tokio::net::TcpStream) -> String {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let mut request = Vec::new();
        while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
            let mut chunk = [0; 1024];
            let size = stream.read(&mut chunk).await.expect("request should read");
            assert!(size > 0, "request closed before complete headers");
            request.extend_from_slice(&chunk[..size]);
            assert!(request.len() <= 8192, "fixture headers must remain bounded");
        }
        String::from_utf8(request).expect("fixture headers should be UTF-8")
    })
    .await
    .expect("fixture request headers must arrive within the deadline")
}

fn config_for(
    environment: &impl EnvironmentSource,
    provider_base_url: &str,
    key: &str,
) -> nan_harness_runtime::ResolvedConfig {
    ConfigResolver::resolve(
        environment,
        ConfigOverrides {
            provider_base_url: Some(provider_base_url.to_owned()),
            nan_api_key: Some(SecretValue::new(key).expect("fixture key should be valid")),
        },
    )
    .expect("fixture config should resolve")
}

#[tokio::test]
async fn rejected_environment_key_can_use_a_verified_saved_key() {
    let provider = ModelProvider::start([("environment-key", 401), ("saved-key", 200)]).await;
    let directory = tempfile::tempdir().expect("credential directory should exist");
    let manager = CredentialManager::file_backend(directory.path());
    manager.save("saved-key").expect("saved key should persist");
    let environment = TestEnvironment(BTreeMap::from([(
        "NAN_API_KEY".to_owned(),
        "environment-key".to_owned(),
    )]));
    let original = crate::commands::credentials::verification::verify_models(&config_for(
        &environment,
        &provider.base_url,
        "environment-key",
    ))
    .await
    .expect_err("environment fixture must be rejected");
    assert!(matches!(original, CredentialError::Verification(_)));
    let mut prompts = [true].into_iter();
    let recovered = recover_rejected_credential_with(
        &environment,
        &manager,
        Some(provider.base_url.clone()),
        CredentialSource::Environment,
        original,
        |_, _| Ok(prompts.next().expect("saved-key prompt should be used")),
        || panic!("replacement prompt must not be reached"),
    )
    .await
    .expect("saved key should recover the launch");
    recovered
        .config
        .secrets
        .with_secret(&recovered.config.provider_credential_ref, |key| {
            assert_eq!(key, "saved-key");
        })
        .expect("recovered key should be present");
    assert_eq!(
        manager
            .load()
            .expect("saved key should load")
            .map(|(_, source)| source),
        Some(CredentialSource::PrivateFile)
    );
    provider.shutdown().await;
}

#[tokio::test]
async fn rejected_saved_key_can_be_replaced_after_successful_verification() {
    let provider = ModelProvider::start([
        ("environment-key", 401),
        ("saved-key", 401),
        ("replacement-key", 200),
    ])
    .await;
    let directory = tempfile::tempdir().expect("credential directory should exist");
    let manager = CredentialManager::file_backend(directory.path());
    manager.save("saved-key").expect("saved key should persist");
    let environment = TestEnvironment(BTreeMap::from([(
        "NAN_API_KEY".to_owned(),
        "environment-key".to_owned(),
    )]));
    let environment_config = config_for(&environment, &provider.base_url, "environment-key");
    crate::commands::credentials::verification::verify_models(&environment_config)
        .await
        .expect_err("environment fixture must be rejected");
    let mut prompts = [true, true].into_iter();
    let original = CredentialError::Verification(
        crate::commands::persistence::PersistenceError::ModelDiscoveryStatus(401),
    );
    let recovered = recover_rejected_credential_with(
        &environment,
        &manager,
        Some(provider.base_url.clone()),
        CredentialSource::Environment,
        original,
        |_, _| Ok(prompts.next().expect("recovery prompt should be used")),
        || SecretValue::new("replacement-key").map_err(CredentialError::Secret),
    )
    .await
    .expect("verified replacement should recover the launch");
    recovered
        .config
        .secrets
        .with_secret(&recovered.config.provider_credential_ref, |key| {
            assert_eq!(key, "replacement-key");
        })
        .expect("replacement key should be present");
    let (saved, _) = manager
        .load()
        .expect("saved key should load")
        .expect("replacement should be saved");
    saved.with_secret(|key| assert_eq!(key, "replacement-key"));
    provider.shutdown().await;
}

#[tokio::test]
async fn declining_replacement_preserves_original_error_and_saved_state() {
    let directory = tempfile::tempdir().expect("credential directory should exist");
    let manager = CredentialManager::file_backend(directory.path());
    manager.save("saved-key").expect("saved key should persist");
    let before = manager
        .load()
        .expect("saved key should load")
        .expect("saved key should exist")
        .0;
    let original = CredentialError::Verification(
        crate::commands::persistence::PersistenceError::ModelDiscoveryStatus(401),
    );
    let result = recover_rejected_credential_with(
        &TestEnvironment::default(),
        &manager,
        Some("http://127.0.0.1:1/v1".to_owned()),
        CredentialSource::PrivateFile,
        original,
        |_, _| Ok(false),
        || panic!("declining replacement must not prompt for a key"),
    )
    .await;
    assert!(matches!(
        result,
        Err(CredentialError::Verification(
            crate::commands::persistence::PersistenceError::ModelDiscoveryStatus(401)
        ))
    ));
    let after = manager
        .load()
        .expect("saved key should load")
        .expect("saved key should exist")
        .0;
    before.with_secret(|before| after.with_secret(|after| assert_eq!(after, before)));
}

#[tokio::test]
async fn non_authentication_failure_propagates_without_replacement() {
    let provider = ModelProvider::start([("saved-key", 500)]).await;
    let directory = tempfile::tempdir().expect("credential directory should exist");
    let manager = CredentialManager::file_backend(directory.path());
    manager.save("saved-key").expect("saved key should persist");
    let original = CredentialError::Verification(
        crate::commands::persistence::PersistenceError::ModelDiscoveryStatus(401),
    );
    let mut replacement_prompted = false;
    let result = recover_rejected_credential_with(
        &TestEnvironment::default(),
        &manager,
        Some(provider.base_url.clone()),
        CredentialSource::Environment,
        original,
        |prompt, _| {
            replacement_prompted |= prompt.contains("replacement");
            Ok(true)
        },
        || panic!("non-authentication verification failure must not replace"),
    )
    .await;
    assert!(!replacement_prompted);
    assert!(matches!(
        result,
        Err(CredentialError::Verification(
            crate::commands::persistence::PersistenceError::ModelDiscoveryStatus(500)
        ))
    ));
    provider.shutdown().await;
}

#[tokio::test]
async fn rejected_replacement_preserves_the_previous_saved_credential() {
    let provider = ModelProvider::start([("saved-key", 401), ("replacement-key", 403)]).await;
    let directory = tempfile::tempdir().expect("credential directory");
    let manager = CredentialManager::file_backend(directory.path());
    manager.save("saved-key").expect("initial saved credential");
    let original = CredentialError::Verification(
        crate::commands::persistence::PersistenceError::ModelDiscoveryStatus(401),
    );
    let mut prompts = [true, true].into_iter();
    let result = recover_rejected_credential_with(
        &TestEnvironment::default(),
        &manager,
        Some(provider.base_url.clone()),
        CredentialSource::Environment,
        original,
        |_, _| {
            Ok(prompts
                .next()
                .expect("only saved-key and replacement prompts"))
        },
        || SecretValue::new("replacement-key").map_err(CredentialError::Secret),
    )
    .await;
    assert!(matches!(
        result,
        Err(CredentialError::Verification(
            crate::commands::persistence::PersistenceError::ModelDiscoveryStatus(403)
        ))
    ));
    assert!(prompts.next().is_none());
    let (saved, source) = manager
        .load()
        .expect("read saved key")
        .expect("old key remains");
    assert_eq!(source, CredentialSource::PrivateFile);
    saved.with_secret(|key| assert_eq!(key, "saved-key"));
    provider.shutdown().await;
}
