#[allow(clippy::wildcard_imports)]
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LaunchModelSource {
    Explicit,
    ExplicitUndiscovered,
    Remembered,
    Default,
    Fallback,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LaunchModel {
    pub(super) id: String,
    pub(super) source: LaunchModelSource,
    pub(super) reasoning: Option<ReasoningSelection>,
}

pub(super) fn model_for_launch(kind: HarnessKind, arguments: &HarnessRunArgs) -> LaunchModel {
    let remembered = PersistenceManager::from_environment()
        .ok()
        .and_then(|manager| manager.last_selection(kind).ok())
        .flatten();
    choose_launch_model(arguments.model.as_deref(), remembered)
}

pub(super) fn choose_launch_model(
    explicit: Option<&str>,
    remembered: Option<LastSelection>,
) -> LaunchModel {
    if let Some(model) = explicit {
        return LaunchModel {
            id: model.to_owned(),
            source: LaunchModelSource::Explicit,
            reasoning: None,
        };
    }
    if let Some(selection) = remembered {
        return LaunchModel {
            id: selection.model,
            source: LaunchModelSource::Remembered,
            reasoning: selection.reasoning,
        };
    }
    LaunchModel {
        id: DEFAULT_MODEL_ID.to_owned(),
        source: LaunchModelSource::Default,
        reasoning: None,
    }
}

pub(super) fn successful_selection(
    kind: HarnessKind,
    launched: &LaunchModel,
    report: &nan_harness_runtime::ExecutionReport,
) -> Option<LastSelection> {
    if report.outcome != ExecutionOutcome::Succeeded {
        return None;
    }
    if kind == HarnessKind::Codex
        && let Some(model) = report.selected_model.as_deref()
        && (matches!(
            launched.source,
            LaunchModelSource::Explicit
                | LaunchModelSource::ExplicitUndiscovered
                | LaunchModelSource::Remembered
                | LaunchModelSource::Fallback
        ) || model != launched.id)
    {
        return Some(LastSelection {
            model: model.to_owned(),
            reasoning: report.selected_reasoning,
        });
    }
    matches!(
        launched.source,
        LaunchModelSource::Explicit
            | LaunchModelSource::ExplicitUndiscovered
            | LaunchModelSource::Fallback
    )
    .then(|| LastSelection {
        model: launched.id.clone(),
        reasoning: launched.reasoning,
    })
}

pub(super) fn fallback_model(
    selected: &LaunchModel,
    error: &RuntimeError,
    models: &[CodingModelProfile],
) -> Option<LaunchModel> {
    if !matches!(
        selected.source,
        LaunchModelSource::Remembered | LaunchModelSource::Default
    ) {
        return None;
    }
    let (unavailable, available) = error.unavailable_model()?;
    if unavailable != selected.id {
        return None;
    }
    let id = models
        .iter()
        .filter(|model| model.id != selected.id && available.contains(&model.id))
        .find(|model| model.id == DEFAULT_MODEL_ID && known_coding_model(&model.id).is_some())
        .or_else(|| {
            models.iter().find(|model| {
                model.id != selected.id
                    && available.contains(&model.id)
                    && known_coding_model(&model.id).is_some()
            })
        })?
        .id
        .clone();
    Some(LaunchModel {
        id,
        source: LaunchModelSource::Fallback,
        reasoning: None,
    })
}

pub(super) fn should_attempt_fallback(selected: &LaunchModel, error: &RuntimeError) -> bool {
    matches!(
        selected.source,
        LaunchModelSource::Remembered | LaunchModelSource::Default
    ) && error
        .unavailable_model()
        .is_some_and(|(unavailable, _)| unavailable == selected.id)
}

pub(super) fn format_launch_announcement(kind: HarnessKind, model: &LaunchModel) -> String {
    use nan_harness_i18n::{locale, messages};
    match model.source {
        LaunchModelSource::Explicit | LaunchModelSource::ExplicitUndiscovered => {
            messages::launch_explicit(locale(), &kind, &model.id)
        }
        LaunchModelSource::Remembered => messages::launch_remembered(locale(), &kind, &model.id),
        LaunchModelSource::Default => messages::launch_default(locale(), &kind, &model.id),
        LaunchModelSource::Fallback => messages::launch_fallback(locale(), &kind, &model.id),
    }
}

pub(super) fn format_exit_bookend(
    kind: HarnessKind,
    outcome: ExecutionOutcome,
    exit_code: i32,
) -> Option<(String, String)> {
    if exit_code == 0 || matches!(outcome, ExecutionOutcome::Cancelled(_)) {
        return None;
    }
    Some((
        nan_harness_i18n::messages::launch_exit(nan_harness_i18n::locale(), &exit_code, &kind),
        nan_harness_i18n::messages::launch_setup_hint(nan_harness_i18n::locale(), &kind),
    ))
}
