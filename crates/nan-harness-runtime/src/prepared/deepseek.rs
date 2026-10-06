use nan_harness_core::{HarnessKind, LaunchPlan};
use nan_harness_i18n::DiagnosticText;
use serde_yaml_ng::{Mapping, Value};
use std::path::Path;

fn error() -> DiagnosticText {
    // Never include the private dump, its parser diagnostics, or the child's stderr.
    DiagnosticText::new(nan_harness_i18n::messages::detail_compose_deepseek_profile_failed)
}

pub(super) fn load_effective_config(plan: &LaunchPlan) -> Result<Option<Value>, DiagnosticText> {
    if plan.harness.kind != HarnessKind::DeepSeekHarness {
        return Ok(None);
    }
    let args = &plan.process.arguments;
    let profile = if args.first().is_some_and(|arg| arg == "--profile") {
        args.get(1).map(String::as_str).ok_or_else(error)?
    } else if let Some(profile) = args.first().and_then(|arg| arg.strip_prefix("--profile=")) {
        profile
    } else {
        args.first().map_or("web", String::as_str)
    };
    let output = crate::discovery::run_bounded_config_command(
        Path::new(&plan.harness.executable),
        &["--profile", profile, "--dump-config"],
        Path::new(&plan.process.working_directory),
    )
    .map_err(|_| error())?;
    if !output.status.success() {
        return Err(error());
    }
    let document: Value = serde_yaml_ng::from_slice(&output.stdout).map_err(|_| error())?;
    if !document.is_sequence() {
        return Err(error());
    }
    Ok(Some(document))
}

pub(super) fn compose_patch(
    source: &str,
    effective: Option<&Value>,
) -> Result<String, DiagnosticText> {
    let effective = effective.and_then(Value::as_sequence).ok_or_else(error)?;
    let mut patch: Value = serde_yaml_ng::from_str(source).map_err(|_| error())?;
    for row in patch.as_sequence_mut().ok_or_else(error)? {
        let Some(id) = row.get("id").and_then(Value::as_str).map(str::to_owned) else {
            continue;
        };
        let matching = effective
            .iter()
            .filter(|candidate| candidate.get("id").and_then(Value::as_str) == Some(&id))
            .collect::<Vec<_>>();
        if matching.len() != 1 {
            return Err(error());
        }
        let Some(desired) = row.get("config").cloned() else {
            continue;
        };
        let mut config = matching[0]
            .get("config")
            .cloned()
            .unwrap_or_else(|| Value::Mapping(Mapping::new()));
        let fields = config.as_mapping_mut().ok_or_else(error)?;
        for (key, value) in desired.as_mapping().ok_or_else(error)? {
            if id == "llm-pi-ai" && key.as_str() == Some("providers") {
                let providers = fields
                    .entry(key.clone())
                    .or_insert_with(|| Value::Mapping(Mapping::new()));
                let providers = providers.as_mapping_mut().ok_or_else(error)?;
                providers.extend(value.as_mapping().ok_or_else(error)?.clone());
            } else {
                fields.insert(key.clone(), value.clone());
            }
        }
        row["config"] = config;
    }
    serde_yaml_ng::to_string(&patch).map_err(|_| error())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_preserves_effective_provider_siblings_and_replaces_owned_routes() {
        let effective = serde_yaml_ng::from_str("- id: llm-pi-ai\n  config:\n    other: keep\n    providers:\n      custom: {apiKeyEnv: CUSTOM_KEY, models: [{id: user-model}]}\n      nan-harness: {models: [{id: old}]}\n- id: agent-default-model\n  config: {provider: custom, model: user-model, userOption: true}\n").unwrap();
        let rendered = compose_patch("- id: llm-pi-ai\n  config:\n    providers:\n      nan-harness: {models: [{id: new}]}\n- id: agent-default-model\n  config: {provider: nan-harness, model: new}\n", Some(&effective)).unwrap();
        let result: Value = serde_yaml_ng::from_str(&rendered).unwrap();
        assert_eq!(result[0]["config"]["other"], "keep");
        assert_eq!(
            result[0]["config"]["providers"]["custom"]["apiKeyEnv"],
            "CUSTOM_KEY"
        );
        assert_eq!(
            result[0]["config"]["providers"]["nan-harness"]["models"][0]["id"],
            "new"
        );
        assert_eq!(result[1]["config"]["userOption"], true);
    }

    #[test]
    fn unknown_or_dynamic_entries_fail_without_exposing_their_contents() {
        let effective =
            serde_yaml_ng::from_str("- id: llm-pi-ai\n  config: !js PRIVATE_CONFIG\n").unwrap();
        let error = compose_patch(
            "- id: llm-pi-ai\n  config: {providers: {}}",
            Some(&effective),
        )
        .unwrap_err();
        assert!(!error.to_string().contains("PRIVATE_CONFIG"));
    }
}
