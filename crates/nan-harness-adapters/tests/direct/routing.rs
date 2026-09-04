use super::support::context;
use nan_harness_adapters::{
    AiderAdapter, ClineAdapter, DeepSeekHarnessAdapter, GooseAdapter, HermesAdapter,
    KimiCodeAdapter, OmpAdapter, OpenClawAdapter, OpenCodeAdapter, PiAdapter, QwenCodeAdapter,
};
use nan_harness_core::{HarnessAdapter, HarnessKind, PlanError, build_validated_plan};

#[test]
fn direct_adapters_reject_arguments_that_can_bypass_nan_routing() {
    for (adapter, kind, argument) in [
        (
            &OpenCodeAdapter as &dyn HarnessAdapter,
            HarnessKind::OpenCode,
            "--model=other/model",
        ),
        (
            &HermesAdapter as &dyn HarnessAdapter,
            HarnessKind::Hermes,
            "--provider",
        ),
        (
            &PiAdapter as &dyn HarnessAdapter,
            HarnessKind::Pi,
            "--api-key",
        ),
        (
            &OmpAdapter as &dyn HarnessAdapter,
            HarnessKind::Omp,
            "--config=other.yml",
        ),
        (
            &DeepSeekHarnessAdapter as &dyn HarnessAdapter,
            HarnessKind::DeepSeekHarness,
            "--patch=other.yml",
        ),
        (
            &OpenClawAdapter as &dyn HarnessAdapter,
            HarnessKind::OpenClaw,
            "--model=other/model",
        ),
        (
            &ClineAdapter as &dyn HarnessAdapter,
            HarnessKind::Cline,
            "--config=other",
        ),
        (
            &QwenCodeAdapter as &dyn HarnessAdapter,
            HarnessKind::QwenCode,
            "--fallback-model=other",
        ),
        (
            &KimiCodeAdapter as &dyn HarnessAdapter,
            HarnessKind::KimiCode,
            "--model=other",
        ),
        (
            &AiderAdapter as &dyn HarnessAdapter,
            HarnessKind::Aider,
            "--weak-model=other",
        ),
        (
            &GooseAdapter as &dyn HarnessAdapter,
            HarnessKind::Goose,
            "--model=other",
        ),
    ] {
        let error = build_validated_plan(adapter, &context(kind, vec![argument.to_owned()]))
            .expect_err("routing override should fail");
        assert!(matches!(
            error,
            PlanError::InvalidField {
                field: "process.arguments",
                ..
            }
        ));
    }
}
