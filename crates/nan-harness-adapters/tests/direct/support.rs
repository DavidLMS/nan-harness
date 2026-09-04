use nan_harness_core::launch_plan::{
    BRIDGE_BASE_URL_PLACEHOLDER, LaunchId, NAN_SEARCH_BLOCK_BEGIN, NAN_SEARCH_BLOCK_END,
    ObservabilityFormat, PROVIDER_BASE_URL_PLACEHOLDER, Transport,
};
use nan_harness_core::model::{ModelAvailability, ProfileSource, QualificationStatus};
use nan_harness_core::{
    DetectedHarness, HarnessAdapter, HarnessCapability, HarnessKind, LaunchPlan, PlanContext,
    ResolvedModel, VersionStatus, WebSearchPolicy, build_validated_plan,
};

pub(super) fn plan(adapter: &dyn HarnessAdapter, context: &PlanContext) -> LaunchPlan {
    build_validated_plan(adapter, context).expect("adapter should produce a valid plan")
}

pub(super) fn context(kind: HarnessKind, user_arguments: Vec<String>) -> PlanContext {
    PlanContext {
        launch_id: LaunchId::new("launch_01directadapter").expect("valid launch ID"),
        harness: DetectedHarness {
            kind,
            executable: format!("/usr/local/bin/{}", kind.binary_name()),
            detected_version: "test-version".to_owned(),
            version_status: VersionStatus::Tested,
            capabilities: (kind == HarnessKind::Codex)
                .then_some(HarnessCapability::CodexConfigProfile)
                .into_iter()
                .collect(),
        },
        model: ResolvedModel {
            requested_id: "qwen3.6".to_owned(),
            resolved_id: "qwen3.6".to_owned(),
            reasoning_selection: None,
            availability: ModelAvailability::Discovered,
            profile_source: ProfileSource::Bundled,
            qualification: QualificationStatus::Qualified,
            warnings: Vec::new(),
        },
        working_directory: "/workspace/project".to_owned(),
        user_arguments,
        web_search_policy: WebSearchPolicy::Auto,
        observability_format: ObservabilityFormat::Human,
    }
}

pub(super) fn assert_direct_secret(plan: &LaunchPlan, target: &str) {
    assert!(matches!(
        &plan.transport,
        Transport::DirectChat {
            base_url,
            credential_target,
            ..
        } if base_url == PROVIDER_BASE_URL_PLACEHOLDER && credential_target == target
    ));
    assert_eq!(
        plan.environment
            .secrets
            .get(target)
            .expect("credential target should be mapped")
            .as_str(),
        "nan_api_key"
    );
    assert!(
        !serde_json::to_string(plan)
            .expect("plan should serialize")
            .contains("nan-secret-value")
    );
}

pub(super) fn without_search_block(template: &str) -> String {
    let begin = template.find(NAN_SEARCH_BLOCK_BEGIN).expect("search begin");
    let end = template.find(NAN_SEARCH_BLOCK_END).expect("search end");
    format!(
        "{}{}",
        &template[..begin],
        &template[end + NAN_SEARCH_BLOCK_END.len()..]
    )
}

pub(super) fn with_search_block(template: &str) -> String {
    template
        .replace(NAN_SEARCH_BLOCK_BEGIN, "")
        .replace(NAN_SEARCH_BLOCK_END, "")
}

pub(super) fn assert_search_mcp(template: &str, token_environment: &str) {
    let enabled = template
        .replace(NAN_SEARCH_BLOCK_BEGIN, "")
        .replace(NAN_SEARCH_BLOCK_END, "");
    let value: serde_json::Value =
        serde_json::from_str(&enabled).expect("enabled MCP template should be JSON");
    let server = &value["mcpServers"]["nan-search"];
    assert_eq!(server["command"], "nan-harness");
    assert_eq!(server["args"][0], "__search-mcp");
    assert_eq!(server["args"][4], token_environment);
    assert!(
        server["args"][2]
            .as_str()
            .expect("endpoint")
            .contains(BRIDGE_BASE_URL_PLACEHOLDER)
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&without_search_block(template))
            .expect("disabled MCP template should be JSON"),
        serde_json::json!({})
    );
}
