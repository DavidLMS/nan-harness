use super::*;

#[test]
fn launch_announcement_describes_each_model_source() {
    let cases = [
        (
            LaunchModel {
                id: "glm5.2".to_owned(),
                source: LaunchModelSource::Explicit,
                reasoning: Some(ReasoningSelection::Toggle(false)),
            },
            "Starting codex with model 'glm5.2'. Reasoning: disabled.",
        ),
        (
            LaunchModel {
                id: "future-model".to_owned(),
                source: LaunchModelSource::ExplicitUndiscovered,
                reasoning: None,
            },
            "Starting codex with model 'future-model'. Reasoning: not specified.",
        ),
        (
            LaunchModel {
                id: "glm5.2".to_owned(),
                source: LaunchModelSource::Remembered,
                reasoning: Some(ReasoningSelection::Effort(ReasoningEffort::High)),
            },
            "Starting codex with model 'glm5.2' (remembered from your last session; override with --model). Reasoning: high.",
        ),
        (
            LaunchModel {
                id: "qwen3.6".to_owned(),
                source: LaunchModelSource::Default,
                reasoning: None,
            },
            "Starting codex with model 'qwen3.6' (default; override with --model). Reasoning: not specified.",
        ),
        (
            LaunchModel {
                id: "glm5.2-flash".to_owned(),
                source: LaunchModelSource::Fallback,
                reasoning: None,
            },
            "Starting codex with model 'glm5.2-flash' (provider-selected fallback). Reasoning: not specified.",
        ),
    ];

    for (model, expected) in cases {
        assert_eq!(
            format_launch_announcement(HarnessKind::Codex, &model),
            expected
        );
    }
}

#[test]
fn reasoning_state_has_stable_text_for_every_selection() {
    let cases = [
        (None, "not specified"),
        (Some(ReasoningSelection::Auto), "auto"),
        (Some(ReasoningSelection::Toggle(true)), "enabled"),
        (Some(ReasoningSelection::Toggle(false)), "disabled"),
        (
            Some(ReasoningSelection::Effort(ReasoningEffort::Low)),
            "low",
        ),
        (
            Some(ReasoningSelection::Effort(ReasoningEffort::Medium)),
            "medium",
        ),
        (
            Some(ReasoningSelection::Effort(ReasoningEffort::High)),
            "high",
        ),
    ];

    for (selection, expected) in cases {
        assert_eq!(format_reasoning_state(selection), expected);
    }
}

#[test]
fn non_zero_exit_bookend_explains_failures_only() {
    assert_eq!(
        format_exit_bookend(HarnessKind::Codex, ExecutionOutcome::Failed, 7),
        Some((
            "codex exited with code 7.".to_owned(),
            "If this looks like a setup problem, run `nan doctor codex`.".to_owned(),
        ))
    );
    assert_eq!(
        format_exit_bookend(HarnessKind::Codex, ExecutionOutcome::Succeeded, 0),
        None
    );
    assert_eq!(
        format_exit_bookend(
            HarnessKind::Codex,
            ExecutionOutcome::Cancelled(SignalKind::Interrupt),
            130
        ),
        None
    );
    assert_eq!(
        format_exit_bookend(
            HarnessKind::Codex,
            ExecutionOutcome::Cancelled(SignalKind::Terminate),
            143
        ),
        None
    );
}
