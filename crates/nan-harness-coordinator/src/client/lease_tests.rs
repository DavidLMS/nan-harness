use super::{
    AttemptOutcome, ClientMessage, EndpointKind, PROTOCOL_VERSION, RequestLane, RequestLease,
    RequestPriority, RetryDirective, ServerMessage, read_frame, write_frame,
};
use std::time::Duration;
use tokio::time::timeout;
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    sync::mpsc,
};

const TEST_BUDGET: Duration = Duration::from_secs(2);

async fn start_granted_lease(listener: TcpListener) -> (RequestLease, TcpStream) {
    let address = listener.local_addr().expect("listener address");
    let mut client = TcpStream::connect(address)
        .await
        .expect("client should connect");
    write_frame(
        &mut client,
        &ClientMessage::Acquire {
            protocol_version: PROTOCOL_VERSION,
            token: "test-token".to_owned(),
            scope: "scope".to_owned(),
            launch_id: "launch".to_owned(),
            endpoint: EndpointKind::Inference,
            model: Some("model".to_owned()),
            lane: RequestLane::Inference,
            priority: RequestPriority::Foreground,
            budget_tokens: None,
        },
    )
    .await
    .expect("Acquire request should be written");
    let (mut server, _) = listener.accept().await.expect("client should connect");
    let message: ClientMessage = read_frame(&mut server)
        .await
        .expect("Acquire request should arrive");
    assert!(matches!(message, ClientMessage::Acquire { .. }));
    write_frame(
        &mut server,
        &ServerMessage::Granted {
            lease_id: 1,
            queued_ms: 0,
        },
    )
    .await
    .expect("grant should be written");
    let granted = read_frame::<ServerMessage>(&mut client)
        .await
        .expect("grant should be consumed");
    assert!(matches!(granted, ServerMessage::Granted { .. }));
    let lease = RequestLease {
        stream: Some(client),
        lease_id: 1,
        queued: Duration::ZERO,
    };
    (lease, server)
}

#[tokio::test]
async fn observe_without_acknowledgement_is_bounded_and_closes_the_peer() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let (mut lease, server) = start_granted_lease(listener).await;
    let (events, mut server_events) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut server = server;
        let message: ClientMessage = read_frame(&mut server)
            .await
            .expect("observation should arrive");
        assert!(matches!(message, ClientMessage::Observe { .. }));
        events.send("observed").expect("observation observer");
        let closure = server.read_u8().await.expect_err("peer should close");
        assert_eq!(closure.kind(), std::io::ErrorKind::UnexpectedEof);
        events.send("closed").expect("closure observer");
    });

    let directive = timeout(TEST_BUDGET, lease.observe(AttemptOutcome::Success, None))
        .await
        .expect("observation should complete");

    assert_eq!(directive, RetryDirective::Complete);
    assert_eq!(
        timeout(TEST_BUDGET, server_events.recv())
            .await
            .expect("observation should be received"),
        Some("observed")
    );
    assert_eq!(
        timeout(TEST_BUDGET, server_events.recv())
            .await
            .expect("closure should be observed"),
        Some("closed")
    );
}

#[tokio::test]
async fn observe_preserves_a_retry_directive() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let (mut lease, server) = start_granted_lease(listener).await;
    tokio::spawn(async move {
        let mut server = server;
        let _: ClientMessage = read_frame(&mut server)
            .await
            .expect("observation should arrive");
        write_frame(&mut server, &ServerMessage::Retry { delay_ms: 750 })
            .await
            .expect("retry directive should be written");
    });

    let directive = timeout(TEST_BUDGET, lease.observe(AttemptOutcome::Transport, None))
        .await
        .expect("observation should complete");

    assert_eq!(
        directive,
        RetryDirective::RetryAfter(Duration::from_millis(750))
    );
}

#[tokio::test]
async fn observe_tolerates_a_disconnected_peer() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let (mut lease, server) = start_granted_lease(listener).await;
    tokio::spawn(async move {
        let mut server = server;
        let _: ClientMessage = read_frame(&mut server)
            .await
            .expect("observation should arrive");
        drop(server);
    });

    let directive = timeout(TEST_BUDGET, lease.observe(AttemptOutcome::Success, None))
        .await
        .expect("observation should complete");

    assert_eq!(directive, RetryDirective::Complete);
}

#[tokio::test]
async fn progress_write_failure_closes_the_lease() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let (mut lease, server) = start_granted_lease(listener).await;
    let _server = server;
    lease
        .stream
        .as_mut()
        .expect("lease stream")
        .shutdown()
        .await
        .expect("close client write half");

    timeout(
        TEST_BUDGET,
        lease.headers_received(Duration::from_millis(10)),
    )
    .await
    .expect("progress should complete");

    assert!(
        lease.stream.is_none(),
        "progress failure should close lease"
    );
}
