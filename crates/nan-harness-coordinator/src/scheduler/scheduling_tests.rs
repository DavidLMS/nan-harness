use super::state::{BudgetState, ScopeState, observe};
use super::{AcquireRequest, ObservationRequest, Pending, Scheduler, schedule};
use crate::{AttemptOutcome, RequestLane, RequestPriority, TokenUsage};
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

fn request(launch_id: &str) -> AcquireRequest {
    AcquireRequest {
        scope: "credential".to_owned(),
        launch_id: launch_id.to_owned(),
        lane: RequestLane::Inference,
        priority: RequestPriority::Foreground,
        enqueued_at: Instant::now(),
        budget_tokens: None,
    }
}

fn background_request(launch_id: &str) -> AcquireRequest {
    AcquireRequest {
        priority: RequestPriority::Background,
        ..request(launch_id)
    }
}

fn control_request(launch_id: &str) -> AcquireRequest {
    AcquireRequest {
        lane: RequestLane::Control,
        priority: RequestPriority::Background,
        ..request(launch_id)
    }
}

#[test]
fn an_existing_budget_rejects_budgetless_inference_but_not_control() {
    let mut state = ScopeState {
        budgets: HashMap::from([("launch".to_owned(), BudgetState::new(100))]),
        pending: VecDeque::new(),
        ..ScopeState::default()
    };
    let (inference_reply, _inference_response) = tokio::sync::oneshot::channel();
    let (control_reply, mut control_response) = tokio::sync::oneshot::channel();
    state.pending.push_back(Pending {
        request: request("launch"),
        reply: inference_reply,
    });
    state.pending.push_back(Pending {
        request: control_request("launch"),
        reply: control_reply,
    });

    let mut next_lease_id = 1;
    schedule(
        &mut HashMap::from([("scope".to_owned(), state)]),
        &mut next_lease_id,
    );
    let grant = control_response
        .try_recv()
        .expect("control request should remain eligible");
    assert!(grant.rejection.is_none());
}

#[test]
fn budget_accounting_is_shared_across_models_and_missing_usage_blocks() {
    let mut state = ScopeState::default();
    state
        .budgets
        .insert("launch".to_owned(), BudgetState::new(100));
    let usage = TokenUsage {
        input_tokens: 30,
        output_tokens: 12,
    };
    observe(
        &mut state,
        &ObservationRequest {
            scope: "scope".to_owned(),
            outcome: AttemptOutcome::Success,
            retry_after: None,
            growth_eligible: false,
            foreground_inference: true,
            headers_elapsed: None,
            launch_id: "launch".to_owned(),
            budget_tokens: Some(100),
            usage: Some(usage),
        },
    );
    let budget = &state.budgets["launch"];
    assert_eq!(budget.consumed, 42);
    assert!(!budget.accounting_blocked);

    observe(
        &mut state,
        &ObservationRequest {
            scope: "scope".to_owned(),
            outcome: AttemptOutcome::Success,
            retry_after: None,
            growth_eligible: false,
            foreground_inference: true,
            headers_elapsed: None,
            launch_id: "launch".to_owned(),
            budget_tokens: Some(100),
            usage: None,
        },
    );
    assert!(state.budgets["launch"].accounting_blocked);
}

#[test]
fn retry_cooldown_blocks_subsequent_scope_requests_until_expiration() {
    use super::state::CooldownDeadline;
    for deadline in [
        CooldownDeadline::Until(Instant::now() + Duration::from_mins(1)),
        CooldownDeadline::Unrepresentable,
    ] {
        let (reply, mut response) = tokio::sync::oneshot::channel();
        let mut scopes = HashMap::from([(
            "credential".to_owned(),
            ScopeState {
                cooldown_until: Some(deadline),
                pending: VecDeque::from([Pending {
                    request: request("next"),
                    reply,
                }]),
                ..ScopeState::default()
            },
        )]);
        let mut next_lease_id = 1;
        schedule(&mut scopes, &mut next_lease_id);
        assert!(matches!(
            response.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        assert_eq!(scopes["credential"].pending.len(), 1);
        scopes.get_mut("credential").expect("scope").cooldown_until =
            Some(CooldownDeadline::Until(Instant::now()));
        schedule(&mut scopes, &mut next_lease_id);
        assert!(response.try_recv().is_ok());
    }
}

#[test]
fn disconnected_waiters_are_removed_even_while_capacity_is_full() {
    let (reply, response) = tokio::sync::oneshot::channel();
    drop(response);
    let mut scopes = HashMap::from([(
        "credential".to_owned(),
        ScopeState {
            active: 1,
            window: 1,
            pending: VecDeque::from([Pending {
                request: request("disconnected"),
                reply,
            }]),
            ..ScopeState::default()
        },
    )]);

    let mut next_lease_id = 1;
    schedule(&mut scopes, &mut next_lease_id);

    assert!(scopes["credential"].pending.is_empty());
}

#[tokio::test]
async fn requests_wait_for_capacity_and_resume_after_release() {
    let temporary = tempfile::tempdir().expect("temporary directory should exist");
    let scheduler = Scheduler::start(temporary.path().join("capacity.json"));
    scheduler
        .acquire(request("codex"))
        .await
        .expect("first grant");
    scheduler
        .acquire(request("pi"))
        .await
        .expect("second grant");

    let pending_scheduler = scheduler.clone();
    let mut pending =
        tokio::spawn(async move { pending_scheduler.acquire(request("claude")).await });
    assert!(
        tokio::time::timeout(Duration::from_millis(60), &mut pending)
            .await
            .is_err()
    );

    scheduler.release("credential".to_owned(), true);
    let grant = tokio::time::timeout(Duration::from_millis(200), pending)
        .await
        .expect("queued request should resume")
        .expect("queued task should finish")
        .expect("queued request should receive a grant");
    assert!(grant.queued >= Duration::from_millis(50));
}

#[tokio::test]
async fn ten_launches_eventually_receive_capacity_without_starvation() {
    let temporary = tempfile::tempdir().expect("temporary directory should exist");
    let scheduler = Scheduler::start(temporary.path().join("capacity.json"));
    let mut launches = tokio::task::JoinSet::new();
    for index in 0..10 {
        let scheduler = scheduler.clone();
        launches.spawn(async move {
            let launch = format!("launch-{index}");
            scheduler.acquire(request(&launch)).await.map(|_| launch)
        });
    }

    let mut completed = Vec::new();
    while let Some(result) = tokio::time::timeout(Duration::from_secs(1), launches.join_next())
        .await
        .expect("a queued launch should receive capacity")
    {
        let launch = result
            .expect("launch task should finish")
            .expect("launch should receive a grant");
        completed.push(launch);
        scheduler.release("credential".to_owned(), true);
    }
    completed.sort();
    assert_eq!(
        completed,
        (0..10)
            .map(|index| format!("launch-{index}"))
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn control_pressure_does_not_qualify_inference_capacity_growth() {
    let temporary = tempfile::tempdir().expect("temporary directory should exist");
    let scheduler = Scheduler::start(temporary.path().join("capacity.json"));
    scheduler
        .acquire(control_request("discovery"))
        .await
        .expect("control request should receive a grant");
    let grant = scheduler
        .acquire(request("interactive"))
        .await
        .expect("inference request should receive a grant");

    assert!(!grant.growth_eligible);
}

#[tokio::test]
async fn a_different_launch_is_selected_before_the_previous_launch() {
    let temporary = tempfile::tempdir().expect("temporary directory should exist");
    let scheduler = Scheduler::start(temporary.path().join("capacity.json"));
    scheduler
        .acquire(request("codex"))
        .await
        .expect("first grant");
    scheduler
        .acquire(request("codex"))
        .await
        .expect("second grant");

    let same_scheduler = scheduler.clone();
    let mut same = tokio::spawn(async move { same_scheduler.acquire(request("codex")).await });
    let other_scheduler = scheduler.clone();
    let other = tokio::spawn(async move { other_scheduler.acquire(request("pi")).await });
    tokio::time::sleep(Duration::from_millis(30)).await;
    scheduler.release("credential".to_owned(), true);

    tokio::time::timeout(Duration::from_millis(200), other)
        .await
        .expect("different launch should be selected")
        .expect("different launch task should finish")
        .expect("different launch should receive a grant");
    assert!(
        tokio::time::timeout(Duration::from_millis(40), &mut same)
            .await
            .is_err()
    );
    scheduler.release("credential".to_owned(), true);
    tokio::time::timeout(Duration::from_millis(200), same)
        .await
        .expect("same launch should eventually resume")
        .expect("same launch task should finish")
        .expect("same launch should receive a grant");
}

#[tokio::test]
async fn foreground_requests_pass_queued_background_work() {
    let temporary = tempfile::tempdir().expect("temporary directory should exist");
    let scheduler = Scheduler::start(temporary.path().join("capacity.json"));
    scheduler
        .acquire(request("first"))
        .await
        .expect("first grant");
    scheduler
        .acquire(request("second"))
        .await
        .expect("second grant");

    let background_scheduler = scheduler.clone();
    let mut background = tokio::spawn(async move {
        background_scheduler
            .acquire(background_request("system"))
            .await
    });
    let foreground_scheduler = scheduler.clone();
    let foreground =
        tokio::spawn(async move { foreground_scheduler.acquire(request("interactive")).await });
    tokio::time::sleep(Duration::from_millis(30)).await;
    scheduler.release("credential".to_owned(), true);

    tokio::time::timeout(Duration::from_millis(200), foreground)
        .await
        .expect("foreground should be selected")
        .expect("foreground task should finish")
        .expect("foreground should receive a grant");
    assert!(
        tokio::time::timeout(Duration::from_millis(40), &mut background)
            .await
            .is_err()
    );
}
