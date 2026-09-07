//! The configuration settings nan-harness owns inside a managed profile.
//!
//! Ownership is derived from the managed document itself: every top-level
//! scalar it emits, and every key of a top-level table it emits. Unrelated
//! provider tables and feature flags are therefore never applied or restored.

use super::super::{ChatGptDesktopError, MANAGED_PROVIDER_KEY, SESSION_TOKEN_ENVIRONMENT};
use serde::{Deserialize, Serialize};
use std::path::Path;
use toml_edit::{DocumentMut, Item, Table, TableLike, value};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::commands::chatgpt_desktop) struct OwnedSetting {
    table: Option<String>,
    key: String,
}

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

pub(super) fn owned_settings(managed: &DocumentMut) -> Vec<OwnedSetting> {
    managed
        .iter()
        .flat_map(|(name, item)| match item.as_table_like() {
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

/// A stored receipt may only claim the settings a managed document emits, and
/// may claim each of them once, so recovery can never touch anything else.
pub(super) fn owned_settings_are_valid(settings: &[OwnedSetting]) -> bool {
    let managed = owned_settings(&managed_document("", "", Path::new(""), false));
    !settings.is_empty()
        && settings.iter().enumerate().all(|(index, setting)| {
            managed.contains(setting) && !settings[..index].contains(setting)
        })
}

pub(super) fn overlay_managed_settings(
    document: &mut DocumentMut,
    managed: &DocumentMut,
) -> Result<(), ChatGptDesktopError> {
    let settings = owned_settings(managed);
    reject_incompatible_settings(document, &settings)?;
    for setting in &settings {
        let Some(item) = managed_item(managed, setting) else {
            continue;
        };
        match &setting.table {
            None => {
                document.insert(&setting.key, item.clone());
            }
            Some(name) => {
                if let Some(target) = ensure_table_like(document, name, managed.get(name)) {
                    target.insert(&setting.key, item.clone());
                }
            }
        }
    }
    Ok(())
}

pub(super) fn restore_managed_settings(
    document: &mut DocumentMut,
    original: Option<&DocumentMut>,
    settings: &[OwnedSetting],
) -> Result<(), ChatGptDesktopError> {
    reject_incompatible_settings(document, settings)?;
    if let Some(original) = original {
        reject_incompatible_settings(original, settings)?;
    }
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
            Some(name) => restore_nested(
                document,
                original.and_then(|source| table_like(source, name)),
                name,
                &setting.key,
            ),
        }
    }
    Ok(())
}

/// A managed table name occupied by a scalar cannot hold the routing keys, and
/// overwriting it would discard whatever the app or the user put there.
fn reject_incompatible_settings(
    document: &DocumentMut,
    settings: &[OwnedSetting],
) -> Result<(), ChatGptDesktopError> {
    let incompatible = settings.iter().any(|setting| {
        setting.table.as_deref().is_some_and(|name| {
            document
                .get(name)
                .is_some_and(|item| !item.is_none() && item.as_table_like().is_none())
        })
    });
    if incompatible {
        return Err(ChatGptDesktopError::IncompatibleConfigSetting);
    }
    Ok(())
}

fn managed_item<'a>(managed: &'a DocumentMut, setting: &OwnedSetting) -> Option<&'a Item> {
    match &setting.table {
        None => managed.get(&setting.key),
        Some(name) => table_like(managed, name).and_then(|table| table.get(&setting.key)),
    }
}

fn table_like<'a>(document: &'a DocumentMut, name: &str) -> Option<&'a dyn TableLike> {
    document.get(name).and_then(Item::as_table_like)
}

/// The caller has already refused an incompatible occupant, so this only ever
/// creates a table where the document has none.
fn ensure_table_like<'a>(
    document: &'a mut DocumentMut,
    name: &str,
    template: Option<&Item>,
) -> Option<&'a mut dyn TableLike> {
    if document.get(name).and_then(Item::as_table_like).is_none() {
        let mut fresh = Table::new();
        fresh.set_implicit(
            template
                .and_then(Item::as_table)
                .is_some_and(Table::is_implicit),
        );
        document.insert(name, Item::Table(fresh));
    }
    document.get_mut(name).and_then(Item::as_table_like_mut)
}

fn restore_nested(
    document: &mut DocumentMut,
    original: Option<&dyn TableLike>,
    name: &str,
    key: &str,
) {
    let original_item = original.and_then(|source| source.get(key));
    if let Some(target) = document.get_mut(name).and_then(Item::as_table_like_mut) {
        match original_item {
            Some(item) => {
                target.insert(key, item.clone());
            }
            None => {
                target.remove(key);
            }
        }
        if target.is_empty() && original.is_none() {
            document.remove(name);
        }
        return;
    }
    if let Some(item) = original_item {
        let mut rebuilt = Table::new();
        rebuilt.insert(key, item.clone());
        document.insert(name, Item::Table(rebuilt));
    }
}

/// A configuration still carrying nan-harness routing is a remnant of an
/// interrupted session, never a native configuration to adopt.
pub(super) fn contains_managed_routing(document: &DocumentMut, catalog_path: &Path) -> bool {
    let providers = table_like(document, "model_providers");
    let provider_table = providers.is_some_and(|providers| {
        providers.contains_key(MANAGED_PROVIDER_KEY)
            || providers.iter().any(|(_, provider)| {
                provider
                    .as_table_like()
                    .and_then(|provider| provider.get("env_key"))
                    .and_then(Item::as_str)
                    .is_some_and(|key| key == SESSION_TOKEN_ENVIRONMENT)
            })
    });
    provider_table
        || document
            .get("model_provider")
            .and_then(Item::as_str)
            .is_some_and(|provider| provider == MANAGED_PROVIDER_KEY)
        || document
            .get("model_catalog_json")
            .and_then(Item::as_str)
            .is_some_and(|catalog| Path::new(catalog) == catalog_path)
}
