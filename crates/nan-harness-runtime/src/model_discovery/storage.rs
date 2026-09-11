use nan_harness_core::{CodingModelProfile, coding_models_from_provider_ids};
use nan_harness_private_fs::{
    PrivatePathKind, create_private_dir_all, open_private_new, open_private_read, restrict_path,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::fmt::Write as _;
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};

const MAX_CACHE_BYTES: u64 = 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct CachedModels {
    schema_version: u8,
    fetched_at_unix_seconds: u64,
    ids: Vec<String>,
}

pub(super) struct ModelCache {
    path: PathBuf,
}

impl ModelCache {
    pub(super) fn default_directory() -> Option<PathBuf> {
        crate::search_policy::config_directory().map(|directory| directory.join("model-cache/v1"))
    }

    pub(super) fn open(directory: &Path, provider: &str, secret: &str) -> io::Result<Self> {
        create_private_dir_all(directory)?;
        restrict_path(directory, PrivatePathKind::Directory)?;
        let salt = load_or_create_salt(directory)?;
        let mut digest = Sha256::new();
        digest.update(b"nan-harness-model-cache-v1\0");
        digest.update(salt);
        // Length prefixes keep the full URL and credential unambiguous.
        for value in [provider.trim_end_matches('/'), secret] {
            digest.update((value.len() as u64).to_be_bytes());
            digest.update(value.as_bytes());
        }
        let mut scope = String::with_capacity(64);
        for byte in digest.finalize() {
            let _ = write!(&mut scope, "{byte:02x}");
        }
        Ok(Self {
            path: directory.join(format!("{scope}.json")),
        })
    }

    pub(super) fn load(&self) -> Option<(Vec<CodingModelProfile>, u64)> {
        let (file, _) = open_private_read(&self.path).ok()?;
        let mut payload = Vec::new();
        file.take(MAX_CACHE_BYTES + 1)
            .read_to_end(&mut payload)
            .ok()?;
        if payload.len() as u64 > MAX_CACHE_BYTES {
            return None;
        }
        let cached: CachedModels = serde_json::from_slice(&payload).ok()?;
        if cached.schema_version != 1 {
            return None;
        }
        let models = coding_models_from_provider_ids(cached.ids);
        if models.is_empty() {
            return None;
        }
        Some((models, cached.fetched_at_unix_seconds))
    }

    pub(super) fn save(&self, models: &[CodingModelProfile], fetched_at: u64) -> io::Result<()> {
        if models.is_empty() {
            return Ok(());
        }
        let cached = CachedModels {
            schema_version: 1,
            fetched_at_unix_seconds: fetched_at,
            ids: models.iter().map(|model| model.id.clone()).collect(),
        };
        let payload = serde_json::to_vec(&cached).map_err(io::Error::other)?;
        if payload.len() as u64 > MAX_CACHE_BYTES {
            return Err(io::Error::other("model cache exceeds size limit"));
        }
        let parent = self
            .path
            .parent()
            .ok_or_else(|| io::Error::other("cache has no parent"))?;
        let mut temporary = tempfile::Builder::new()
            .prefix(".models-")
            .make_in(parent, open_private_new)?;
        temporary.write_all(&payload)?;
        temporary.as_file().sync_all()?;
        temporary.persist(&self.path).map_err(|error| error.error)?;
        Ok(())
    }
}

fn load_or_create_salt(directory: &Path) -> io::Result<[u8; 32]> {
    let path = directory.join("scope.salt");
    match read_salt(&path) {
        Ok(salt) => return Ok(salt),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let mut salt = [0_u8; 32];
    getrandom::fill(&mut salt).map_err(io::Error::other)?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".salt-")
        .make_in(directory, open_private_new)?;
    temporary.write_all(&salt)?;
    temporary.as_file().sync_all()?;
    // Publish the complete salt without replacing a concurrent process's identity.
    match temporary.persist_noclobber(&path) {
        Ok(_) => Ok(salt),
        Err(error) if error.error.kind() == io::ErrorKind::AlreadyExists => read_salt(&path),
        Err(error) => Err(error.error),
    }
}

fn read_salt(path: &Path) -> io::Result<[u8; 32]> {
    let (mut file, _) = open_private_read(path)?;
    let mut salt = [0_u8; 32];
    file.read_exact(&mut salt)?;
    if file.read(&mut [0_u8; 1])? != 0 {
        return Err(io::Error::other("invalid model cache salt"));
    }
    Ok(salt)
}
