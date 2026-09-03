use super::*;

#[test]
fn model_selection_precedence_is_explicit_then_remembered_then_default() {
    let remembered = LastSelection {
        model: "remembered-model".to_owned(),
        reasoning: Some(ReasoningSelection::Toggle(true)),
    };
    assert_eq!(
        choose_launch_model(Some("explicit-model"), Some(remembered.clone())),
        LaunchModel {
            id: "explicit-model".to_owned(),
            source: LaunchModelSource::Explicit,
            reasoning: None,
        }
    );
    assert_eq!(
        choose_launch_model(None, Some(remembered)),
        LaunchModel {
            id: "remembered-model".to_owned(),
            source: LaunchModelSource::Remembered,
            reasoning: Some(ReasoningSelection::Toggle(true)),
        }
    );
    assert_eq!(
        choose_launch_model(None, None),
        LaunchModel {
            id: "qwen3.6".to_owned(),
            source: LaunchModelSource::Default,
            reasoning: None,
        }
    );
}

#[test]
fn selections_are_remembered_only_after_eligible_successes() {
    let explicit = LaunchModel {
        id: "explicit-model".to_owned(),
        source: LaunchModelSource::Explicit,
        reasoning: Some(ReasoningSelection::Toggle(true)),
    };
    let fallback = LaunchModel {
        id: "fallback-model".to_owned(),
        source: LaunchModelSource::Fallback,
        reasoning: None,
    };
    let default = LaunchModel {
        id: "qwen3.6".to_owned(),
        source: LaunchModelSource::Default,
        reasoning: None,
    };

    assert_eq!(
        successful_selection(
            HarnessKind::Fx,
            &explicit,
            &execution_report(ExecutionOutcome::Succeeded, None, None),
        ),
        Some(LastSelection {
            model: "explicit-model".to_owned(),
            reasoning: Some(ReasoningSelection::Toggle(true)),
        })
    );
    assert_eq!(
        successful_selection(
            HarnessKind::ClaudeCode,
            &fallback,
            &execution_report(ExecutionOutcome::Succeeded, None, None),
        ),
        Some(LastSelection {
            model: "fallback-model".to_owned(),
            reasoning: None,
        })
    );
    assert_eq!(
        successful_selection(
            HarnessKind::Fx,
            &explicit,
            &execution_report(ExecutionOutcome::Failed, None, None),
        ),
        None
    );
    assert_eq!(
        successful_selection(
            HarnessKind::Fx,
            &explicit,
            &execution_report(
                ExecutionOutcome::Cancelled(SignalKind::Interrupt),
                None,
                None,
            ),
        ),
        None
    );
    assert_eq!(
        successful_selection(
            HarnessKind::Fx,
            &default,
            &execution_report(ExecutionOutcome::Succeeded, None, None),
        ),
        None,
        "an implicit default must not be remembered"
    );
    assert_eq!(
        successful_selection(
            HarnessKind::Codex,
            &default,
            &execution_report(
                ExecutionOutcome::Succeeded,
                Some("qwen3.6"),
                Some(ReasoningSelection::Toggle(true)),
            ),
        ),
        None,
        "observing the implicit default must not make it persistent"
    );
}

#[test]
fn codex_remembers_the_observable_actual_selection() {
    let remembered = LaunchModel {
        id: "remembered-model".to_owned(),
        source: LaunchModelSource::Remembered,
        reasoning: Some(ReasoningSelection::Toggle(false)),
    };
    let actual_reasoning = Some(ReasoningSelection::Effort(ReasoningEffort::High));
    assert_eq!(
        successful_selection(
            HarnessKind::Codex,
            &remembered,
            &execution_report(
                ExecutionOutcome::Succeeded,
                Some("picker-selected-model"),
                actual_reasoning,
            ),
        ),
        Some(LastSelection {
            model: "picker-selected-model".to_owned(),
            reasoning: actual_reasoning,
        })
    );
}

#[test]
fn requested_model_stays_in_sync_with_the_shared_catalog() {
    for model in KNOWN_CODING_MODELS {
        let resolved = offline_requested_model(&LaunchModel {
            id: model.id.to_owned(),
            source: LaunchModelSource::Default,
            reasoning: None,
        })
        .expect("known model should resolve");

        assert_eq!(
            resolved.profile_source,
            ProfileSource::Bundled,
            "known model {} should use bundled metadata",
            model.id
        );
        assert_eq!(
            resolved.qualification,
            QualificationStatus::Qualified,
            "known model {} should be qualified",
            model.id
        );
    }
}

#[test]
fn gateway_escape_hatch_explains_the_security_and_feature_tradeoff() {
    assert_eq!(direct_chat_gateway_notice(false, false), None);
    assert_eq!(
        direct_chat_gateway_notice(true, false),
        Some(
            "warning: Chat Completions gateway disabled for this launch. The harness will receive the provider credential directly; usage accounting and gateway-dependent features are unavailable."
        )
    );
    assert_eq!(
        direct_chat_gateway_notice(true, true),
        Some(
            "note: Chat Completions gateway would be disabled for this launch. The harness would receive the provider credential directly; usage accounting and gateway-dependent features would be unavailable."
        )
    );
}

#[test]
fn requested_model_keeps_unknown_models_generic_and_unknown() {
    let resolved = offline_requested_model(&LaunchModel {
        id: "future-text-model".to_owned(),
        source: LaunchModelSource::Explicit,
        reasoning: None,
    })
    .expect("valid future model should resolve offline");

    assert_eq!(resolved.profile_source, ProfileSource::Generic);
    assert_eq!(resolved.qualification, QualificationStatus::Unknown);
    assert_eq!(resolved.warnings.len(), 1);
}

#[test]
fn explicit_generic_dry_run_is_offline_and_keeps_a_structured_warning() {
    let cli =
        Cli::try_parse_checked_from(["nan", "opencode", "--dry-run", "--model", "future-model"])
            .expect("dry-run command should parse");
    let (_, arguments) = harness_run_arguments(&cli).expect("harness arguments should exist");
    assert!(arguments.dry_run);
    let resolved = offline_requested_model(&LaunchModel {
        id: "future-model".to_owned(),
        source: LaunchModelSource::Explicit,
        reasoning: None,
    })
    .expect("generic model should resolve without discovery");
    assert_eq!(
        resolved.warnings,
        vec![
            "model 'future-model' has no bundled capability profile; using conservative defaults."
        ]
    );
}

#[test]
fn explicit_model_resolution_uses_live_bundled_and_generic_profiles() {
    let qwen = coding_model_profile("qwen3.6").expect("bundled profile should exist");
    let future = coding_model_profile("future-model").expect("generic profile should exist");
    let discovered = vec![qwen, future];

    let live = resolve_explicit_model(
        HarnessKind::Codex,
        &LaunchModel {
            id: "qwen3.6".to_owned(),
            source: LaunchModelSource::Explicit,
            reasoning: None,
        },
        &discovered,
    )
    .expect("discovered explicit model should resolve");
    assert_eq!(live.model.availability, ModelAvailability::Discovered);
    assert_eq!(live.model.profile_source, ProfileSource::Bundled);
    assert_eq!(live.warning, None);
    assert_eq!(live.catalog, discovered);

    let live_generic = resolve_explicit_model(
        HarnessKind::Codex,
        &LaunchModel {
            id: "future-model".to_owned(),
            source: LaunchModelSource::Explicit,
            reasoning: None,
        },
        &discovered,
    )
    .expect("discovered generic model should resolve");
    assert_eq!(
        live_generic.warning.as_deref(),
        Some(
            "warning: model 'future-model' has no bundled capability profile; using conservative defaults."
        )
    );

    let absent_bundled = resolve_explicit_model(
        HarnessKind::Fx,
        &LaunchModel {
            id: "glm5.3-flash".to_owned(),
            source: LaunchModelSource::Explicit,
            reasoning: None,
        },
        &[],
    )
    .expect("absent bundled model should be attempted");
    assert_eq!(
        absent_bundled.model.availability,
        ModelAvailability::ExplicitUndiscovered
    );
    assert_eq!(absent_bundled.model.profile_source, ProfileSource::Bundled);
    assert_eq!(absent_bundled.catalog.len(), 1);
    assert_eq!(
        absent_bundled.warning.as_deref(),
        Some(
            "warning: model 'glm5.3-flash' was not returned by live discovery for this credential; attempting it because you selected it explicitly."
        )
    );

    let absent_generic = resolve_explicit_model(
        HarnessKind::OpenCode,
        &LaunchModel {
            id: "future-model".to_owned(),
            source: LaunchModelSource::Explicit,
            reasoning: None,
        },
        &[],
    )
    .expect("absent generic model should be attempted");
    assert_eq!(absent_generic.model.profile_source, ProfileSource::Generic);
    assert_eq!(absent_generic.catalog.len(), 1);
    assert_eq!(
        absent_generic.warning.as_deref(),
        Some(
            "warning: model 'future-model' was not returned by live discovery and has no bundled capability profile; attempting it with conservative defaults because you selected it explicitly."
        )
    );

    for invalid in ["", " leading-space", "control\u{0007}"] {
        let error = resolve_explicit_model(
            HarnessKind::Codex,
            &LaunchModel {
                id: invalid.to_owned(),
                source: LaunchModelSource::Explicit,
                reasoning: None,
            },
            &[],
        )
        .expect_err("invalid model IDs must fail safely");
        assert!(invalid.is_empty() || !error.to_string().contains(invalid));
    }
    let overlong = "x".repeat(257);
    let error = resolve_explicit_model(
        HarnessKind::Codex,
        &LaunchModel {
            id: overlong.clone(),
            source: LaunchModelSource::Explicit,
            reasoning: None,
        },
        &[],
    )
    .expect_err("overlong model ID must fail safely");
    assert!(!error.to_string().contains(&overlong));
}

#[test]
fn explicit_warning_matrix_and_near_matches_are_deterministic() {
    assert_eq!(
        explicit_model_warning("future-model", true, false, &[]).as_deref(),
        Some(
            "warning: model 'future-model' has no bundled capability profile; using conservative defaults."
        )
    );
    assert_eq!(
        near_model_match("glm53flash", &["glm5.3-flash".to_owned()]),
        Some("glm5.3-flash".to_owned())
    );
    assert_eq!(
        near_model_match("model-c", &["model-a".to_owned(), "model-b".to_owned()]),
        None,
        "equal-distance candidates must not produce a suggestion"
    );
    assert_eq!(
        near_model_match("totally-different", &["qwen3.6".to_owned()]),
        None
    );
    assert_eq!(
        explicit_model_warning("glm53flash", true, true, &["glm5.3-flash".to_owned()]).as_deref(),
        Some(
            "warning: model 'glm53flash' was not returned by live discovery and has no bundled capability profile; attempting it with conservative defaults because you selected it explicitly. Did you mean 'glm5.3-flash'?"
        )
    );
}

#[test]
fn implicit_fallback_prefers_default_then_live_bundled_models_only() {
    let selected = LaunchModel {
        id: "old-model".to_owned(),
        source: LaunchModelSource::Remembered,
        reasoning: None,
    };
    let error = RuntimeError::Bridge(BridgeError::SelectedModelUnavailable {
        model: "old-model".to_owned(),
        available: vec![
            "future-model".to_owned(),
            "glm5.3-flash".to_owned(),
            "qwen3.6".to_owned(),
        ],
    });
    let models = [
        coding_model_profile("future-model").expect("generic profile"),
        coding_model_profile("glm5.3-flash").expect("bundled profile"),
        coding_model_profile("qwen3.6").expect("default profile"),
    ];
    assert_eq!(
        fallback_model(&selected, &error, &models),
        Some(LaunchModel {
            id: "qwen3.6".to_owned(),
            source: LaunchModelSource::Fallback,
            reasoning: None,
        })
    );
    let default_selected = LaunchModel {
        source: LaunchModelSource::Default,
        ..selected.clone()
    };
    assert_eq!(
        fallback_model(&default_selected, &error, &models),
        Some(LaunchModel {
            id: "qwen3.6".to_owned(),
            source: LaunchModelSource::Fallback,
            reasoning: None,
        })
    );
    assert_eq!(
        fallback_model(&selected, &error, &models[..2]),
        Some(LaunchModel {
            id: "glm5.3-flash".to_owned(),
            source: LaunchModelSource::Fallback,
            reasoning: None,
        }),
        "the first live bundled model should win when the default is absent"
    );

    let explicit = LaunchModel {
        source: LaunchModelSource::Explicit,
        ..selected.clone()
    };
    assert_eq!(fallback_model(&explicit, &error, &models), None);
    assert_eq!(fallback_model(&selected, &error, &models[..1]), None);
}
