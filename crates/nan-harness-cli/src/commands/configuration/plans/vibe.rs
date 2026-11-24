use super::super::{ConfigurationError, ConfigurationPaths, SEARCH_MCP_ID, dotenv_quote};
use super::dispatch::PlanRequest;
use super::types::{DocumentPlan, TextBlockPlan, VibeEntryPlan, VibePlan};
use std::path::Path;
use toml_edit::{Array, DocumentMut, Table, value};

/// Variable Vibe reads the NaN key from. It matches the name managed launches
/// inject, and stays distinct from the generic `NAN_API_KEY` that other tools
/// may export, because a non-empty process value would shadow `.env`.
const CREDENTIAL_VARIABLE: &str = "NAN_VIBE_API_KEY";

pub(crate) fn vibe_plans(
    paths: &ConfigurationPaths,
    request: PlanRequest<'_>,
) -> Result<Vec<DocumentPlan>, ConfigurationError> {
    let directory = &paths.vibe_directory;
    if !directory.is_absolute() {
        return Err(ConfigurationError::ReadDocument {
            path: directory.clone(),
            source: std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Vibe home must be absolute",
            ),
        });
    }
    let config = directory.join("config.toml");
    let mut entries = array_entries(
        "providers",
        &nan_harness_adapters::mistral_vibe_provider_block(request.base_url, CREDENTIAL_VARIABLE),
        &config,
    )?;
    entries.extend(array_entries(
        "models",
        &nan_harness_runtime::mistral_vibe_native_models(request.models)?,
        &config,
    )?);
    if request.search.managed {
        let mut server = Table::new();
        server.insert("name", value(SEARCH_MCP_ID));
        server.insert("transport", value("stdio"));
        server.insert("command", value("nan-harness"));
        server.insert("args", value(Array::from_iter(["__search-mcp"])));
        entries.push(VibeEntryPlan {
            array: "mcp_servers",
            name: SEARCH_MCP_ID.to_owned(),
            table: server,
        });
    }
    Ok(vec![
        DocumentPlan::Vibe(VibePlan {
            path: config,
            entries,
            active_model: request.default_model.to_owned(),
        }),
        DocumentPlan::TextBlock(TextBlockPlan {
            path: directory.join(".env"),
            begin: "# nan-harness:begin provider-credential".to_owned(),
            end: "# nan-harness:end provider-credential".to_owned(),
            body: Some(format!(
                "{CREDENTIAL_VARIABLE}={}",
                dotenv_quote(request.api_key)
            )),
            conflicting_keys: vec![format!("{CREDENTIAL_VARIABLE}=")],
        }),
    ])
}

/// Splits rendered `[[array]]` text from the shared managed-launch projection
/// into standalone tables keyed by their `name`.
fn array_entries(
    array: &'static str,
    rendered: &str,
    path: &Path,
) -> Result<Vec<VibeEntryPlan>, ConfigurationError> {
    let document =
        rendered
            .parse::<DocumentMut>()
            .map_err(|source| ConfigurationError::ParseToml {
                path: path.to_path_buf(),
                source,
            })?;
    Ok(document
        .get(array)
        .and_then(|item| item.as_array_of_tables())
        .into_iter()
        .flatten()
        .filter_map(|table| {
            let name = table.get("name")?.as_str()?.to_owned();
            let mut table = table.clone();
            table.set_position(None);
            table.decor_mut().clear();
            Some(VibeEntryPlan { array, name, table })
        })
        .collect())
}
