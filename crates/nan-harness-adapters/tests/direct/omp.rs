use super::support::{assert_direct_secret, context, plan};
use nan_harness_adapters::OmpAdapter;
use nan_harness_core::HarnessKind;
use nan_harness_core::launch_plan::{NAN_SEARCH_BLOCK_BEGIN, PI_MODEL_CATALOG_PLACEHOLDER};

#[test]
fn omp_routes_every_model_role_through_private_launch_artifacts() {
    let plan = plan(
        &OmpAdapter,
        &context(HarnessKind::Omp, vec!["--continue".to_owned()]),
    );
    assert_eq!(plan.temporary_artifacts.len(), 2);
    assert_eq!(
        plan.process.arguments,
        [
            "--extension",
            "{artifact:omp-provider-extension}",
            "--config",
            "{artifact:omp-launch-config}",
            "--model",
            "nan/qwen3.6",
            "--models",
            "nan/*",
            "--continue"
        ]
    );
    let extension = plan.temporary_artifacts[0]
        .content_template
        .as_deref()
        .expect("OMP extension");
    assert!(extension.contains("@oh-my-pi/pi-ai"));
    assert!(extension.contains("pi.registerProvider(\"nan\""));
    assert!(extension.contains("ctx.invokeTool"));
    assert!(extension.contains("hybridProviders"));
    assert!(extension.contains(PI_MODEL_CATALOG_PLACEHOLDER));
    assert!(extension.contains(NAN_SEARCH_BLOCK_BEGIN));
    let config = plan.temporary_artifacts[1]
        .content_template
        .as_deref()
        .expect("OMP config");
    assert!(config.contains("enabledModels:"));
    assert!(config.contains("advisor: \"nan/qwen3.6\""));
    assert!(config.contains("modelFallback: false"));
    for name in [
        "PI_CONFIG_FILES",
        "PI_SMOL_MODEL",
        "PI_SLOW_MODEL",
        "PI_PLAN_MODEL",
    ] {
        assert!(plan.environment.remove.contains(name));
    }
    assert_direct_secret(&plan, "NAN_API_KEY");
}
