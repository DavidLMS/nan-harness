use crate::direct::{
    DirectLaunch, build_direct_plan, provider_environment, validate_routing_arguments,
};
use nan_harness_core::launch_plan::{
    ArtifactLifecycle, ConfigurationOverlay, NAN_SEARCH_BLOCK_BEGIN, NAN_SEARCH_BLOCK_END,
    NAN_SEARCH_CONFIG_PLACEHOLDER, NativeContextLimit, OverlayFile, OverlayFilePolicy,
    PRIME_HOME_PLACEHOLDER, PRIME_PROVIDER_CATALOG_PLACEHOLDER, TemporaryArtifactMode,
};
use nan_harness_core::{LaunchPlan, PlanContext, PlanError};
use std::collections::BTreeSet;

pub(crate) fn uses_native_configuration(version: &str) -> bool {
    semver::Version::parse(version).is_ok_and(|version| version >= semver::Version::new(0, 10, 0))
}

pub(crate) fn native_plan(context: &PlanContext) -> Result<LaunchPlan, PlanError> {
    validate_routing_arguments(
        &context.user_arguments,
        &["--model", "--provider", "--api-key", "--models"],
    )?;
    let mut arguments = vec![
        "--provider".into(),
        if nan_harness_core::coding_model_profile(&context.model.resolved_id).is_some_and(
            |profile| {
                matches!(
                    profile.reasoning,
                    nan_harness_core::ReasoningPolicy::Toggle { .. }
                )
            },
        ) {
            "nan-thinking".into()
        } else {
            "nan".into()
        },
        "--model".into(),
        context.model.resolved_id.clone(),
        "--models".into(),
        "nan/*,nan-thinking/*".into(),
    ];
    arguments.extend(context.user_arguments.iter().cloned());
    let mut environment = provider_environment();
    environment.insert(
        "PRIME_AGENT_CODING_AGENT_DIR".into(),
        "{artifact:prime-home}".into(),
    );
    let server = serde_json::json!({"type": "stdio", "command": "nan-harness",
        "args": ["__search-mcp", "--config", NAN_SEARCH_CONFIG_PLACEHOLDER], "enabled": true});
    let compaction = context
        .context_limit
        .as_ref()
        .and_then(|limit| match limit.native {
            NativeContextLimit::PiReserve { reserve_tokens } => Some(format!(
                "\"compaction\":{{\"enabled\":true,\"reserveTokens\":{reserve_tokens}}}"
            )),
            _ => None,
        });
    let separator = if compaction.is_some() { "," } else { "" };
    let settings = format!(
        "{{{}{NAN_SEARCH_BLOCK_BEGIN}{separator}\"mcpServers\":{{\"nan-search\":{server}}}{NAN_SEARCH_BLOCK_END}}}",
        compaction.unwrap_or_default()
    );
    build_direct_plan(
        context,
        DirectLaunch {
            arguments,
            credential_target: "NAN_API_KEY",
            public_environment: environment,
            removed_environment: BTreeSet::new(),
            temporary_artifacts: vec![],
            configuration_overlays: vec![ConfigurationOverlay {
                id: "prime-home".into(),
                path_hint: "prime-agent".into(),
                source_path: PRIME_HOME_PLACEHOLDER.into(),
                lifecycle: ArtifactLifecycle::Launch,
                files: vec![
                    OverlayFile {
                        path: "auth.json".into(),
                        mode: TemporaryArtifactMode::OwnerFile,
                        content_template: r#"{"nan":{"type":"api_key","key":"NAN_API_KEY"},"nan-thinking":{"type":"api_key","key":"NAN_API_KEY"}}"#
                            .into(),
                        policy: OverlayFilePolicy::MergeJson,
                    },
                    OverlayFile {
                        path: "models.json".into(),
                        mode: TemporaryArtifactMode::OwnerFile,
                        content_template: format!(
                            "{{\"providers\":{PRIME_PROVIDER_CATALOG_PLACEHOLDER}}}"
                        ),
                        policy: OverlayFilePolicy::MergeJson,
                    },
                    OverlayFile {
                        path: "settings.json".into(),
                        mode: TemporaryArtifactMode::OwnerFile,
                        content_template: settings,
                        policy: OverlayFilePolicy::MergeJson,
                    },
                ],
            }],
        },
    )
}
