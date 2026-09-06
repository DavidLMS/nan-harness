use super::*;

#[tokio::test]
async fn supervision_follows_a_replacement_and_closes_only_when_both_are_gone() {
    let old = desktop(1);
    let replacement = desktop(2);
    let lifecycle = FakeLifecycle::new()
        .identities([Ok(true), Ok(true), Ok(false), Ok(true), Ok(false)])
        .discoveries([Ok(Some(replacement.clone())), Ok(None)]);
    let mut gateway = FakeGateway::Pending;
    let (_sender, mut signals) = mpsc::unbounded_channel();

    let completion = bounded(supervise_running(
        old.clone(),
        &lifecycle,
        &mut gateway,
        &mut signals,
        IMMEDIATE,
    ))
    .await
    .expect("supervision should end when no desktop remains");

    assert_eq!(completion, LifecycleCompletion::Closed(0));
    assert_eq!(
        lifecycle.checked(),
        vec![
            old.clone(),
            old.clone(),
            old,
            replacement.clone(),
            replacement,
        ]
    );
    assert_eq!(lifecycle.terminations(), 0);
}

#[tokio::test]
async fn an_absent_gateway_stays_pending_while_supervising() {
    let lifecycle = FakeLifecycle::new()
        .identities([Ok(false)])
        .discoveries([Ok(None)]);
    let mut gateway: Option<&mut RunningChatCompletionsGateway> = None;
    let (_sender, mut signals) = mpsc::unbounded_channel();

    let completion = bounded(supervise_running(
        desktop(1),
        &lifecycle,
        &mut gateway,
        &mut signals,
        IMMEDIATE,
    ))
    .await
    .expect("an absent gateway should not end supervision");

    assert_eq!(completion, LifecycleCompletion::Closed(0));
}

#[tokio::test]
async fn an_explicit_signal_terminates_the_desktop_once() {
    let lifecycle = FakeLifecycle::new();
    let mut gateway = FakeGateway::Pending;
    let (sender, mut signals) = mpsc::unbounded_channel();
    sender.send(130).expect("signal should queue");

    let completion = bounded(supervise_running(
        desktop(1),
        &lifecycle,
        &mut gateway,
        &mut signals,
        NEVER,
    ))
    .await
    .expect("a signal should close the session");

    assert_eq!(completion, LifecycleCompletion::Closed(130));
    assert_eq!(lifecycle.terminations(), 1);
}

#[tokio::test]
async fn a_closed_signal_channel_terminates_the_desktop_with_143() {
    let lifecycle = FakeLifecycle::new();
    let mut gateway = FakeGateway::Pending;
    let (sender, mut signals) = mpsc::unbounded_channel::<i32>();
    drop(sender);

    let completion = bounded(supervise_running(
        desktop(1),
        &lifecycle,
        &mut gateway,
        &mut signals,
        NEVER,
    ))
    .await
    .expect("a closed signal channel should close the session");

    assert_eq!(completion, LifecycleCompletion::Closed(143));
    assert_eq!(lifecycle.terminations(), 1);
}

#[tokio::test]
async fn a_signal_termination_failure_is_propagated() {
    let lifecycle = FakeLifecycle::new().failing_termination();
    let mut gateway = FakeGateway::Pending;
    let (sender, mut signals) = mpsc::unbounded_channel();
    sender.send(143).expect("signal should queue");

    let error = bounded(supervise_running(
        desktop(1),
        &lifecycle,
        &mut gateway,
        &mut signals,
        NEVER,
    ))
    .await
    .expect_err("a failed termination should be reported");

    assert!(matches!(error, HermesDesktopError::DidNotTerminate));
    assert_eq!(lifecycle.terminations(), 1);
}

#[tokio::test]
async fn a_normal_gateway_exit_terminates_the_desktop_and_reports_it_stopped() {
    let lifecycle = FakeLifecycle::new();
    let mut gateway = FakeGateway::Exited;
    let (_sender, mut signals) = mpsc::unbounded_channel();

    let error = bounded(supervise_running(
        desktop(1),
        &lifecycle,
        &mut gateway,
        &mut signals,
        NEVER,
    ))
    .await
    .expect_err("a gateway exit should fail the session");

    assert!(matches!(error, HermesDesktopError::GatewayExited));
    assert_eq!(lifecycle.terminations(), 1);
}

#[tokio::test]
async fn a_gateway_failure_is_preserved_and_still_terminates_the_desktop() {
    let lifecycle = FakeLifecycle::new();
    let mut gateway = FakeGateway::Failed;
    let (_sender, mut signals) = mpsc::unbounded_channel();

    let error = bounded(supervise_running(
        desktop(1),
        &lifecycle,
        &mut gateway,
        &mut signals,
        NEVER,
    ))
    .await
    .expect_err("a gateway failure should fail the session");

    assert!(matches!(error, HermesDesktopError::BindGateway(_)));
    assert_eq!(lifecycle.terminations(), 1);
}

#[tokio::test]
async fn a_termination_failure_outranks_the_gateway_failure() {
    let lifecycle = FakeLifecycle::new().failing_termination();
    let mut gateway = FakeGateway::Failed;
    let (_sender, mut signals) = mpsc::unbounded_channel();

    let error = bounded(supervise_running(
        desktop(1),
        &lifecycle,
        &mut gateway,
        &mut signals,
        NEVER,
    ))
    .await
    .expect_err("a failed termination should be reported");

    assert!(matches!(error, HermesDesktopError::DidNotTerminate));
}

#[tokio::test]
async fn an_identity_error_propagates_without_terminating() {
    let lifecycle =
        FakeLifecycle::new().identities([Err(HermesDesktopError::ProcessCheckFailed(Some(2)))]);
    let mut gateway = FakeGateway::Pending;
    let (_sender, mut signals) = mpsc::unbounded_channel();

    let error = bounded(supervise_running(
        desktop(1),
        &lifecycle,
        &mut gateway,
        &mut signals,
        IMMEDIATE,
    ))
    .await
    .expect_err("an identity error should be reported");

    assert!(matches!(
        error,
        HermesDesktopError::ProcessCheckFailed(Some(2))
    ));
    assert_eq!(lifecycle.terminations(), 0);
}

#[tokio::test]
async fn a_replacement_discovery_error_propagates_without_terminating() {
    let lifecycle = FakeLifecycle::new()
        .identities([Ok(false)])
        .discoveries([Err(HermesDesktopError::ProcessCheckFailed(Some(3)))]);
    let mut gateway = FakeGateway::Pending;
    let (_sender, mut signals) = mpsc::unbounded_channel();

    let error = bounded(supervise_running(
        desktop(1),
        &lifecycle,
        &mut gateway,
        &mut signals,
        IMMEDIATE,
    ))
    .await
    .expect_err("a discovery error should be reported");

    assert!(matches!(
        error,
        HermesDesktopError::ProcessCheckFailed(Some(3))
    ));
    assert_eq!(lifecycle.terminations(), 0);
}
