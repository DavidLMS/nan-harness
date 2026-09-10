use crate::config::{SearchConfigError, SearxngConfig, SearxngMode};
use nan_harness_private_fs::{
    PrivatePathKind, create_private_dir_all, restrict_file, restrict_path,
};
use serde::{Deserialize, Serialize};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use tempfile::Builder as TempFileBuilder;
use thiserror::Error;

const SCHEMA_VERSION: u8 = 1;
const MAX_CONFIG_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone)]
pub struct SearchConfigStore {
    path: PathBuf,
}

impl SearchConfigStore {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Loads a persisted configuration, returning `None` when it has not been created.
    ///
    /// Existing files are opened through the private-file contract before reading.
    ///
    /// # Errors
    ///
    /// Returns [`SearchConfigStoreError`] when the file cannot be read, parsed,
    /// validated, or has an unsupported schema.
    pub fn load(&self) -> Result<Option<SearxngConfig>, SearchConfigStoreError> {
        let (mut file, _) = match nan_harness_private_fs::open_private_read(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(SearchConfigStoreError::Read(error)),
        };
        if file.metadata().map_err(SearchConfigStoreError::Read)?.len() > MAX_CONFIG_BYTES {
            return Err(SearchConfigStoreError::ConfigurationTooLarge);
        }
        let mut payload = Vec::new();
        std::io::Read::read_to_end(&mut file, &mut payload)
            .map_err(SearchConfigStoreError::Read)?;
        let persisted: PersistedSearchConfig =
            serde_json::from_slice(&payload).map_err(SearchConfigStoreError::Parse)?;
        if persisted.schema_version != SCHEMA_VERSION {
            return Err(SearchConfigStoreError::UnsupportedSchema(
                persisted.schema_version,
            ));
        }
        SearxngConfig::new(persisted.mode, &persisted.base_url)
            .map(Some)
            .map_err(SearchConfigStoreError::InvalidConfiguration)
    }

    /// Atomically writes a configuration without accepting credentials or secret URL components.
    ///
    /// # Errors
    ///
    /// Returns [`SearchConfigStoreError`] when the parent directory cannot be
    /// protected or the replacement cannot be serialized or published.
    pub fn save(&self, config: &SearxngConfig) -> Result<(), SearchConfigStoreError> {
        let parent = self
            .path
            .parent()
            .ok_or(SearchConfigStoreError::MissingParent)?;
        create_private_dir_all(parent).map_err(SearchConfigStoreError::CreateDirectory)?;
        let persisted = PersistedSearchConfig {
            schema_version: SCHEMA_VERSION,
            mode: config.mode(),
            base_url: config.base_url_string(),
        };
        let payload =
            serde_json::to_vec_pretty(&persisted).map_err(SearchConfigStoreError::Serialize)?;
        atomic_write(&self.path, &payload).map_err(SearchConfigStoreError::Write)
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PersistedSearchConfig {
    schema_version: u8,
    mode: SearxngMode,
    base_url: String,
}

fn atomic_write(path: &Path, payload: &[u8]) -> Result<(), std::io::Error> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no parent")
    })?;
    let mut temporary = TempFileBuilder::new()
        .prefix(".nan-search-")
        .tempfile_in(parent)?;
    restrict_file(temporary.as_file_mut())?;
    temporary.write_all(payload)?;
    temporary.write_all(b"\n")?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    restrict_path(path, PrivatePathKind::File)?;
    Ok(())
}

#[derive(Debug, Error)]
pub enum SearchConfigStoreError {
    #[error("could not read SearXNG configuration")]
    Read(#[source] std::io::Error),
    #[error("SearXNG configuration is not valid JSON")]
    Parse(#[source] serde_json::Error),
    #[error("SearXNG configuration schema version {0} is unsupported")]
    UnsupportedSchema(u8),
    #[error("SearXNG configuration exceeds the 64 KiB limit")]
    ConfigurationTooLarge,
    #[error("SearXNG configuration contains an invalid endpoint: {0}")]
    InvalidConfiguration(#[source] SearchConfigError),
    #[error("could not serialize SearXNG configuration")]
    Serialize(#[source] serde_json::Error),
    #[error("SearXNG configuration path has no parent directory")]
    MissingParent,
    #[error("could not create the private SearXNG configuration directory")]
    CreateDirectory(#[source] std::io::Error),
    #[error("could not atomically write SearXNG configuration")]
    Write(#[source] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::{SearchConfigStore, SearchConfigStoreError};
    use crate::SearxngConfig;
    use std::fs;

    #[test]
    fn saves_and_loads_only_safe_endpoint_configuration() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let path = directory.path().join("search.json");
        let store = SearchConfigStore::new(&path);
        let config = SearxngConfig::local("http://127.0.0.1:8080").expect("URL should validate");
        store.save(&config).expect("configuration should save");
        let loaded = store
            .load()
            .expect("configuration should load")
            .expect("configuration should exist");
        assert_eq!(loaded, config);
        let payload = fs::read_to_string(path).expect("configuration should be readable");
        assert!(payload.contains("127.0.0.1"));
        assert!(!payload.contains("token"));
    }

    #[test]
    fn rejects_unknown_schema_without_overwriting_existing_state() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let path = directory.path().join("search.json");
        fs::write(
            &path,
            r#"{"schemaVersion":99,"mode":"local","baseUrl":"http://127.0.0.1:8080/"}"#,
        )
        .expect("fixture should write");
        let error = SearchConfigStore::new(path)
            .load()
            .expect_err("unknown schema should fail");
        assert!(matches!(
            error,
            SearchConfigStoreError::UnsupportedSchema(99)
        ));
    }

    #[test]
    fn rejects_oversized_configuration_before_parsing() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let path = directory.path().join("search.json");
        fs::write(&path, vec![b'x'; 64 * 1024 + 1]).expect("fixture should write");
        let error = SearchConfigStore::new(path)
            .load()
            .expect_err("oversized config should fail");
        assert!(matches!(
            error,
            SearchConfigStoreError::ConfigurationTooLarge
        ));
    }
}
