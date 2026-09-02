#[allow(clippy::wildcard_imports)]
use super::*;

pub(super) fn print_dry_run(arguments: &ClaudeDesktopArgs) -> Result<i32, CliError> {
    let plan = dry_run_plan(arguments);
    println!(
        "{}",
        serde_json::to_string_pretty(&plan).map_err(ClaudeDesktopError::SerializeReceipt)?
    );
    Ok(0)
}

pub(super) fn dry_run_plan(arguments: &ClaudeDesktopArgs) -> DesktopLaunchPlan {
    let mut plan = DesktopLaunchPlan::new(
        DesktopHarnessKind::Claude,
        DesktopTransport::AnthropicBridge,
    );
    plan.executable.clone_from(&arguments.executable);
    plan.selected_model.clone_from(&arguments.model);
    plan.private_diagnostics = arguments.show_auto;
    plan.web_search_policy = if arguments.search.no_search {
        WebSearchPolicy::Disabled
    } else if arguments.search.force_search {
        WebSearchPolicy::Force
    } else {
        WebSearchPolicy::Auto
    };
    plan
}

pub(super) fn restore_command(
    paths: &DesktopPaths,
    process: &SystemDesktopProcess,
) -> Result<i32, CliError> {
    let _lock = SessionLock::acquire(&paths.lock)?;
    if process.is_running()? {
        return Err(ClaudeDesktopError::AlreadyRunning.into());
    }
    match restore_receipt(paths) {
        Ok(()) => eprintln!("Claude Desktop configuration restored."),
        Err(ClaudeDesktopError::NoReceipt) => {
            eprintln!("No Claude Desktop session needs recovery.");
        }
        Err(error) => return Err(error.into()),
    }
    Ok(0)
}

pub(super) fn append_diagnostics(
    target: &mut Vec<BridgeDiagnostic>,
    diagnostics: Vec<BridgeDiagnostic>,
) {
    for diagnostic in diagnostics {
        if !target.contains(&diagnostic) {
            target.push(diagnostic);
        }
    }
}

pub(super) async fn run_ready_session(
    paths: &DesktopPaths,
    process: &impl DesktopProcess,
    bridge: &RunningClaudeDesktopBridge,
    show_auto: bool,
) -> Result<i32, ClaudeDesktopError> {
    let receipt = Receipt::capture(paths)?;
    if let Err(error) = receipt.write(&paths.receipt) {
        Receipt::remove_backups(paths);
        return Err(error);
    }
    let apply = bridge.with_session_token(|token| apply_gateway(paths, bridge.base_url(), token));
    if let Err(error) = apply {
        return restore_after(paths, Err(error));
    }
    let activities = show_auto.then(|| bridge.subscribe_activities());
    if let Err(error) = process.launch() {
        return complete_and_restore(paths, process, Err(error)).await;
    }
    eprintln!("{}", launch_message(show_auto));
    let activity_logger =
        activities.map(|activities| tokio::spawn(log_bridge_activities(activities)));
    let completion = wait_for_exit_or_signal(process).await;
    if let Some(activity_logger) = activity_logger {
        activity_logger.abort();
    }
    complete_and_restore(paths, process, completion).await
}

pub(super) fn launch_message(show_auto: bool) -> &'static str {
    if show_auto {
        "Claude Desktop launched through NaN. Auto traces will appear here and may contain private data."
    } else {
        "Claude Desktop launched through NaN."
    }
}

async fn log_bridge_activities(mut activities: tokio::sync::broadcast::Receiver<BridgeActivity>) {
    loop {
        match activities.recv().await {
            Ok(activity) => eprintln!("{}", render_bridge_activity(&activity)),
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                eprintln!("[Auto] {skipped} permission review events were omitted.");
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
        }
    }
}

pub(super) fn render_bridge_activity(activity: &BridgeActivity) -> String {
    match activity {
        BridgeActivity::AuthenticatedClient => {
            "[Bridge] Claude Desktop authenticated to the isolated NaN bridge.".to_owned()
        }
        BridgeActivity::ClaudeAutoModeReview {
            review_id,
            stage,
            model_id,
            request,
        } => {
            let stage = match stage {
                ClaudeAutoModeReviewStage::Initial => "stage 1",
                ClaudeAutoModeReviewStage::FollowUp => "stage 2",
            };
            format!(
                "[Auto #{review_id}] Claude requested a permission review ({stage}, classifier {model_id}).\n[Auto #{review_id}] NaN request:\n{}",
                render_trace_payload(request)
            )
        }
        BridgeActivity::ClaudeAutoModeReviewResponse {
            review_id,
            status,
            response,
        } => {
            format!(
                "[Auto #{review_id}] NaN response (HTTP {status}):\n{}",
                render_trace_payload(response)
            )
        }
        BridgeActivity::ClaudeAutoModeReviewFailed {
            review_id,
            error_code,
        } => {
            format!(
                "[Auto #{review_id}] NaN request failed before a response was received ({error_code})."
            )
        }
    }
}

fn render_trace_payload(payload: &nan_harness_runtime::ClaudeAutoModeTracePayload) -> String {
    payload.with_contents(|contents| {
        serde_json::from_str::<Value>(contents).map_or_else(
            |_| contents.to_owned(),
            |value| serde_json::to_string_pretty(&value).unwrap_or_else(|_| contents.to_owned()),
        )
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WaitOutcome {
    Exited,
    Signaled(i32),
}

pub(super) async fn wait_for_exit_or_signal(
    process: &impl DesktopProcess,
) -> Result<WaitOutcome, ClaudeDesktopError> {
    let mut observed_running = false;
    let mut startup_polls = 0_u8;
    let signal = termination_signal();
    tokio::pin!(signal);
    loop {
        if let Some(outcome) =
            observe_process_state(process, &mut observed_running, &mut startup_polls)?
        {
            return Ok(outcome);
        }
        if let Some(signal) = wait_for_poll_or_signal(signal.as_mut()).await {
            return Ok(WaitOutcome::Signaled(signal));
        }
    }
}

fn observe_process_state(
    process: &impl DesktopProcess,
    observed_running: &mut bool,
    startup_polls: &mut u8,
) -> Result<Option<WaitOutcome>, ClaudeDesktopError> {
    match process.is_running()? {
        true => *observed_running = true,
        false if *observed_running => return Ok(Some(WaitOutcome::Exited)),
        false => check_startup_limit(startup_polls)?,
    }
    Ok(None)
}

fn check_startup_limit(startup_polls: &mut u8) -> Result<(), ClaudeDesktopError> {
    *startup_polls = startup_polls.saturating_add(1);
    if *startup_polls >= 40 {
        return Err(ClaudeDesktopError::DidNotStart);
    }
    Ok(())
}

async fn wait_for_poll_or_signal<F>(signal: std::pin::Pin<&mut F>) -> Option<i32>
where
    F: Future<Output = i32>,
{
    tokio::select! {
        () = tokio::time::sleep(Duration::from_millis(125)) => None,
        signal = signal => Some(signal),
    }
}

async fn terminate_and_wait(process: &impl DesktopProcess) -> Result<(), ClaudeDesktopError> {
    let graceful = process.terminate();
    if graceful.is_ok() && wait_for_termination(process).await.is_ok() {
        return Ok(());
    }
    process.force_terminate()?;
    wait_for_termination(process).await
}

pub(super) async fn complete_and_restore(
    paths: &DesktopPaths,
    process: &impl DesktopProcess,
    completion: Result<WaitOutcome, ClaudeDesktopError>,
) -> Result<i32, ClaudeDesktopError> {
    match completion {
        Ok(WaitOutcome::Exited) => restore_after(paths, Ok(0)),
        Ok(WaitOutcome::Signaled(exit_code)) => {
            terminate_and_wait(process).await?;
            restore_after(paths, Ok(exit_code))
        }
        Err(error) => {
            terminate_and_wait(process).await?;
            restore_after(paths, Err(error))
        }
    }
}

async fn wait_for_termination(process: &impl DesktopProcess) -> Result<(), ClaudeDesktopError> {
    for _ in 0..120 {
        if !process.is_running()? {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(125)).await;
    }
    Err(ClaudeDesktopError::DidNotTerminate)
}

pub(super) fn restore_after(
    paths: &DesktopPaths,
    completion: Result<i32, ClaudeDesktopError>,
) -> Result<i32, ClaudeDesktopError> {
    match (completion, restore_receipt(paths)) {
        (Ok(exit_code), Ok(())) => Ok(exit_code),
        (Err(error), Ok(())) | (_, Err(error)) => Err(error),
    }
}

#[cfg(unix)]
async fn termination_signal() -> i32 {
    let Ok(mut terminate) =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
    else {
        let _ = tokio::signal::ctrl_c().await;
        return 130;
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => 130,
        _ = terminate.recv() => 143,
    }
}

#[cfg(not(unix))]
async fn termination_signal() -> i32 {
    let _ = tokio::signal::ctrl_c().await;
    130
}
