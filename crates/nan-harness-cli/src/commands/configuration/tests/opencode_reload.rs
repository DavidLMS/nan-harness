use super::*;
use axum::{
    Json, Router,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[tokio::test]
async fn reload_only_contacts_matching_authenticated_existing_service() {
    let calls = Arc::new(AtomicUsize::new(0));
    let reload_calls = calls.clone();
    let router = Router::new()
        .route(
            "/api/info",
            get(|headers: HeaderMap| async move {
                assert_eq!(headers["authorization"], "Basic b3BlbmNvZGU6c3ludGhldGlj");
                Json(json!({"pid": 123, "version": "2.0.27"}))
            }),
        )
        .route(
            "/api/location/reload",
            post(move |headers: HeaderMap| async move {
                assert_eq!(headers["authorization"], "Basic b3BlbmNvZGU6c3ludGhldGlj");
                reload_calls.fetch_add(1, Ordering::SeqCst);
                StatusCode::NO_CONTENT
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("service.json");
    assert!(reload(&path).await.is_ok());
    assert!(!path.exists());
    for (pid, version, success) in [
        (123, "2.0.27", true),
        (124, "2.0.27", false),
        (123, "2.0.28", false),
    ] {
        std::fs::write(
            &path,
            serde_json::to_vec(&json!({
                "url": format!("http://{address}"), "pid": pid, "version": version,
                "password": "synthetic"
            }))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(reload(&path).await.is_ok(), success);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    server.abort();
}

#[test]
fn registration_rejects_remote_credentials_and_other_generations() {
    for url in [
        "http://example.test",
        "http://localhost",
        "https://127.0.0.1",
        "http://127.0.0.1?secret=x",
        "http://user@127.0.0.1",
        "http://127.0.0.1/path",
    ] {
        let registration = Registration {
            url: url.to_owned(),
            pid: 123,
            version: "2.0.27".to_owned(),
            password: "synthetic".to_owned(),
        };
        assert!(local_endpoint(&registration).is_err(), "{url}");
    }
    let registration = Registration {
        url: "http://127.0.0.1:1234".to_owned(),
        pid: 123,
        version: "1.18.29".to_owned(),
        password: "synthetic".to_owned(),
    };
    assert!(local_endpoint(&registration).is_err());
}
