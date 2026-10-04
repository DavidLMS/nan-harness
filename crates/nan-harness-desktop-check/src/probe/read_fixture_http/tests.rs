use super::*;
use nan_harness_private_fs::{create_private_dir, open_private_new};
use std::io::Write as _;

fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let directory = tempfile::tempdir().expect("synthetic root");
    let root = directory.path().canonicalize().unwrap().join("owned");
    create_private_dir(&root).expect("private root");
    let target = root.join("read-target.txt");
    open_private_new(&target)
        .unwrap()
        .write_all(b"actual independently read bytes")
        .unwrap();
    (directory, target)
}

async fn call(client: &reqwest::Client, url: &str, id: u32, method: &str, params: Value) -> Value {
    client
        .post(url)
        .header("accept", "application/json, text/event-stream")
        .json(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

#[tokio::test]
async fn real_http_supports_probe_then_active_connection_and_reads_only_owned_file() {
    let (_directory, path) = fixture();
    let server = ReadFixtureServer::start(&path, Duration::from_secs(10))
        .await
        .unwrap();
    let client = reqwest::Client::new();
    assert!(server.owns(&path));
    let premature = call(&client, &server.url, 1, "tools/list", json!({})).await;
    assert_eq!(premature["error"]["code"], -32000);
    for id in [2, 3] {
        let initialized = call(
            &client,
            &server.url,
            id,
            "initialize",
            json!({"protocolVersion":"2025-06-18"}),
        )
        .await;
        assert_eq!(initialized["result"]["protocolVersion"], "2025-06-18");
    }
    let notified = client
        .post(&server.url)
        .json(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
        .send()
        .await
        .unwrap();
    assert_eq!(notified.status(), StatusCode::ACCEPTED);
    assert!(notified.bytes().await.unwrap().is_empty());
    assert_eq!(
        client.get(&server.url).send().await.unwrap().status(),
        StatusCode::METHOD_NOT_ALLOWED
    );
    let listed = call(&client, &server.url, 4, "tools/list", json!({})).await;
    assert_eq!(listed["result"]["tools"].as_array().unwrap().len(), 1);
    assert_eq!(
        listed["result"]["tools"][0]["inputSchema"]["properties"]["path"]["const"],
        json!(path)
    );
    let read = call(
        &client,
        &server.url,
        5,
        "tools/call",
        json!({"name":"read_file","arguments":{"path":path}}),
    )
    .await;
    assert_eq!(
        read["result"],
        json!({"content":[{"type":"text","text":"actual independently read bytes"}],"isError":false})
    );
    for arguments in [
        json!({"path":path.with_file_name("other")}),
        json!({"path":path,"extra":true}),
    ] {
        assert_eq!(
            call(
                &client,
                &server.url,
                6,
                "tools/call",
                json!({"name":"read_file","arguments":arguments})
            )
            .await["error"]["code"],
            -32602
        );
    }
    let address = server
        .state
        .authority
        .parse::<std::net::SocketAddr>()
        .unwrap();
    server.stop().await.expect("listener drained");
    assert!(tokio::net::TcpStream::connect(address).await.is_err());
    std::fs::remove_file(&path).expect("retained file released after server shutdown");
}

#[tokio::test]
async fn transport_rejects_foreign_origin_protocol_oversize_and_exhausted_budget() {
    let (_directory, path) = fixture();
    let server = ReadFixtureServer::start(&path, Duration::from_secs(10))
        .await
        .unwrap();
    let client = reqwest::Client::new();
    let request = json!({"jsonrpc":"2.0","id":1,"method":"initialize"});
    for (name, value) in [
        ("origin", "https://foreign.invalid"),
        ("host", "foreign.invalid"),
        ("mcp-protocol-version", "unknown"),
    ] {
        assert_eq!(
            client
                .post(&server.url)
                .header(name, value)
                .json(&request)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        client
            .post(&server.url)
            .json(&json!({"large":"x".repeat(8193)}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    server.state.requests.store(MAX_REQUESTS, Ordering::SeqCst);
    assert_eq!(
        client
            .post(&server.url)
            .json(&request)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[cfg(unix)]
#[test]
fn retained_file_rejects_replacement_mutation_links_and_shared_permissions() {
    use std::os::unix::fs::{PermissionsExt as _, symlink};
    let (_directory, path) = fixture();
    let mut retained = ReadTarget::open(&path).unwrap();
    std::fs::write(&path, "mutated private bytes").unwrap();
    assert!(retained.read().is_err());
    std::fs::remove_file(&path).unwrap();
    open_private_new(&path)
        .unwrap()
        .write_all(b"replacement")
        .unwrap();
    assert!(retained.read().is_err());
    let alias = path.with_file_name("alias");
    std::fs::hard_link(&path, &alias).unwrap();
    assert!(ReadTarget::open(&path).is_err());
    std::fs::remove_file(&alias).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(ReadTarget::open(&path).is_err());
    std::fs::remove_file(&path).unwrap();
    symlink("outside", &path).unwrap();
    assert!(ReadTarget::open(&path).is_err());
}

#[cfg(windows)]
#[test]
fn retained_windows_file_denies_write_and_delete_until_server_releases_it() {
    let (_directory, path) = fixture();
    let retained = ReadTarget::open(&path).unwrap();
    assert!(std::fs::write(&path, b"foreign replacement").is_err());
    assert!(std::fs::remove_file(&path).is_err());
    assert!(std::fs::rename(path.parent().unwrap(), path.with_file_name("renamed")).is_err());
    drop(retained);
    assert!(std::fs::remove_file(&path).is_ok());
}

#[tokio::test]
async fn lifetime_ends_listener_and_ownership_without_tool_activity() {
    let (_directory, path) = fixture();
    let server = ReadFixtureServer::start(&path, Duration::from_millis(10))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(!server.owns(&path));
    assert!(server.task.is_finished());
}
