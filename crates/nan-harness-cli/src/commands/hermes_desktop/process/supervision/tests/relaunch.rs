use super::*;

#[tokio::test]
async fn a_present_process_is_returned_before_the_timeout_is_consulted() {
    let process = desktop(7);
    let lifecycle = FakeLifecycle::new().discoveries([Ok(Some(process.clone()))]);
    let mut gateway = FakeGateway::Pending;
    let (_sender, mut signals) = mpsc::unbounded_channel();

    let completion = bounded(wait_for_relaunch(
        &lifecycle,
        &mut gateway,
        &mut signals,
        false,
        NEVER,
        IMMEDIATE,
    ))
    .await
    .expect("an already running desktop should be returned");

    assert_eq!(completion, RelaunchWaitCompletion::Running(process));
}

#[tokio::test]
async fn no_process_within_a_zero_timeout_times_out_without_waiting() {
    let lifecycle = FakeLifecycle::new();
    let mut gateway = FakeGateway::Pending;
    let (_sender, mut signals) = mpsc::unbounded_channel();

    let completion = bounded(wait_for_relaunch(
        &lifecycle,
        &mut gateway,
        &mut signals,
        false,
        NEVER,
        IMMEDIATE,
    ))
    .await
    .expect("an exhausted timeout should be reported");

    assert_eq!(completion, RelaunchWaitCompletion::TimedOut);
}

#[tokio::test]
async fn a_pending_relaunch_can_still_find_the_process() {
    let process = desktop(8);
    let lifecycle =
        FakeLifecycle::new().discoveries([Ok(None), Ok(None), Ok(Some(process.clone()))]);
    let mut gateway = FakeGateway::Pending;
    let (_sender, mut signals) = mpsc::unbounded_channel();

    let completion = bounded(wait_for_relaunch(
        &lifecycle,
        &mut gateway,
        &mut signals,
        false,
        IMMEDIATE,
        NEVER,
    ))
    .await
    .expect("a later relaunch should be observed");

    assert_eq!(completion, RelaunchWaitCompletion::Running(process));
}

#[tokio::test]
async fn the_first_relaunch_interrupt_continues_and_the_second_preserves_recovery() {
    let lifecycle = FakeLifecycle::new();
    let mut gateway = FakeGateway::Pending;
    let (sender, mut signals) = mpsc::unbounded_channel();
    sender.send(130).expect("first interrupt should queue");
    sender.send(130).expect("second interrupt should queue");

    let completion = bounded(wait_for_relaunch(
        &lifecycle,
        &mut gateway,
        &mut signals,
        false,
        NEVER,
        NEVER,
    ))
    .await
    .expect("a second interrupt should preserve recovery");

    assert_eq!(completion, RelaunchWaitCompletion::PreserveRecovery(130));
    assert_eq!(lifecycle.terminations(), 0);
}

#[tokio::test]
async fn an_interrupt_after_an_earlier_one_exits_immediately() {
    let lifecycle = FakeLifecycle::new();
    let mut gateway = FakeGateway::Pending;
    let (sender, mut signals) = mpsc::unbounded_channel();
    sender.send(130).expect("interrupt should queue");

    let completion = bounded(wait_for_relaunch(
        &lifecycle,
        &mut gateway,
        &mut signals,
        true,
        NEVER,
        NEVER,
    ))
    .await
    .expect("an already seen interrupt should preserve recovery");

    assert_eq!(completion, RelaunchWaitCompletion::PreserveRecovery(130));
}

#[tokio::test]
async fn a_termination_signal_during_relaunch_preserves_recovery() {
    let lifecycle = FakeLifecycle::new();
    let mut gateway = FakeGateway::Pending;
    let (sender, mut signals) = mpsc::unbounded_channel();
    sender.send(143).expect("signal should queue");

    let completion = bounded(wait_for_relaunch(
        &lifecycle,
        &mut gateway,
        &mut signals,
        false,
        NEVER,
        NEVER,
    ))
    .await
    .expect("a termination signal should preserve recovery");

    assert_eq!(completion, RelaunchWaitCompletion::PreserveRecovery(143));
}

#[tokio::test]
async fn a_closed_signal_channel_during_relaunch_preserves_recovery_with_143() {
    let lifecycle = FakeLifecycle::new();
    let mut gateway = FakeGateway::Pending;
    let (sender, mut signals) = mpsc::unbounded_channel::<i32>();
    drop(sender);

    let completion = bounded(wait_for_relaunch(
        &lifecycle,
        &mut gateway,
        &mut signals,
        false,
        NEVER,
        NEVER,
    ))
    .await
    .expect("a closed signal channel should preserve recovery");

    assert_eq!(completion, RelaunchWaitCompletion::PreserveRecovery(143));
}

#[tokio::test]
async fn a_relaunch_discovery_error_propagates() {
    let lifecycle =
        FakeLifecycle::new().discoveries([Err(HermesDesktopError::ProcessCheckFailed(Some(4)))]);
    let mut gateway = FakeGateway::Pending;
    let (_sender, mut signals) = mpsc::unbounded_channel();

    let error = bounded(wait_for_relaunch(
        &lifecycle,
        &mut gateway,
        &mut signals,
        false,
        NEVER,
        NEVER,
    ))
    .await
    .expect_err("a discovery error should be reported");

    assert!(matches!(
        error,
        HermesDesktopError::ProcessCheckFailed(Some(4))
    ));
}

#[tokio::test]
async fn a_gateway_exit_during_relaunch_does_not_terminate_the_desktop() {
    let lifecycle = FakeLifecycle::new();
    let mut gateway = FakeGateway::Exited;
    let (_sender, mut signals) = mpsc::unbounded_channel();

    let error = bounded(wait_for_relaunch(
        &lifecycle,
        &mut gateway,
        &mut signals,
        false,
        NEVER,
        NEVER,
    ))
    .await
    .expect_err("a gateway exit should fail the relaunch wait");

    assert!(matches!(error, HermesDesktopError::GatewayExited));
    assert_eq!(lifecycle.terminations(), 0);
}

#[tokio::test]
async fn a_gateway_failure_during_relaunch_is_preserved() {
    let lifecycle = FakeLifecycle::new();
    let mut gateway = FakeGateway::Failed;
    let (_sender, mut signals) = mpsc::unbounded_channel();

    let error = bounded(wait_for_relaunch(
        &lifecycle,
        &mut gateway,
        &mut signals,
        false,
        NEVER,
        NEVER,
    ))
    .await
    .expect_err("a gateway failure should fail the relaunch wait");

    assert!(matches!(error, HermesDesktopError::BindGateway(_)));
    assert_eq!(lifecycle.terminations(), 0);
}
