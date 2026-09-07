use super::super::{
    IntegrationState, LEGACY_PI_EXTENSION_RELATIVE_PATH, ManagedFile, PI_EXTENSION_RELATIVE_PATH,
    PRIME_EXTENSION_RELATIVE_PATH, PersistenceError, PersistenceManager, RemovalOutcome,
    permissions, read_optional, rollback_file, sha256,
};
use crate::commands::persistence::{ConfigurationHealth, read_managed_document};
use std::fs;
use std::path::{Path, PathBuf};

impl PersistenceManager {
    pub(crate) fn unpersist_pi(&self) -> Result<RemovalOutcome, PersistenceError> {
        let mut state = self.load_state()?;
        let Some(managed) = state.pi.clone() else {
            return Ok(RemovalOutcome::NotConfigured);
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
        if original.is_some() {
            fs::remove_file(&path).map_err(|source| PersistenceError::RemoveFile {
                path: path.clone(),
                source,
            })?;
        }
        state.pi = None;
        if let Err(error) = self.save_state(&state) {
            rollback_file(&path, original.as_deref(), original_permissions.as_ref());
            return Err(error);
        }
        Ok(RemovalOutcome::Removed)
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
        let mut state = self.load_state()?;
        let Some(managed) = state.prime_agent.clone() else {
            return Ok(RemovalOutcome::NotConfigured);
        };
        let path = managed
            .path
            .clone()
            .unwrap_or_else(|| self.home_directory.join(PRIME_EXTENSION_RELATIVE_PATH));
        let original = read_optional(&path)?;
        let original_permissions = permissions(&path)?;
        Self::remove_managed_file(&path, &managed)?;
        state.prime_agent = None;
        if let Err(error) = self.save_state(&state) {
            rollback_file(&path, original.as_deref(), original_permissions.as_ref());
            return Err(error);
        }
        Ok(RemovalOutcome::Removed)
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

    fn remove_managed_file(path: &Path, managed: &ManagedFile) -> Result<(), PersistenceError> {
        let Some(contents) = read_optional(path)? else {
            return Ok(());
        };
        if sha256(&contents) != managed.sha256 {
            return Err(PersistenceError::ManagedFileChanged(path.to_path_buf()));
        }
        fs::remove_file(path).map_err(|source| PersistenceError::RemoveFile {
            path: path.to_path_buf(),
            source,
        })
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
