use super::super::lifecycle::serve;
use crate::protocol::{
    AttemptOutcome, ClientMessage, EndpointKind, PROTOCOL_VERSION, RequestLane, RequestPriority,
    ServerMessage, read_frame, write_frame,
};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};

#[tokio::test]
async fn authenticated_ipc_grants_and_completes_a_lease() {
    let temporary = tempfile::tempdir().expect("temporary directory should exist");
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("test coordinator should bind");
    let address = listener
        .local_addr()
        .expect("listener address should exist");
    let cache = temporary.path().join("capacity.json");
    let daemon = tokio::spawn(serve(listener, "token".to_owned(), cache));

    let mut stream = TcpStream::connect(address)
        .await
        .expect("client should connect");
    write_frame(
        &mut stream,
        &ClientMessage::Acquire {
            protocol_version: PROTOCOL_VERSION,
            token: "token".to_owned(),
            scope: "credential".to_owned(),
            launch_id: "codex".to_owned(),
            endpoint: EndpointKind::Inference,
            model: Some("model".to_owned()),
            lane: RequestLane::Inference,
            priority: RequestPriority::Foreground,
            budget_tokens: None,
        },
    )
    .await
    .expect("acquire should write");
    let grant = read_frame::<ServerMessage>(&mut stream)
        .await
        .expect("grant should read");
    assert!(matches!(grant, ServerMessage::Granted { .. }));

    write_frame(
        &mut stream,
        &ClientMessage::Observe {
            lease_id: 1,
            outcome: AttemptOutcome::Success,
            retry_after_ms: None,
            usage: None,
        },
    )
    .await
    .expect("outcome should write");
    let completion = read_frame::<ServerMessage>(&mut stream)
        .await
        .expect("completion should read");
    assert!(matches!(completion, ServerMessage::Complete));
    drop(stream);
    daemon.abort();
}

#[tokio::test]
async fn incompatible_or_unauthorized_clients_are_rejected() {
    let temporary = tempfile::tempdir().expect("temporary directory should exist");
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("test coordinator should bind");
    let address = listener
        .local_addr()
        .expect("listener address should exist");
    let daemon = tokio::spawn(serve(
        listener,
        "token".to_owned(),
        temporary.path().join("capacity.json"),
    ));

    for (protocol_version, token) in [(PROTOCOL_VERSION - 1, "token"), (PROTOCOL_VERSION, "wrong")]
    {
        let mut stream = TcpStream::connect(address)
            .await
            .expect("client should connect");
        write_frame(
            &mut stream,
            &ClientMessage::Acquire {
                protocol_version,
                token: token.to_owned(),
                scope: "credential".to_owned(),
                launch_id: "codex".to_owned(),
                endpoint: EndpointKind::Inference,
                model: Some("model".to_owned()),
                lane: RequestLane::Inference,
                priority: RequestPriority::Foreground,
                budget_tokens: None,
            },
        )
        .await
        .expect("acquire should write");
        let response = tokio::time::timeout(
            Duration::from_secs(1),
            read_frame::<ServerMessage>(&mut stream),
        )
        .await
        .expect("invalid clients should be rejected promptly")
        .expect("rejection should read");
        assert!(matches!(
            response,
            ServerMessage::Rejected { reason }
                if reason == "incompatible or unauthorized coordinator client"
        ));
    }
    daemon.abort();
}

#[tokio::test]
async fn disconnected_lease_releases_capacity_for_a_queued_client() {
    let temporary = tempfile::tempdir().expect("temporary directory should exist");
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("test coordinator should bind");
    let address = listener.local_addr().expect("listener address");
    let daemon = tokio::spawn(serve(
        listener,
        "token".to_owned(),
        temporary.path().join("capacity.json"),
    ));

    let mut first = request_capacity(address).await;
    let mut second = request_capacity(address).await;
    for stream in [&mut first, &mut second] {
        let grant =
            tokio::time::timeout(Duration::from_secs(1), read_frame::<ServerMessage>(stream))
                .await
                .expect("initial capacity should be available")
                .expect("grant should read");
        assert!(matches!(grant, ServerMessage::Granted { .. }));
    }

    let mut queued = request_capacity(address).await;
    let grant = {
        let pending = read_frame::<ServerMessage>(&mut queued);
        tokio::pin!(pending);
        assert!(
            tokio::time::timeout(Duration::from_millis(50), &mut pending)
                .await
                .is_err(),
            "both initial permits are held, so the third client must remain queued"
        );
        // A disconnected client cannot send an outcome or explicitly release its lease.
        drop(first);
        tokio::time::timeout(Duration::from_secs(1), &mut pending)
            .await
            .expect("disconnect should free capacity without waiting for an idle timeout")
            .expect("queued grant should read")
    };
    assert!(matches!(grant, ServerMessage::Granted { .. }));
    drop(second);
    drop(queued);
    daemon.abort();
}

async fn request_capacity(address: SocketAddr) -> TcpStream {
    let mut stream = TcpStream::connect(address)
        .await
        .expect("client connection");
    write_frame(
        &mut stream,
        &ClientMessage::Acquire {
            protocol_version: PROTOCOL_VERSION,
            token: "token".to_owned(),
            scope: "credential".to_owned(),
            launch_id: "codex".to_owned(),
            endpoint: EndpointKind::Inference,
            model: Some("model".to_owned()),
            lane: RequestLane::Inference,
            priority: RequestPriority::Foreground,
            budget_tokens: None,
        },
    )
    .await
    .expect("acquire should write");
    stream
}
