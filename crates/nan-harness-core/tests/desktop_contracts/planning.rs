use nan_harness_core::launch_plan::LaunchPlan;
use nan_harness_core::{
    HarnessAdapter, HarnessKind, PlanContext, PlanError, WebSearchPolicy, build_validated_plan,
};
use std::cell::{Cell, RefCell};

const DIRECT_PLAN: &str = include_str!("../fixtures/launch-plan.direct.json");

struct FakeAdapter {
    kind: HarnessKind,
    result: RefCell<Option<Result<LaunchPlan, PlanError>>>,
    calls: Cell<u8>,
}

impl FakeAdapter {
    fn new(kind: HarnessKind, result: Result<LaunchPlan, PlanError>) -> Self {
        Self {
            kind,
            result: RefCell::new(Some(result)),
            calls: Cell::new(0),
        }
    }
}

impl HarnessAdapter for FakeAdapter {
    fn kind(&self) -> HarnessKind {
        self.kind
    }

    fn plan(&self, _context: &PlanContext) -> Result<LaunchPlan, PlanError> {
        self.calls.set(self.calls.get().saturating_add(1));
        self.result
            .borrow_mut()
            .take()
            .expect("adapter should only be called once")
    }
}

fn direct_plan() -> LaunchPlan {
    serde_json::from_str(DIRECT_PLAN).expect("direct plan fixture should be valid")
}

fn context_for(plan: &LaunchPlan, user_arguments: Vec<String>) -> PlanContext {
    PlanContext {
        launch_id: plan.launch_id.clone(),
        harness: plan.harness.clone(),
        model: plan.model.clone(),
        working_directory: plan.process.working_directory.clone(),
        user_arguments,
        web_search_policy: WebSearchPolicy::Auto,
        observability_format: plan.observability.format,
    }
}

#[test]
fn validated_plans_return_the_adapter_result_without_changing_context() {
    let plan = direct_plan();
    let adapter = FakeAdapter::new(HarnessKind::OpenCode, Ok(plan.clone()));
    let context = context_for(&plan, vec!["--synthetic".to_owned()]);
    let unchanged_context = context.clone();

    let validated = build_validated_plan(&adapter, &context)
        .expect("valid adapter plan should pass validation");

    assert_eq!(validated, plan);
    assert_eq!(context, unchanged_context);
    assert_eq!(adapter.calls.get(), 1);
}

#[test]
fn adapter_mismatch_is_typed_before_the_adapter_plans() {
    let mut requested = direct_plan();
    requested.harness.kind = HarnessKind::Pi;
    let adapter = FakeAdapter::new(
        HarnessKind::OpenCode,
        Err(PlanError::InvalidField {
            field: "adapter",
            message: "adapter must not be called".to_owned(),
        }),
    );

    let result = build_validated_plan(&adapter, &context_for(&requested, Vec::new()));

    assert_eq!(
        result,
        Err(PlanError::AdapterMismatch {
            adapter: HarnessKind::OpenCode,
            requested: HarnessKind::Pi,
        })
    );
    assert_eq!(adapter.calls.get(), 0);
}

#[test]
fn typed_adapter_plan_errors_are_returned_before_validation() {
    let adapter_error = PlanError::InvalidField {
        field: "synthetic-adapter-field",
        message: "adapter error has typed precedence".to_owned(),
    };
    let adapter = FakeAdapter::new(HarnessKind::OpenCode, Err(adapter_error));
    let context = context_for(&direct_plan(), Vec::new());

    let result = build_validated_plan(&adapter, &context);

    assert!(matches!(
        result,
        Err(PlanError::InvalidField {
            field: "synthetic-adapter-field",
            message
        }) if message == "adapter error has typed precedence"
    ));
    assert_eq!(adapter.calls.get(), 1);
}

#[test]
fn invalid_adapter_plans_fail_semantic_validation() {
    let mut plan = direct_plan();
    plan.schema_version = 1;
    let adapter = FakeAdapter::new(HarnessKind::OpenCode, Ok(plan));
    let context = context_for(&direct_plan(), Vec::new());

    let result = build_validated_plan(&adapter, &context);

    assert_eq!(
        result,
        Err(PlanError::InvalidField {
            field: "schemaVersion",
            message: "only schema version 2 is supported".to_owned(),
        })
    );
    assert_eq!(adapter.calls.get(), 1);
}
