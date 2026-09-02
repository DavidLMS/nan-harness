use super::{
    BTreeSet, CodingModelProfile, ConfigurationChange, ConfigurationError, ConfigurationPaths,
    ConfigurationState, DEFAULT_MODEL_ID, DocumentPlan, HarnessKind, HarnessReceipt,
    ManagedSearchStatus, PathBuf, PersistenceError, PersistenceManager, RemovalOutcome,
    ResolvedConfig, STATE_SCHEMA_VERSION, SearchConfiguration, SearchPolicyError, WebSearchPolicy,
    apply_prepared, catalog_integration, document_is_active, ensure_supported, env, for_harness,
    inspect_search_configuration, legacy_harness, preferred_model, prepare_documents,
    prepare_removals, receipt_manages_content, rollback_prepared, sha256, write_private_file,
};
use std::fs;
#[cfg(test)]
use std::path::Path;

#[derive(Debug)]
pub(crate) struct ConfigurationManager {
    pub(crate) paths: ConfigurationPaths,
    pub(crate) legacy: PersistenceManager,
}

impl ConfigurationManager {
    pub(crate) fn from_environment() -> Result<Self, ConfigurationError> {
        Ok(Self {
            paths: ConfigurationPaths::from_environment()?,
            legacy: PersistenceManager::from_environment()?,
        })
    }

    #[cfg(test)]
    pub(crate) fn new(state_directory: &Path, home_directory: &Path) -> Self {
        Self {
            paths: ConfigurationPaths::new(state_directory, home_directory),
            legacy: PersistenceManager::new_for_tests(state_directory, home_directory),
        }
    }

    pub(crate) fn configured_harnesses(&self) -> Result<Vec<HarnessKind>, ConfigurationError> {
        let state = self.load_state()?;
        let mut configured = state
            .harnesses
            .keys()
            .filter_map(|value| value.parse::<HarnessKind>().ok())
            .collect::<BTreeSet<_>>();
        for integration in self.legacy.configured_integrations()? {
            configured.insert(legacy_harness(integration));
        }
        Ok(configured.into_iter().collect())
    }

    pub(crate) fn is_configured(&self, harness: HarnessKind) -> Result<bool, ConfigurationError> {
        Ok(self.configured_harnesses()?.contains(&harness))
    }

    pub(crate) fn is_active(&self, harness: HarnessKind) -> Result<bool, ConfigurationError> {
        let state = self.load_state()?;
        let Some(receipt) = state.harnesses.get(&harness.to_string()) else {
            return Ok(self.legacy_is_active(harness));
        };
        Ok(receipt.documents.iter().all(document_is_active) && self.legacy_is_active(harness))
    }

    pub(crate) fn credential_is_current(
        &self,
        harness: HarnessKind,
        saved_fingerprint: Option<&str>,
    ) -> Result<Option<bool>, ConfigurationError> {
        let state = self.load_state()?;
        Ok(state.harnesses.get(&harness.to_string()).map(|receipt| {
            saved_fingerprint
                .is_some_and(|fingerprint| fingerprint == receipt.credential_fingerprint)
        }))
    }

    pub(crate) fn search_status(
        &self,
        harness: HarnessKind,
    ) -> Result<Option<ManagedSearchStatus>, ConfigurationError> {
        let state = self.load_state()?;
        Ok(state
            .harnesses
            .get(&harness.to_string())
            .map(|receipt| ManagedSearchStatus {
                policy: receipt.search_policy,
                managed: receipt.search_managed,
            }))
    }

    pub(crate) fn configure(
        &self,
        harness: HarnessKind,
        config: &ResolvedConfig,
        models: &[CodingModelProfile],
        search_policy_override: Option<WebSearchPolicy>,
    ) -> Result<ConfigurationChange, ConfigurationError> {
        ensure_supported(harness)?;
        let default_model = preferred_model(models);
        let (api_key, fingerprint) = config
            .secrets
            .with_secret(&config.provider_credential_ref, |value| {
                (value.to_owned(), sha256(value.as_bytes()))
            })
            .map_err(PersistenceError::Secret)?;
        let mut state = self.load_state()?;
        let previous = state.harnesses.get(&harness.to_string());
        let search_policy = search_policy_override
            .or_else(|| previous.map(|receipt| receipt.search_policy))
            .unwrap_or_default();
        let search_managed = self.resolve_managed_search(
            harness,
            search_policy,
            previous.is_some_and(|receipt| receipt.search_managed),
        )?;
        let plans = self.plans_for(
            harness,
            &api_key,
            &config.provider_base_url,
            models,
            default_model,
            ManagedSearchStatus {
                policy: search_policy,
                managed: search_managed,
            },
        )?;
        let prepared =
            prepare_documents(&plans, previous.map(|receipt| receipt.documents.as_slice()))?;
        apply_prepared(&prepared)?;
        let catalog_change = match self.configure_catalogs(
            harness,
            models,
            &config.provider_base_url,
            &api_key,
            search_managed,
        ) {
            Ok(change) => change,
            Err(error) => {
                rollback_prepared(&prepared);
                return Err(error);
            }
        };
        let changed = catalog_change.as_ref().is_some_and(|change| change.changed)
            || prepared
                .iter()
                .any(|document| document.original.as_deref() != document.replacement.as_deref());
        let mut paths = prepared
            .iter()
            .filter(|document| receipt_manages_content(&document.receipt))
            .map(|document| document.path.clone())
            .collect::<Vec<_>>();
        if let Some(change) = catalog_change {
            paths.push(change.path);
            paths.extend(change.additional_paths);
        }
        paths.sort();
        paths.dedup();
        state.harnesses.insert(
            harness.to_string(),
            HarnessReceipt {
                credential_fingerprint: fingerprint,
                model_ids: models.iter().map(|model| model.id.clone()).collect(),
                search_policy,
                search_managed,
                documents: prepared
                    .iter()
                    .map(|document| document.receipt.clone())
                    .collect(),
            },
        );
        if let Err(error) = self.save_state(&state) {
            rollback_prepared(&prepared);
            return Err(error);
        }
        Ok(ConfigurationChange {
            changed,
            paths,
            model_count: models.len(),
            search: ManagedSearchStatus {
                policy: search_policy,
                managed: search_managed,
            },
        })
    }

    pub(crate) fn remove(
        &self,
        harness: HarnessKind,
    ) -> Result<RemovalOutcome, ConfigurationError> {
        ensure_supported(harness)?;
        let mut state = self.load_state()?;
        let Some(receipt) = state.harnesses.get(&harness.to_string()).cloned() else {
            return self.remove_legacy(harness);
        };
        let prepared = prepare_removals(&receipt.documents)?;
        self.remove_legacy(harness)?;
        apply_prepared(&prepared)?;
        state.harnesses.remove(&harness.to_string());
        if let Err(error) = self.save_state(&state) {
            rollback_prepared(&prepared);
            return Err(error);
        }
        Ok(RemovalOutcome::Removed)
    }

    pub(crate) fn remove_all(
        &self,
    ) -> Result<Vec<(HarnessKind, RemovalOutcome)>, ConfigurationError> {
        self.configured_harnesses()?
            .into_iter()
            .map(|harness| self.remove(harness).map(|outcome| (harness, outcome)))
            .collect()
    }

    pub(crate) fn paths_for_search(
        &self,
        harness: HarnessKind,
        search_managed: bool,
    ) -> Result<Vec<PathBuf>, ConfigurationError> {
        ensure_supported(harness)?;
        let placeholder_models = vec![CodingModelProfile::generic(DEFAULT_MODEL_ID)];
        let mut paths = self
            .plans_for(
                harness,
                "<saved API key>",
                "https://api.nan.builders/v1",
                &placeholder_models,
                DEFAULT_MODEL_ID,
                ManagedSearchStatus {
                    policy: WebSearchPolicy::Auto,
                    managed: search_managed,
                },
            )?
            .into_iter()
            .filter_map(|plan| match plan {
                DocumentPlan::Json(plan) if plan.entries.is_empty() => None,
                DocumentPlan::Json(plan) => Some(plan.path),
                DocumentPlan::Yaml(plan) => Some(plan.path),
                DocumentPlan::TextBlock(plan) if plan.body.is_none() => None,
                DocumentPlan::TextBlock(plan) => Some(plan.path),
                DocumentPlan::ExactFile(plan) if plan.payload.is_none() => None,
                DocumentPlan::ExactFile(plan) => Some(plan.path),
                DocumentPlan::Kimi(plan) => Some(plan.path),
            })
            .collect::<Vec<_>>();
        if let Some(integration) = catalog_integration(harness) {
            paths.extend(self.legacy.managed_catalog_paths(integration)?);
        }
        paths.sort();
        paths.dedup();
        Ok(paths)
    }

    pub(crate) fn plans_for(
        &self,
        harness: HarnessKind,
        api_key: &str,
        base_url: &str,
        models: &[CodingModelProfile],
        default_model: &str,
        search: ManagedSearchStatus,
    ) -> Result<Vec<DocumentPlan>, ConfigurationError> {
        for_harness(
            &self.paths,
            harness,
            api_key,
            base_url,
            models,
            default_model,
            search,
        )
    }

    pub(crate) fn resolve_managed_search(
        &self,
        harness: HarnessKind,
        policy: WebSearchPolicy,
        previously_managed: bool,
    ) -> Result<bool, ConfigurationError> {
        if policy == WebSearchPolicy::Disabled {
            return Ok(false);
        }
        if harness == HarnessKind::Aider {
            return if policy == WebSearchPolicy::Force {
                Err(SearchPolicyError::UnsupportedHarness(harness).into())
            } else {
                Ok(false)
            };
        }
        if matches!(
            harness,
            HarnessKind::Pi | HarnessKind::Omp | HarnessKind::PrimeAgent
        ) {
            return Ok(true);
        }
        let working_directory = env::current_dir().map_err(ConfigurationError::CurrentDirectory)?;
        let detected =
            inspect_search_configuration(harness, &self.paths.home_directory, &working_directory)?;
        Ok(match (policy, detected) {
            (WebSearchPolicy::Force | WebSearchPolicy::Auto, SearchConfiguration::ManagedNan) => {
                previously_managed
            }
            (WebSearchPolicy::Force, _) | (WebSearchPolicy::Auto, SearchConfiguration::None) => {
                true
            }
            (
                WebSearchPolicy::Auto,
                SearchConfiguration::External | SearchConfiguration::Unsupported,
            ) => false,
            (WebSearchPolicy::Disabled, _) => unreachable!("disabled returns before inspection"),
        })
    }

    pub(crate) fn load_state(&self) -> Result<ConfigurationState, ConfigurationError> {
        match fs::read(&self.paths.state_path) {
            Ok(contents) => {
                let state: ConfigurationState =
                    serde_json::from_slice(&contents).map_err(ConfigurationError::ParseState)?;
                if state.schema_version != STATE_SCHEMA_VERSION {
                    return Err(ConfigurationError::UnsupportedStateSchema(
                        state.schema_version,
                    ));
                }
                Ok(state)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(ConfigurationState::default())
            }
            Err(source) => Err(ConfigurationError::ReadState {
                path: self.paths.state_path.clone(),
                source,
            }),
        }
    }

    pub(crate) fn save_state(&self, state: &ConfigurationState) -> Result<(), ConfigurationError> {
        let payload =
            serde_json::to_vec_pretty(state).map_err(ConfigurationError::SerializeState)?;
        write_private_file(&self.paths.state_path, &payload, None)?;
        Ok(())
    }
}
