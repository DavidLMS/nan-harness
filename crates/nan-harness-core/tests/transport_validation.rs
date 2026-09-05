use nan_harness_core::launch_plan::{
    ArtifactLifecycle, ConfigurationOverlay, LaunchPlanValidator, LaunchScopedFile, OverlayFile,
    OverlayFilePolicy, TemporaryArtifactMode, Transport,
};
use nan_harness_core::{ErrorCategory, HarnessKind, LaunchPlan, PlanError};

const DIRECT_PLAN: &str = include_str!("fixtures/launch-plan.direct.json");
const BRIDGE_PLAN: &str = include_str!("fixtures/launch-plan.bridge.json");

#[test]
fn valid_direct_and_bridge_harness_sides_are_accepted() {
    for harness in [
        HarnessKind::OpenCode,
        HarnessKind::Hermes,
        HarnessKind::Pi,
        HarnessKind::Omp,
        HarnessKind::PrimeAgent,
        HarnessKind::DeepSeekHarness,
        HarnessKind::OpenClaw,
        HarnessKind::Cline,
        HarnessKind::QwenCode,
        HarnessKind::KimiCode,
        HarnessKind::Aider,
        HarnessKind::Goose,
    ] {
        let mut plan = direct_plan();
        plan.harness.kind = harness;
        LaunchPlanValidator::validate(&plan).expect("direct harness should accept direct chat");
    }

    for (harness, transport) in [
        (
            HarnessKind::ClaudeCode,
            Transport::AnthropicBridge {
                client_protocol: nan_harness_core::launch_plan::Protocol::AnthropicMessages,
                upstream_protocol: nan_harness_core::launch_plan::Protocol::ChatCompletions,
                listen: nan_harness_core::launch_plan::ListenAddress {
                    host: "127.0.0.1".to_owned(),
                    port: 0,
                },
                provider_credential_ref: secret_ref("nan_api_key"),
                session_token_ref: secret_ref("bridge_session_token"),
            },
        ),
        (
            HarnessKind::Codex,
            Transport::ResponsesBridge {
                client_protocol: nan_harness_core::launch_plan::Protocol::OpenAiResponses,
                upstream_protocol: nan_harness_core::launch_plan::Protocol::ChatCompletions,
                listen: nan_harness_core::launch_plan::ListenAddress {
                    host: "127.0.0.1".to_owned(),
                    port: 0,
                },
                provider_credential_ref: secret_ref("nan_api_key"),
                session_token_ref: secret_ref("bridge_session_token"),
            },
        ),
    ] {
        let mut plan = bridge_plan();
        plan.harness.kind = harness;
        plan.transport = transport;
        LaunchPlanValidator::validate(&plan).expect("bridge harness should accept its bridge");
    }

    let mut fx_plan = bridge_plan();
    fx_plan.harness.kind = HarnessKind::Fx;
    fx_plan.transport = Transport::FxGatewayBridge {
        listen: nan_harness_core::launch_plan::ListenAddress {
            host: "127.0.0.1".to_owned(),
            port: 0,
        },
        provider_credential_ref: secret_ref("nan_api_key"),
        session_token_ref: secret_ref("bridge_session_token"),
    };
    LaunchPlanValidator::validate(&fx_plan).expect("fx should accept its gateway bridge");
}

#[test]
fn transport_rejects_kind_role_protocol_and_endpoint_mismatches() {
    let mut wrong_kind = direct_plan();
    wrong_kind.harness.kind = HarnessKind::ClaudeCode;
    assert_transport_mismatch(&wrong_kind);

    let mut direct_protocol = direct_plan();
    if let Transport::DirectChat { protocol, .. } = &mut direct_protocol.transport {
        *protocol = nan_harness_core::launch_plan::Protocol::AnthropicMessages;
    }
    assert_invalid_field(&direct_protocol, "transport.protocol");

    let mut bridge_client = bridge_plan();
    if let Transport::AnthropicBridge {
        client_protocol, ..
    } = &mut bridge_client.transport
    {
        *client_protocol = nan_harness_core::launch_plan::Protocol::OpenAiResponses;
    }
    assert_invalid_field(&bridge_client, "transport");

    let mut bridge_upstream = bridge_plan();
    if let Transport::AnthropicBridge {
        upstream_protocol, ..
    } = &mut bridge_upstream.transport
    {
        *upstream_protocol = nan_harness_core::launch_plan::Protocol::OpenAiResponses;
    }
    assert_invalid_field(&bridge_upstream, "transport");

    let mut non_loopback = bridge_plan();
    if let Transport::AnthropicBridge { listen, .. } = &mut non_loopback.transport {
        listen.host = "0.0.0.0".to_owned();
    }
    assert_invalid_field(&non_loopback, "transport.listen.host");

    for base_url in ["ftp://provider", "https://provider path", "https://"] {
        let mut plan = direct_plan();
        if let Transport::DirectChat {
            base_url: value, ..
        } = &mut plan.transport
        {
            *value = base_url.to_owned();
        }
        assert_invalid_field(&plan, "transport.baseUrl");
    }
}

#[test]
fn direct_and_bridge_cleanup_must_match_transport_and_remove_resources() {
    let mut direct = direct_plan();
    direct.cleanup.terminate_bridge = true;
    assert_invalid_field(&direct, "cleanup.terminateBridge");

    let mut bridge = bridge_plan();
    bridge.cleanup.terminate_bridge = false;
    assert_invalid_field(&bridge, "cleanup.terminateBridge");

    let mut artifact = direct_plan();
    artifact.cleanup.delete_temporary_artifacts = false;
    assert_invalid_field(&artifact, "cleanup.deleteTemporaryArtifacts");

    let mut overlay = direct_plan();
    overlay.temporary_artifacts.clear();
    overlay.configuration_overlays.push(ConfigurationOverlay {
        id: "user-config".to_owned(),
        path_hint: "config".to_owned(),
        source_path: "{runtime:user_home}".to_owned(),
        files: vec![OverlayFile {
            path: "settings.json".to_owned(),
            mode: TemporaryArtifactMode::OwnerFile,
            content_template: "{}".to_owned(),
            policy: OverlayFilePolicy::Replace,
        }],
        lifecycle: ArtifactLifecycle::Launch,
    });
    overlay.cleanup.delete_temporary_artifacts = false;
    assert_invalid_field(&overlay, "cleanup.deleteTemporaryArtifacts");

    let mut scoped_file = direct_plan();
    scoped_file.temporary_artifacts.clear();
    scoped_file.launch_scoped_files.push(LaunchScopedFile {
        id: "profile-file".to_owned(),
        directory: "{runtime:user_home}".to_owned(),
        file_name: "nan-harness-launch_profile".to_owned(),
        ownership_prefix: "nan-harness-launch_".to_owned(),
        mode: TemporaryArtifactMode::OwnerFile,
        content_template: "profile".to_owned(),
        lifecycle: ArtifactLifecycle::Launch,
    });
    scoped_file.cleanup.delete_temporary_artifacts = false;
    assert_invalid_field(&scoped_file, "cleanup.deleteTemporaryArtifacts");
}

#[test]
fn required_fields_and_working_directory_are_checked_independently() {
    let mut schema = direct_plan();
    schema.schema_version = 1;
    assert_invalid_field(&schema, "schemaVersion");

    let mut executable = direct_plan();
    executable.harness.executable.clear();
    assert_invalid_field(&executable, "harness.executable");

    let mut detected_version = direct_plan();
    detected_version.harness.detected_version.clear();
    assert_invalid_field(&detected_version, "harness.detectedVersion");

    let mut requested_id = direct_plan();
    requested_id.model.requested_id.clear();
    assert_invalid_field(&requested_id, "model");

    let mut resolved_id = direct_plan();
    resolved_id.model.resolved_id.clear();
    assert_invalid_field(&resolved_id, "model");

    let mut working_directory = direct_plan();
    working_directory.process.working_directory = "relative/project".to_owned();
    assert_invalid_field(&working_directory, "process.workingDirectory");
}

#[test]
fn grace_period_accepts_boundary_and_rejects_values_above_it() {
    let mut plan = direct_plan();
    plan.cleanup.grace_period_ms = 30_000;
    LaunchPlanValidator::validate(&plan).expect("maximum grace period should be valid");

    plan.cleanup.grace_period_ms = 30_001;
    assert_invalid_field(&plan, "cleanup.gracePeriodMs");
}

#[test]
fn transport_secret_references_are_required_in_the_child_environment() {
    let mut direct = direct_plan();
    if let Transport::DirectChat {
        credential_target, ..
    } = &mut direct.transport
    {
        *credential_target = "MISSING_SECRET".to_owned();
    }
    assert_missing_secret(&direct, "MISSING_SECRET");

    let mut bridge = bridge_plan();
    bridge.environment.secrets.clear();
    assert_missing_secret(&bridge, "bridge_session_token");
}

fn direct_plan() -> LaunchPlan {
    serde_json::from_str(DIRECT_PLAN).expect("valid direct plan fixture")
}

fn bridge_plan() -> LaunchPlan {
    serde_json::from_str(BRIDGE_PLAN).expect("valid bridge plan fixture")
}

fn secret_ref(value: &str) -> nan_harness_core::SecretRef {
    nan_harness_core::SecretRef::new(value).expect("valid secret reference")
}

fn assert_transport_mismatch(plan: &LaunchPlan) {
    let error = LaunchPlanValidator::validate(plan).expect_err("transport should mismatch");
    assert!(matches!(error, PlanError::TransportMismatch { .. }));
    assert_eq!(error.category(), ErrorCategory::Contract);
}

fn assert_invalid_field(plan: &LaunchPlan, field: &'static str) {
    let error = LaunchPlanValidator::validate(plan).expect_err("plan should be invalid");
    assert!(matches!(error, PlanError::InvalidField { field: actual, .. } if actual == field));
    assert_eq!(error.category(), ErrorCategory::Contract);
}

fn assert_missing_secret(plan: &LaunchPlan, reference: &str) {
    let error = LaunchPlanValidator::validate(plan).expect_err("secret should be missing");
    assert!(
        matches!(error, PlanError::MissingSecretReference { reference: ref actual } if actual == reference)
    );
    assert_eq!(error.category(), ErrorCategory::Security);
}
