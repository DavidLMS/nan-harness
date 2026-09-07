//! The configuration settings nan-harness owns inside a managed profile.
//!
//! `ChatGPT` Desktop reads a native `config.toml` from its profile. A managed
//! session must route the app through the local bridge without discarding the
//! settings the app or the user wrote there, so nan-harness overlays only the
//! keys it emits itself and records them for the matching restoration.
//!
//! Ownership is derived from the managed document rather than from a second
//! hand-written list: every top-level scalar it emits is one owned setting, and
//! every key of a top-level table it emits is one owned setting of that table.
//! The managed provider table is therefore owned as a whole, while unrelated
//! provider tables and unrelated feature flags are left untouched.

use super::super::{MANAGED_PROVIDER_KEY, SESSION_TOKEN_ENVIRONMENT};
use serde::{Deserialize, Serialize};
use std::path::Path;
use toml_edit::{DocumentMut, Item, Table, value};

/// The maximum number of owned settings a receipt may describe. The managed
/// document emits far fewer; the bound keeps a tampered receipt cheap to reject.
const MAX_OWNED_SETTINGS: usize = 64;

/// One setting nan-harness applies to the managed configuration.
///
/// `table` is `None` for a top-level key and `Some(name)` for a key inside the
/// top-level table `name`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::commands::chatgpt_desktop) struct OwnedSetting {
    table: Option<String>,
    key: String,
}

impl OwnedSetting {
    fn is_valid(&self) -> bool {
        !self.key.is_empty() && self.table.as_ref().is_none_or(|table| !table.is_empty())
    }
}

/// Renders the configuration nan-harness applies for one managed session.
pub(in crate::commands::chatgpt_desktop) fn managed_document(
    selected_model: &str,
    bridge_base_url: &str,
    catalog_path: &Path,
    web_search_enabled: bool,
) -> DocumentMut {
    let mut document = DocumentMut::new();
    document["model"] = value(selected_model);
    document["model_provider"] = value(MANAGED_PROVIDER_KEY);
    document["model_catalog_json"] = value(catalog_path.to_string_lossy().as_ref());
    document["suppress_unstable_features_warning"] = value(true);

    let mut features = Table::new();
    features["apps"] = value(false);
    features["standalone_web_search"] = value(web_search_enabled);
    features["responses_websockets"] = value(false);
    features["responses_websockets_v2"] = value(false);
    document["features"] = Item::Table(features);

    let mut providers = Table::new();
    providers.set_implicit(true);
    providers[MANAGED_PROVIDER_KEY] =
        Item::Table(managed_provider(bridge_base_url, web_search_enabled));
    document["model_providers"] = Item::Table(providers);
    document
}

fn managed_provider(bridge_base_url: &str, web_search_enabled: bool) -> Table {
    let mut provider = Table::new();
    provider["name"] = value("nan-harness");
    provider["base_url"] = value(format!("{}/v1", bridge_base_url.trim_end_matches('/')));
    provider["env_key"] = value(SESSION_TOKEN_ENVIRONMENT);
    provider["wire_api"] = value("responses");
    provider["request_max_retries"] = value(0_i64);
    provider["stream_max_retries"] = value(0_i64);
    provider["supports_websockets"] = value(false);
    provider["supports_standalone_web_search"] = value(web_search_enabled);
    provider["requires_openai_auth"] = value(false);
    provider
}

/// Lists the settings a managed document owns, in document order.
pub(super) fn owned_settings(managed: &DocumentMut) -> Vec<OwnedSetting> {
    managed
        .iter()
        .flat_map(|(name, item)| match item.as_table() {
            Some(table) => table
                .iter()
                .map(|(key, _)| OwnedSetting {
                    table: Some(name.to_owned()),
                    key: key.to_owned(),
                })
                .collect(),
            None => vec![OwnedSetting {
                table: None,
                key: name.to_owned(),
            }],
        })
        .collect()
}

/// Accepts only a well-formed, bounded ownership list from a stored receipt.
pub(super) fn owned_settings_are_valid(settings: &[OwnedSetting]) -> bool {
    !settings.is_empty()
        && settings.len() <= MAX_OWNED_SETTINGS
        && settings.iter().all(OwnedSetting::is_valid)
}

/// Applies every owned setting of `managed` onto `document`, leaving all other
/// settings of `document` exactly as the app or the user wrote them.
pub(super) fn overlay_managed_settings(document: &mut DocumentMut, managed: &DocumentMut) {
    for setting in owned_settings(managed) {
        match &setting.table {
            None => {
                if let Some(item) = managed.get(&setting.key) {
                    document.insert(&setting.key, item.clone());
                }
            }
            Some(name) => overlay_nested(document, managed, name, &setting.key),
        }
    }
}

fn overlay_nested(document: &mut DocumentMut, managed: &DocumentMut, table: &str, key: &str) {
    let Some(source) = managed.get(table).and_then(Item::as_table) else {
        return;
    };
    let Some(item) = source.get(key) else {
        return;
    };
    // A non-table value under a managed table name cannot hold the routing keys
    // the session needs; the original bytes stay recoverable from the backup.
    if document.get(table).and_then(Item::as_table).is_none() {
        let mut fresh = Table::new();
        fresh.set_implicit(source.is_implicit());
        document.insert(table, Item::Table(fresh));
    }
    if let Some(target) = document.get_mut(table).and_then(Item::as_table_mut) {
        target.insert(key, item.clone());
    }
}

/// Returns every owned setting of `document` to the state `original` recorded,
/// removing keys that were absent and restoring the values that were present.
/// Settings outside the ownership list are left as the app wrote them.
pub(super) fn restore_managed_settings(
    document: &mut DocumentMut,
    original: Option<&DocumentMut>,
    settings: &[OwnedSetting],
) {
    for setting in settings {
        match &setting.table {
            None => match original.and_then(|source| source.get(&setting.key)) {
                Some(item) => {
                    document.insert(&setting.key, item.clone());
                }
                None => {
                    document.remove(&setting.key);
                }
            },
            Some(table) => restore_nested(
                document,
                original
                    .and_then(|source| source.get(table))
                    .and_then(Item::as_table),
                table,
                &setting.key,
            ),
        }
    }
}

fn restore_nested(document: &mut DocumentMut, original: Option<&Table>, table: &str, key: &str) {
    let original_item = original.and_then(|source| source.get(key));
    if let Some(target) = document.get_mut(table).and_then(Item::as_table_mut) {
        match original_item {
            Some(item) => {
                target.insert(key, item.clone());
            }
            None => {
                target.remove(key);
            }
        }
        // A table nan-harness created for its own settings disappears with them.
        if target.is_empty() && original.is_none() {
            document.remove(table);
        }
        return;
    }
    // The app replaced or removed a table that held an original owned setting.
    if let (Some(item), Some(source)) = (original_item, original) {
        let mut rebuilt = Table::new();
        rebuilt.set_implicit(source.is_implicit());
        rebuilt.insert(key, item.clone());
        document.insert(table, Item::Table(rebuilt));
    }
}

/// Reports whether a configuration still carries nan-harness session routing.
///
/// A managed launch refuses to adopt such a document without a valid receipt:
/// it is a remnant of an interrupted session, not a native configuration.
/// A provider table authenticated with the launch-scoped session token can
/// only have been written by a managed session, whatever it is named.
fn uses_session_token((_, provider): (&str, &Item)) -> bool {
    provider
        .as_table()
        .and_then(|provider| provider.get("env_key"))
        .and_then(Item::as_str)
        .is_some_and(|key| key == SESSION_TOKEN_ENVIRONMENT)
}

pub(super) fn contains_managed_routing(document: &DocumentMut, catalog_path: &Path) -> bool {
    let provider_declared = document
        .get("model_provider")
        .and_then(Item::as_str)
        .is_some_and(|provider| provider == MANAGED_PROVIDER_KEY);
    let providers = document.get("model_providers").and_then(Item::as_table);
    let provider_table = providers.is_some_and(|providers| {
        providers.contains_key(MANAGED_PROVIDER_KEY) || providers.iter().any(uses_session_token)
    });
    let catalog_declared = document
        .get("model_catalog_json")
        .and_then(Item::as_str)
        .is_some_and(|catalog| Path::new(catalog) == catalog_path);
    provider_declared || provider_table || catalog_declared
}
