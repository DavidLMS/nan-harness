mod cordis;
#[cfg(test)]
mod tests;

use super::super::{
    DEEPSEEK_BLOCK_BEGIN, DEEPSEEK_BLOCK_END, IntegrationChange, ManagedCordisDocument,
    ManagedCordisEntry, ManagedDeepSeek, PersistenceError, PersistenceManager, PreparedFileChange,
    RemovalOutcome, deepseek_provider_settings, optional_utf8, permissions,
    prepare_managed_block_removal, read_optional, sha256,
};
use crate::commands::persistence::ConfigurationHealth;
use nan_harness_core::CodingModelProfile;
use serde_yaml_ng::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

type Sources = BTreeMap<PathBuf, Option<Vec<u8>>>;

impl PersistenceManager {
    #[cfg(test)]
    pub(crate) fn configure_deepseek_harness(
        &self,
        models: &[CodingModelProfile],
        base_url: &str,
    ) -> Result<IntegrationChange, PersistenceError> {
        let (files, change) = self.prepare_deepseek_harness(models, base_url)?;
        self.publish_configuration_files(&files)?;
        Ok(change)
    }

    #[cfg(test)]
    pub(crate) fn prepare_deepseek_harness(
        &self,
        models: &[CodingModelProfile],
        base_url: &str,
    ) -> Result<(Vec<PreparedFileChange>, IntegrationChange), PersistenceError> {
        self.prepare_deepseek_with_sources(models, base_url, &Sources::new())
    }

    pub(crate) fn prepare_deepseek_with_sources(
        &self,
        models: &[CodingModelProfile],
        base_url: &str,
        sources: &Sources,
    ) -> Result<(Vec<PreparedFileChange>, IntegrationChange), PersistenceError> {
        let (mut state, receipt_file) = self.prepare_state()?;
        let (mut changes, imported) =
            Self::prepare_deepseek_legacy(state.deepseek_harness.as_ref())?;
        let home_patch = self.deepseek_directory.join("cordis.patch.yml");
        let global = read_source(&home_patch, sources)?;
        let global_doc = cordis::parse(&global, &home_patch)?;
        let global_provider = cordis::entry(&global_doc, "llm-pi-ai", &home_patch)?.is_some();
        let mut paths = self.deepseek_profile_paths()?;
        if global_provider
            || cordis::entry(&global_doc, "agent-default-model", &home_patch)?.is_some()
        {
            paths.insert(home_patch.clone());
        }
        if let Some(previous) = &state.deepseek_cordis {
            paths.extend(previous.documents.iter().map(|doc| doc.path.clone()));
        }
        let desired = cordis::parse(&deepseek_provider_settings(models, base_url)?, &home_patch)?;
        let mut documents = Vec::new();
        for path in paths {
            let source = read_source(&path, sources)?;
            let mut document = cordis::parse(&source, &path)?;
            let old = state
                .deepseek_cordis
                .as_ref()
                .and_then(|managed| managed.documents.iter().find(|doc| doc.path == path));
            let original = read_optional(&path)?;
            let mut owned =
                cordis::receipt(&path, original.as_deref().map(|_| source.as_str()), old);
            if old.is_some() {
                cordis::verify(&document, &owned)?;
                if sha256(source.as_bytes()) != owned.rendered_sha256 {
                    let mut baseline = document.clone();
                    cordis::restore(&mut baseline, &owned)?;
                    owned.original =
                        Some(cordis::render_preserving_search(&baseline, &source, &path)?);
                }
            }
            if let Some(imported) = &imported {
                adopt_imported(&document, &mut owned, imported, &source)?;
            }
            // A home override already masks the profile config. Otherwise each profile keeps its own providers.
            let configure_provider = if path == home_patch {
                global_provider
            } else {
                !global_provider
            };
            apply_desired(&mut document, &mut owned, &desired, configure_provider)?;
            let rendered = cordis::render_preserving_search(&document, &source, &path)?;
            owned.rendered_sha256 = sha256(rendered.as_bytes());
            changes.push(file_change(&path, original, Some(rendered.into_bytes()))?);
            documents.push(owned);
        }
        state.deepseek_harness = None;
        let modified = changes
            .iter()
            .any(|change| change.original != change.replacement);
        let paths = documents
            .iter()
            .map(|doc| doc.path.clone())
            .collect::<Vec<_>>();
        state.deepseek_cordis = Some(ManagedDeepSeek { documents });
        let path = paths.first().cloned().unwrap_or(home_patch);
        let additional_paths = paths
            .into_iter()
            .filter(|candidate| candidate != &path)
            .collect();
        let files = Self::prepare_integration_files(changes, &state, receipt_file)?;
        Ok((
            files,
            IntegrationChange {
                path,
                additional_paths,
                backup: None,
                changed: modified,
            },
        ))
    }

    pub(crate) fn deepseek_profile_paths(&self) -> Result<BTreeSet<PathBuf>, PersistenceError> {
        // @deepseek-ai/dsh-app-boot 0.2.0-rc.2 PROFILE_TEMPLATES, verified against the published package.
        let mut names = ["acp", "web", "headless", "sdk"]
            .into_iter()
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();
        let profiles = self.deepseek_directory.join("profiles");
        match std::fs::read_dir(&profiles) {
            Ok(entries) => {
                for entry in entries {
                    let entry = entry.map_err(|source| PersistenceError::ReadFile {
                        path: profiles.clone(),
                        source,
                    })?;
                    if entry
                        .file_type()
                        .map_err(|source| PersistenceError::ReadFile {
                            path: entry.path(),
                            source,
                        })?
                        .is_dir()
                    {
                        let name = entry.file_name().to_string_lossy().into_owned();
                        if name == "sdk-minimal" {
                            continue;
                        }
                        if !entry.path().join("package.json").exists() && names.contains(&name) {
                            continue;
                        }
                        if compatible_profile(&entry.path())? {
                            names.insert(name);
                        } else {
                            names.remove(&name);
                        }
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(PersistenceError::ReadFile {
                    path: profiles,
                    source,
                });
            }
        }
        Ok(names
            .into_iter()
            .map(|name| profiles.join(name).join("cordis.patch.yml"))
            .collect())
    }

    fn prepare_deepseek_legacy(
        managed: Option<&super::super::ManagedBlock>,
    ) -> Result<(Vec<PreparedFileChange>, Option<Value>), PersistenceError> {
        let Some(managed) = managed else {
            return Ok((Vec::new(), None));
        };
        let mut managed = managed.clone();
        if read_optional(&managed.path)?.is_none() {
            managed.path = managed.path.with_extension("yaml.imported");
        }
        let source = read_source(&managed.path, &Sources::new())?;
        let change =
            prepare_managed_block_removal(&managed, DEEPSEEK_BLOCK_BEGIN, DEEPSEEK_BLOCK_END)?;
        let start = source
            .find(DEEPSEEK_BLOCK_BEGIN)
            .ok_or(PersistenceError::InvalidManagedBlock)?
            + DEEPSEEK_BLOCK_BEGIN.len();
        let end = source[start..]
            .find(DEEPSEEK_BLOCK_END)
            .ok_or(PersistenceError::InvalidManagedBlock)?
            + start;
        let imported = serde_yaml_ng::from_str(&source[start..end])
            .map_err(|_| PersistenceError::InvalidManagedSection(managed.path.clone()))?;
        Ok((vec![change], Some(imported)))
    }

    pub(crate) fn unpersist_deepseek_harness(&self) -> Result<RemovalOutcome, PersistenceError> {
        let (files, outcome) = self.prepare_remove_deepseek_harness()?;
        self.publish_configuration_files(&files)?;
        Ok(outcome)
    }

    pub(crate) fn prepare_remove_deepseek_harness(
        &self,
    ) -> Result<(Vec<PreparedFileChange>, RemovalOutcome), PersistenceError> {
        self.prepare_remove_deepseek_with_sources(&Sources::new())
    }

    pub(crate) fn prepare_remove_deepseek_with_sources(
        &self,
        sources: &Sources,
    ) -> Result<(Vec<PreparedFileChange>, RemovalOutcome), PersistenceError> {
        let (mut state, receipt_file) = self.prepare_state()?;
        if state.deepseek_harness.is_none() && state.deepseek_cordis.is_none() {
            return Ok((Vec::new(), RemovalOutcome::NotConfigured));
        }
        let (mut changes, imported) =
            Self::prepare_deepseek_legacy(state.deepseek_harness.as_ref())?;
        let documents = match &state.deepseek_cordis {
            Some(managed) => managed.documents.clone(),
            None => self
                .deepseek_profile_paths()?
                .into_iter()
                .map(|path| {
                    let source = read_source(&path, sources)?;
                    let document = cordis::parse(&source, &path)?;
                    let mut owned = cordis::receipt(&path, Some(&source), None);
                    if let Some(imported) = &imported {
                        adopt_imported(&document, &mut owned, imported, &source)?;
                    }
                    Ok(owned)
                })
                .collect::<Result<Vec<_>, PersistenceError>>()?,
        };
        for owned in documents {
            if owned.entries.is_empty() {
                continue;
            }
            let original = read_optional(&owned.path)?;
            let source = read_source(&owned.path, sources)?;
            let mut document = cordis::parse(&source, &owned.path)?;
            cordis::restore(&mut document, &owned)?;
            let replacement = if sha256(source.as_bytes()) == owned.rendered_sha256 {
                owned.original.map(String::into_bytes)
            } else if owned.original.is_none() && document.as_sequence().is_some_and(Vec::is_empty)
            {
                None
            } else {
                Some(
                    cordis::render_preserving_search(&document, &source, &owned.path)?.into_bytes(),
                )
            };
            changes.push(file_change(&owned.path, original, replacement)?);
        }
        state.deepseek_harness = None;
        state.deepseek_cordis = None;
        Ok((
            Self::prepare_integration_files(changes, &state, receipt_file)?,
            RemovalOutcome::Removed,
        ))
    }

    #[cfg(test)]
    pub(crate) fn deepseek_harness_is_active(&self) -> bool {
        self.inspect_deepseek_harness()
            .is_ok_and(|health| health.is_some_and(ConfigurationHealth::is_active))
    }

    pub(crate) fn inspect_deepseek_harness(
        &self,
    ) -> Result<Option<ConfigurationHealth>, PersistenceError> {
        let state = self.load_state()?;
        let Some(managed) = state.deepseek_cordis else {
            return Ok(state.deepseek_harness.map(|_| ConfigurationHealth::Changed));
        };
        let mut health = ConfigurationHealth::Active;
        for receipt in managed.documents {
            let current = match read_optional(&receipt.path) {
                Ok(None) => ConfigurationHealth::Missing,
                Err(_) => ConfigurationHealth::Unreadable,
                Ok(Some(contents)) => match std::str::from_utf8(&contents)
                    .ok()
                    .and_then(|source| cordis::parse(source, &receipt.path).ok())
                {
                    Some(doc) => {
                        ConfigurationHealth::from_matches(cordis::verify(&doc, &receipt).is_ok())
                    }
                    None => ConfigurationHealth::Invalid,
                },
            };
            health = health.max(current);
        }
        Ok(Some(health))
    }
}

fn read_source(path: &Path, sources: &Sources) -> Result<String, PersistenceError> {
    let original = match sources.get(path) {
        Some(value) => value.clone(),
        None => read_optional(path)?,
    };
    optional_utf8(path, original.as_deref())
}

fn file_change(
    path: &Path,
    original: Option<Vec<u8>>,
    replacement: Option<Vec<u8>>,
) -> Result<PreparedFileChange, PersistenceError> {
    let permissions = permissions(path)?;
    Ok(PreparedFileChange {
        path: path.to_owned(),
        original,
        replacement,
        original_permissions: permissions.clone(),
        replacement_permissions: permissions,
    })
}

fn apply_desired(
    document: &mut Value,
    owned: &mut ManagedCordisDocument,
    desired: &Value,
    provider: bool,
) -> Result<(), PersistenceError> {
    let desired_providers = cordis::entry(desired, "llm-pi-ai", &owned.path)?
        .and_then(|row| row.get("config"))
        .and_then(|config| config.get("providers"));
    let retired = owned
        .entries
        .iter()
        .filter(|entry| {
            entry.id == "llm-pi-ai"
                && entry.key.len() == 3
                && entry.key[1] == "providers"
                && (!provider
                    || desired_providers
                        .and_then(|providers| providers.get(&entry.key[2]))
                        .is_none())
        })
        .cloned()
        .collect::<Vec<_>>();
    if !retired.is_empty() {
        let mut receipt = owned.clone();
        receipt.entries.clone_from(&retired);
        cordis::restore(document, &receipt)?;
        owned.entries.retain(|entry| !retired.contains(entry));
    }
    let default = cordis::entry(desired, "agent-default-model", &owned.path)?
        .ok_or_else(|| PersistenceError::InvalidManagedSection(owned.path.clone()))?;
    for key in ["provider", "model"] {
        cordis::own(
            document,
            owned,
            "agent-default-model",
            &["config", key],
            default["config"][key].clone(),
            false,
        )?;
    }
    if provider {
        let llm = cordis::entry(desired, "llm-pi-ai", &owned.path)?
            .ok_or_else(|| PersistenceError::InvalidManagedSection(owned.path.clone()))?;
        for (name, value) in llm["config"]["providers"]
            .as_mapping()
            .ok_or_else(|| PersistenceError::InvalidManagedSection(owned.path.clone()))?
        {
            cordis::own(
                document,
                owned,
                "llm-pi-ai",
                &[
                    "config",
                    "providers",
                    name.as_str().ok_or_else(|| {
                        PersistenceError::InvalidManagedSection(owned.path.clone())
                    })?,
                ],
                value.clone(),
                true,
            )?;
        }
    }
    Ok(())
}

fn adopt_imported(
    document: &Value,
    owned: &mut ManagedCordisDocument,
    imported: &Value,
    source: &str,
) -> Result<(), PersistenceError> {
    let imported_route = cordis::entry(document, "llm-pi-ai", &owned.path)?
        .and_then(|row| row.get("config"))
        .and_then(|config| config.get("providers"))
        .and_then(|providers| providers.get("nan-harness"));
    if imported_route.is_none() || !owned.entries.is_empty() {
        return Ok(());
    }

    for (id, keys) in [
        ("llm-pi-ai", vec![vec!["providers", "nan-harness"]]),
        ("agent-default-model", vec![vec!["provider"], vec!["model"]]),
    ] {
        let Some(row) = cordis::entry(document, id, &owned.path)? else {
            continue;
        };
        for relative in keys {
            let mut key = vec!["config".to_owned()];
            key.extend(relative.iter().map(|part| (*part).to_owned()));
            if owned
                .entries
                .iter()
                .any(|entry| entry.id == id && entry.key == key)
            {
                continue;
            }
            let Some(current) = cordis::field(row, &key) else {
                continue;
            };
            let expected = relative.iter().fold(imported.get(id), |value, part| {
                value.and_then(|value| value.get(part))
            });
            if Some(current) != expected {
                return Err(PersistenceError::ManagedSectionChanged(owned.path.clone()));
            }
            owned.entries.push(ManagedCordisEntry {
                id: id.to_owned(),
                key,
                value: current.clone(),
                previous: None,
            });
            owned.created_entries.push(id.to_owned());
        }
    }
    // The legacy import contains no rollback snapshot. Preserve every unowned field
    // in the semantic baseline, rather than reviving NaN state or deleting the file.
    let mut baseline = document.clone();
    cordis::restore(&mut baseline, owned)?;
    owned.original = if baseline.as_sequence().is_some_and(Vec::is_empty) {
        None
    } else {
        Some(cordis::render_preserving_search(
            &baseline,
            source,
            &owned.path,
        )?)
    };
    owned.rendered_sha256 = sha256(source.as_bytes());
    Ok(())
}

fn compatible_profile(directory: &Path) -> Result<bool, PersistenceError> {
    let path = directory.join("package.json");
    let Some(contents) = read_optional(&path)? else {
        return Ok(false);
    };
    let manifest: serde_json::Value = serde_json::from_slice(&contents)
        .map_err(|_| PersistenceError::InvalidManagedSection(path))?;
    let Some(bundles) = manifest
        .pointer("/dsh/profile/bundles")
        .and_then(serde_json::Value::as_array)
    else {
        return Ok(false);
    };
    if !bundles
        .iter()
        .any(|bundle| bundle.as_str() == Some("@deepseek-ai/dsh-base"))
    {
        return Ok(false);
    }
    let supported = [
        "@deepseek-ai/dsh-base",
        "@deepseek-ai/dsh-acp-app",
        "@deepseek-ai/dsh-web-app",
        "@deepseek-ai/dsh-headless",
        "@deepseek-ai/dsh-sdk-app",
    ];
    if bundles.iter().any(|bundle| {
        bundle
            .as_str()
            .is_none_or(|name| !supported.contains(&name))
    }) {
        return Err(PersistenceError::UnsupportedDeepSeekProfile(
            directory.to_owned(),
        ));
    }
    Ok(true)
}
