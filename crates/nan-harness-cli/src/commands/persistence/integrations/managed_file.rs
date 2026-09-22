use super::super::{
    IntegrationState, LEGACY_PI_EXTENSION_RELATIVE_PATH, ManagedFile, PI_EXTENSION_RELATIVE_PATH,
    PRIME_EXTENSION_RELATIVE_PATH, PersistenceError, PersistenceManager, RemovalOutcome,
    permissions, read_optional, sha256,
};
use crate::commands::persistence::PreparedFileChange;
use crate::commands::persistence::{ConfigurationHealth, read_managed_document};
use std::path::PathBuf;

impl PersistenceManager {
    pub(crate) fn unpersist_pi(&self) -> Result<RemovalOutcome, PersistenceError> {
        let (files, outcome) = self.prepare_remove_pi()?;
        self.publish_configuration_files(&files)?;
        Ok(outcome)
    }

    pub(crate) fn prepare_remove_pi(
        &self,
    ) -> Result<(Vec<PreparedFileChange>, RemovalOutcome), PersistenceError> {
        let (mut state, receipt) = self.prepare_state()?;
        let Some(managed) = state.pi.clone() else {
            return Ok((Vec::new(), RemovalOutcome::NotConfigured));
        };
        let path = managed.path.clone().unwrap_or_else(|| {
            let current = self.home_directory.join(PI_EXTENSION_RELATIVE_PATH);
            if current.exists() {
                current
            } else {
                self.home_directory.join(LEGACY_PI_EXTENSION_RELATIVE_PATH)
            }
        });
        let original = read_optional(&path)?;
        let original_permissions = permissions(&path)?;
        if let Some(contents) = original.as_deref()
            && sha256(contents) != managed.sha256
        {
            return Err(PersistenceError::ManagedFileChanged(path));
        }
        state.pi = None;
        let files = vec![PreparedFileChange {
            path,
            original,
            replacement_permissions: original_permissions.clone(),
            original_permissions,
            replacement: None,
        }];
        Ok((
            Self::prepare_integration_files(files, &state, receipt)?,
            RemovalOutcome::Removed,
        ))
    }

    #[cfg(test)]
    pub(crate) fn pi_is_active(&self) -> bool {
        self.inspect_pi()
            .is_ok_and(|health| health.is_some_and(ConfigurationHealth::is_active))
    }

    pub(crate) fn inspect_pi(&self) -> Result<Option<ConfigurationHealth>, PersistenceError> {
        self.inspect_managed_file(
            |state| state.pi.as_ref(),
            |managed| {
                managed
                    .path
                    .clone()
                    .unwrap_or_else(|| self.home_directory.join(PI_EXTENSION_RELATIVE_PATH))
            },
        )
    }

    pub(crate) fn unpersist_prime_agent(&self) -> Result<RemovalOutcome, PersistenceError> {
        let (files, outcome) = self.prepare_remove_prime_agent()?;
        self.publish_configuration_files(&files)?;
        Ok(outcome)
    }

    pub(crate) fn prepare_remove_prime_agent(
        &self,
    ) -> Result<(Vec<PreparedFileChange>, RemovalOutcome), PersistenceError> {
        let (mut state, receipt) = self.prepare_state()?;
        let Some(managed) = state.prime_agent.clone() else {
            return Ok((Vec::new(), RemovalOutcome::NotConfigured));
        };
        let path = managed
            .path
            .clone()
            .unwrap_or_else(|| self.home_directory.join(PRIME_EXTENSION_RELATIVE_PATH));
        let original = read_optional(&path)?;
        let original_permissions = permissions(&path)?;
        if original
            .as_ref()
            .is_some_and(|contents| sha256(contents) != managed.sha256)
        {
            return Err(PersistenceError::ManagedFileChanged(path));
        }
        state.prime_agent = None;
        let files = vec![PreparedFileChange {
            path,
            original,
            replacement_permissions: original_permissions.clone(),
            original_permissions,
            replacement: None,
        }];
        Ok((
            Self::prepare_integration_files(files, &state, receipt)?,
            RemovalOutcome::Removed,
        ))
    }

    #[cfg(test)]
    pub(crate) fn prime_agent_is_active(&self) -> bool {
        self.inspect_prime_agent()
            .is_ok_and(|health| health.is_some_and(ConfigurationHealth::is_active))
    }

    pub(crate) fn inspect_prime_agent(
        &self,
    ) -> Result<Option<ConfigurationHealth>, PersistenceError> {
        self.inspect_managed_file(
            |state| state.prime_agent.as_ref(),
            |managed| {
                managed
                    .path
                    .clone()
                    .unwrap_or_else(|| self.prime_directory.join("extensions/nan-provider.js"))
            },
        )
    }

    fn inspect_managed_file(
        &self,
        select: impl FnOnce(&IntegrationState) -> Option<&ManagedFile>,
        path: impl FnOnce(&ManagedFile) -> PathBuf,
    ) -> Result<Option<ConfigurationHealth>, PersistenceError> {
        let state = self.load_state()?;
        let Some(managed) = select(&state) else {
            return Ok(None);
        };
        Ok(Some(match read_managed_document(&path(managed)) {
            Ok(contents) => ConfigurationHealth::from_matches(sha256(&contents) == managed.sha256),
            Err(health) => health,
        }))
    }
}
