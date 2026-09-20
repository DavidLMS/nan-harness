use super::super::{
    CodingModelProfile, ConfigurationError, ConfigurationPaths, HarnessKind, ManagedSearchStatus,
    MediaSelection, json, yaml_quote,
};
use super::combinators::exclusive_json;
use super::families::{omp_plans, pi_family_plans};
use super::hermes::hermes_plans;
use super::openclaw::openclaw_plans;
use super::search::search_mcp_plan;
use super::specific::{cline_plans, deepseek_plans, goose_plans, qwen_plans};
use super::types::{DocumentPlan, JsonPlan, KimiPlan, TextBlockPlan};

#[derive(Debug, Clone, Copy)]
pub(crate) struct PlanRequest<'a> {
    pub(crate) api_key: &'a str,
    pub(crate) base_url: &'a str,
    pub(crate) models: &'a [CodingModelProfile],
    pub(crate) default_model: &'a str,
    pub(crate) search: ManagedSearchStatus,
    pub(crate) media: MediaSelection,
}

pub(crate) fn for_harness_with_media(
    paths: &ConfigurationPaths,
    harness: HarnessKind,
    request: PlanRequest<'_>,
) -> Result<Vec<DocumentPlan>, ConfigurationError> {
    let PlanRequest {
        api_key,
        base_url,
        models,
        default_model,
        search,
        media,
    } = request;
    let plans = match harness {
        HarnessKind::OpenCode => vec![DocumentPlan::Json(JsonPlan {
            path: paths.opencode_auth_path.clone(),
            entries: vec![exclusive_json(
                &["nan"],
                json!({"type": "api", "key": api_key}),
            )],
        })],
        HarnessKind::Pi => pi_family_plans(
            &paths.home_directory.join(".pi/agent"),
            api_key,
            base_url,
            models,
            default_model,
            search,
        ),
        HarnessKind::Omp => omp_plans(
            &paths.omp_directory,
            api_key,
            base_url,
            models,
            default_model,
            search,
        )?,
        HarnessKind::PrimeAgent => pi_family_plans(
            &paths.prime_directory,
            api_key,
            base_url,
            models,
            default_model,
            search,
        ),
        HarnessKind::QwenCode => {
            qwen_plans(&paths.qwen_directory, api_key, base_url, search.managed)
        }
        HarnessKind::DeepSeekHarness => {
            deepseek_plans(&paths.deepseek_directory, api_key, base_url, search.managed)?
        }
        HarnessKind::Aider => vec![aider_plan(paths, api_key, default_model)?],
        HarnessKind::Hermes => hermes_plans(
            &paths.home_directory.join(".hermes"),
            api_key,
            base_url,
            default_model,
            search.managed,
            media,
        )?,
        HarnessKind::OpenClaw => openclaw_plans(
            &paths.home_directory.join(".openclaw"),
            api_key,
            base_url,
            models,
            default_model,
            search.managed,
            media,
        ),
        HarnessKind::Cline => cline_plans(
            &paths.home_directory.join(".cline/data/settings"),
            api_key,
            base_url,
            models,
            default_model,
            search.managed,
        ),
        HarnessKind::KimiCode => vec![
            DocumentPlan::Kimi(KimiPlan {
                path: paths.kimi_directory.join("config.toml"),
                api_key: api_key.to_owned(),
                base_url: base_url.to_owned(),
                models: models.to_vec(),
                default_model: default_model.to_owned(),
            }),
            search_mcp_plan(paths.kimi_directory.join("mcp.json"), search.managed),
        ],
        HarnessKind::Goose => goose_plans(
            &paths.goose_directory,
            api_key,
            base_url,
            models,
            default_model,
            search.managed,
        )?,
        HarnessKind::ClaudeCode | HarnessKind::Codex | HarnessKind::Fx => {
            return Err(ConfigurationError::BridgeOnly(harness));
        }
    };
    Ok(plans)
}

fn aider_plan(
    paths: &ConfigurationPaths,
    api_key: &str,
    default_model: &str,
) -> Result<DocumentPlan, ConfigurationError> {
    Ok(DocumentPlan::TextBlock(TextBlockPlan {
        path: paths.home_directory.join(".aider.conf.yml"),
        begin: "# nan-harness:begin provider-defaults".to_owned(),
        end: "# nan-harness:end provider-defaults".to_owned(),
        body: Some(format!(
            "api-key:\n  - {}\nmodel: {}",
            yaml_quote(&format!("nan={api_key}"))?,
            yaml_quote(&format!("nan/{default_model}"))?
        )),
        conflicting_keys: vec!["api-key:".to_owned(), "model:".to_owned()],
    }))
}
