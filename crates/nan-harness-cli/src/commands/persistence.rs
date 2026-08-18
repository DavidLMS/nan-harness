use jsonc_parser::ParseOptions;
use jsonc_parser::cst::{CstInputValue, CstRootNode};
use nan_harness_adapters::persistent_provider_extension;
use nan_harness_core::{CodingModelProfile, SecretError, coding_models_from_provider_ids};
use nan_harness_runtime::ResolvedConfig;
use nan_harness_runtime::config::DEFAULT_PROVIDER_BASE_URL;
use reqwest::header::ACCEPT;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::env;
use std::fs::{self, Permissions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tempfile::Builder as TempFileBuilder;
use thiserror::Error;
use url::Url;

const STATE_SCHEMA_VERSION: u8 = 1;
const CONFIG_DIRECTORY_ENVIRONMENT_VARIABLE: &str = "NAN_HARNESS_CONFIG_DIR";
const PI_EXTENSION_RELATIVE_PATH: &str = ".pi/agent/extensions/nan-provider.mjs";
const OPENCODE_CONFIG_DIRECTORY: &str = ".config/opencode";
const OPENCODE_JSON: &str = "opencode.json";
const OPENCODE_JSONC: &str = "opencode.jsonc";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IntegrationChange {
    pub(crate) path: PathBuf,
    pub(crate) backup: Option<PathBuf>,
    pub(crate) changed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RemovalOutcome {
    Removed,
    NotConfigured,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ManagedFile {
    sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ManagedOpenCode {
    provider_sha256: String,
    file_name: String,
    created_file: bool,
    created_provider_object: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct IntegrationState {
    schema_version: u8,
    pi: Option<ManagedFile>,
    opencode: Option<ManagedOpenCode>,
}

impl Default for IntegrationState {
    fn default() -> Self {
        Self {
            schema_version: STATE_SCHEMA_VERSION,
            pi: None,
            opencode: None,
        }
    }
}

#[derive(Debug)]
pub(crate) struct PersistenceManager {
    state_directory: PathBuf,
    state_path: PathBuf,
    home_directory: PathBuf,
}

impl PersistenceManager {
    pub(crate) fn from_environment() -> Result<Self, PersistenceError> {
        let state_directory = config_directory().ok_or(PersistenceError::MissingConfigDirectory)?;
        let home_directory = home_directory().ok_or(PersistenceError::MissingHomeDirectory)?;
        Ok(Self::new(state_directory, home_directory))
    }

    fn new(state_directory: impl Into<PathBuf>, home_directory: impl Into<PathBuf>) -> Self {
        let state_directory = state_directory.into();
        let state_path = state_directory.join("integrations.json");
        Self {
            state_directory,
            state_path,
            home_directory: home_directory.into(),
        }
    }

    pub(crate) fn persist_pi(
        &self,
        provider_base_url: &str,
    ) -> Result<IntegrationChange, PersistenceError> {
        validate_provider_url(provider_base_url)?;
        let content = persistent_provider_extension(provider_base_url)
            .map_err(|error| PersistenceError::GeneratePiExtension(error.to_string()))?;
        let path = self.home_directory.join(PI_EXTENSION_RELATIVE_PATH);
        let original = read_optional(&path)?;
        let original_permissions = permissions(&path)?;
        let desired_hash = sha256(content.as_bytes());
        let mut state = self.load_state()?;

        if let Some(existing) = original.as_deref() {
            let existing_hash = sha256(existing);
            match state.pi.as_ref() {
                Some(managed) if managed.sha256 != existing_hash => {
                    return Err(PersistenceError::ManagedFileChanged(path));
                }
                None if existing_hash != desired_hash => {
                    return Err(PersistenceError::UnmanagedFileConflict(path));
                }
                _ => {}
            }
        }

        let changed = original.as_deref() != Some(content.as_bytes());
        if changed {
            write_private_file(&path, content.as_bytes(), original_permissions.as_ref())?;
        }
        state.pi = Some(ManagedFile {
            sha256: desired_hash,
        });
        if let Err(error) = self.save_state(&state) {
            rollback_file(&path, original.as_deref(), original_permissions.as_ref());
            return Err(error);
        }
        Ok(IntegrationChange {
            path,
            backup: None,
            changed,
        })
    }

    pub(crate) fn unpersist_pi(&self) -> Result<RemovalOutcome, PersistenceError> {
        let mut state = self.load_state()?;
        let Some(managed) = state.pi.clone() else {
            return Ok(RemovalOutcome::NotConfigured);
        };
        let path = self.home_directory.join(PI_EXTENSION_RELATIVE_PATH);
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

    pub(crate) fn pi_is_active(&self) -> bool {
        let Ok(state) = self.load_state() else {
            return false;
        };
        let Some(managed) = state.pi else {
            return false;
        };
        fs::read(self.home_directory.join(PI_EXTENSION_RELATIVE_PATH))
            .is_ok_and(|contents| sha256(&contents) == managed.sha256)
    }

    pub(crate) async fn persist_opencode(
        &self,
        config: &ResolvedConfig,
    ) -> Result<IntegrationChange, PersistenceError> {
        let models = discover_models(config).await?;
        let provider = opencode_provider(&models, &config.provider_base_url);
        let provider_hash = hash_input_value(&provider)?;
        let mut state = self.load_state()?;
        let path = self.opencode_config_path(state.opencode.as_ref())?;
        let original = read_optional(&path)?;
        let original_permissions = permissions(&path)?;
        let created_file = original.is_none();
        let source = original.as_deref().map_or_else(
            || "{}\n".to_owned(),
            |value| String::from_utf8_lossy(value).into_owned(),
        );
        let root = parse_jsonc(&source, &path)?;
        let root_object = root
            .object_value_or_create()
            .ok_or_else(|| PersistenceError::RootIsNotObject(path.clone()))?;
        let provider_property = root_object.get("provider");
        let created_provider_object = provider_property.is_none();
        let providers = match provider_property {
            Some(property) => property
                .object_value()
                .ok_or_else(|| PersistenceError::ProviderIsNotObject(path.clone()))?,
            None => root_object.object_value_or_set("provider"),
        };

        if let Some(existing) = providers.get("nan") {
            let existing_value = existing
                .to_serde_value()
                .ok_or_else(|| PersistenceError::InvalidManagedProvider(path.clone()))?;
            let existing_hash = hash_json_value(&existing_value)?;
            match state.opencode.as_ref() {
                Some(managed) if managed.provider_sha256 != existing_hash => {
                    return Err(PersistenceError::ManagedProviderChanged(path));
                }
                None if existing_hash != provider_hash => {
                    return Err(PersistenceError::UnmanagedProviderConflict(path));
                }
                _ => existing.set_value(provider.clone()),
            }
        } else {
            providers.append("nan", provider);
        }

        let rendered = root.to_string();
        let changed = original.as_deref() != Some(rendered.as_bytes());
        let backup = if state.opencode.is_none() && original.is_some() {
            create_backup(&path)?
        } else {
            None
        };
        if changed {
            write_private_file(&path, rendered.as_bytes(), original_permissions.as_ref())?;
        }
        state.opencode = Some(ManagedOpenCode {
            provider_sha256: provider_hash,
            file_name: file_name(&path)?,
            created_file: state
                .opencode
                .as_ref()
                .is_some_and(|managed| managed.created_file)
                || created_file,
            created_provider_object: state
                .opencode
                .as_ref()
                .is_some_and(|managed| managed.created_provider_object)
                || created_provider_object,
        });
        if let Err(error) = self.save_state(&state) {
            rollback_file(&path, original.as_deref(), original_permissions.as_ref());
            return Err(error);
        }
        Ok(IntegrationChange {
            path,
            backup,
            changed,
        })
    }

    pub(crate) fn unpersist_opencode(&self) -> Result<RemovalOutcome, PersistenceError> {
        let mut state = self.load_state()?;
        let Some(managed) = state.opencode.clone() else {
            return Ok(RemovalOutcome::NotConfigured);
        };
        validate_opencode_file_name(&managed.file_name)?;
        let path = self
            .home_directory
            .join(OPENCODE_CONFIG_DIRECTORY)
            .join(&managed.file_name);
        let original = read_optional(&path)?;
        let Some(contents) = original.as_deref() else {
            state.opencode = None;
            self.save_state(&state)?;
            return Ok(RemovalOutcome::Removed);
        };
        let original_permissions = permissions(&path)?;
        let source = String::from_utf8_lossy(contents);
        let root = parse_jsonc(&source, &path)?;
        let root_object = root
            .object_value()
            .ok_or_else(|| PersistenceError::RootIsNotObject(path.clone()))?;
        let Some(providers) = root_object.object_value("provider") else {
            state.opencode = None;
            self.save_state(&state)?;
            return Ok(RemovalOutcome::Removed);
        };
        let Some(provider) = providers.get("nan") else {
            state.opencode = None;
            self.save_state(&state)?;
            return Ok(RemovalOutcome::Removed);
        };
        let provider_value = provider
            .to_serde_value()
            .ok_or_else(|| PersistenceError::InvalidManagedProvider(path.clone()))?;
        if hash_json_value(&provider_value)? != managed.provider_sha256 {
            return Err(PersistenceError::ManagedProviderChanged(path));
        }

        provider.remove();
        if managed.created_provider_object && providers.properties().is_empty() {
            root_object
                .get("provider")
                .expect("provider property was resolved above")
                .remove();
        }
        let rendered = root.to_string();
        if managed.created_file
            && root_object.properties().is_empty()
            && empty_jsonc_object_is_disposable(&rendered)
        {
            fs::remove_file(&path).map_err(|source| PersistenceError::RemoveFile {
                path: path.clone(),
                source,
            })?;
        } else {
            write_private_file(&path, rendered.as_bytes(), original_permissions.as_ref())?;
        }
        state.opencode = None;
        if let Err(error) = self.save_state(&state) {
            rollback_file(&path, original.as_deref(), original_permissions.as_ref());
            return Err(error);
        }
        Ok(RemovalOutcome::Removed)
    }

    fn opencode_config_path(
        &self,
        managed: Option<&ManagedOpenCode>,
    ) -> Result<PathBuf, PersistenceError> {
        let directory = self.home_directory.join(OPENCODE_CONFIG_DIRECTORY);
        if let Some(managed) = managed {
            validate_opencode_file_name(&managed.file_name)?;
            return Ok(directory.join(&managed.file_name));
        }
        let json = directory.join(OPENCODE_JSON);
        let jsonc = directory.join(OPENCODE_JSONC);
        match (json.exists(), jsonc.exists()) {
            (true, true) => Err(PersistenceError::AmbiguousOpenCodeConfig(directory)),
            (_, false) => Ok(json),
            (false, true) => Ok(jsonc),
        }
    }

    fn load_state(&self) -> Result<IntegrationState, PersistenceError> {
        match fs::read(&self.state_path) {
            Ok(contents) => {
                let state: IntegrationState =
                    serde_json::from_slice(&contents).map_err(PersistenceError::ParseState)?;
                if state.schema_version != STATE_SCHEMA_VERSION {
                    return Err(PersistenceError::UnsupportedStateSchema(
                        state.schema_version,
                    ));
                }
                Ok(state)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(IntegrationState::default())
            }
            Err(error) => Err(PersistenceError::ReadState(error)),
        }
    }

    fn save_state(&self, state: &IntegrationState) -> Result<(), PersistenceError> {
        fs::create_dir_all(&self.state_directory)
            .map_err(PersistenceError::CreateStateDirectory)?;
        let payload = serde_json::to_vec_pretty(state).map_err(PersistenceError::SerializeState)?;
        write_private_file(&self.state_path, &payload, None)
    }
}

#[derive(Debug, Deserialize)]
struct NanModelsResponse {
    data: Vec<NanModel>,
}

#[derive(Debug, Deserialize)]
struct NanModel {
    id: String,
}

async fn discover_models(
    config: &ResolvedConfig,
) -> Result<Vec<CodingModelProfile>, PersistenceError> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(PersistenceError::BuildClient)?;
    let endpoint = format!("{}/models", config.provider_base_url.trim_end_matches('/'));
    let request = config
        .secrets
        .with_secret(&config.provider_credential_ref, |api_key| {
            client
                .get(endpoint)
                .header(ACCEPT, "application/json")
                .bearer_auth(api_key)
        })
        .map_err(PersistenceError::Secret)?;
    let response = request
        .send()
        .await
        .map_err(PersistenceError::DiscoverModels)?;
    let status = response.status();
    if !status.is_success() {
        return Err(PersistenceError::ModelDiscoveryStatus(status.as_u16()));
    }
    let payload = response
        .json::<NanModelsResponse>()
        .await
        .map_err(PersistenceError::ParseModels)?;
    let models = coding_models_from_provider_ids(payload.data.into_iter().map(|model| model.id));
    if models.is_empty() {
        return Err(PersistenceError::NoModels);
    }
    Ok(models)
}

fn opencode_provider(models: &[CodingModelProfile], provider_base_url: &str) -> CstInputValue {
    let models = models
        .iter()
        .map(|model| {
            (
                model.id.clone(),
                CstInputValue::Object(vec![
                    (
                        "name".to_owned(),
                        CstInputValue::String(model.display_name.clone()),
                    ),
                    (
                        "description".to_owned(),
                        CstInputValue::String(model.description.clone()),
                    ),
                    (
                        "limit".to_owned(),
                        CstInputValue::Object(vec![
                            (
                                "context".to_owned(),
                                CstInputValue::Number(model.context_window.to_string()),
                            ),
                            (
                                "output".to_owned(),
                                CstInputValue::Number(model.max_output_tokens.to_string()),
                            ),
                        ]),
                    ),
                ]),
            )
        })
        .collect();
    CstInputValue::Object(vec![
        (
            "npm".to_owned(),
            CstInputValue::String("@ai-sdk/openai-compatible".to_owned()),
        ),
        ("name".to_owned(), CstInputValue::String("NaN".to_owned())),
        (
            "options".to_owned(),
            CstInputValue::Object(vec![
                (
                    "apiKey".to_owned(),
                    CstInputValue::String("{env:NAN_API_KEY}".to_owned()),
                ),
                (
                    "baseURL".to_owned(),
                    CstInputValue::String(provider_base_url.to_owned()),
                ),
            ]),
        ),
        ("models".to_owned(), CstInputValue::Object(models)),
    ])
}

fn parse_jsonc(source: &str, path: &Path) -> Result<CstRootNode, PersistenceError> {
    CstRootNode::parse(source, &ParseOptions::default()).map_err(|error| {
        PersistenceError::ParseOpenCodeConfig {
            path: path.to_path_buf(),
            message: error.to_string(),
        }
    })
}

fn hash_input_value(value: &CstInputValue) -> Result<String, PersistenceError> {
    let root = CstRootNode::parse("{}", &ParseOptions::default())
        .map_err(|error| PersistenceError::GenerateOpenCodeProvider(error.to_string()))?;
    root.set_value(value.clone());
    let value = root
        .to_serde_value()
        .ok_or(PersistenceError::GenerateOpenCodeProvider(
            "provider value is empty".to_owned(),
        ))?;
    hash_json_value(&value)
}

fn hash_json_value(value: &serde_json::Value) -> Result<String, PersistenceError> {
    let encoded = serde_json::to_vec(value).map_err(PersistenceError::SerializeProvider)?;
    Ok(sha256(&encoded))
}

fn sha256(value: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    let digest = Sha256::digest(value);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn empty_jsonc_object_is_disposable(value: &str) -> bool {
    value
        .chars()
        .all(|character| character.is_whitespace() || matches!(character, '{' | '}'))
}

fn validate_provider_url(value: &str) -> Result<(), PersistenceError> {
    let url = Url::parse(value).map_err(PersistenceError::InvalidProviderUrl)?;
    if matches!(url.scheme(), "http" | "https") && url.host_str().is_some() {
        Ok(())
    } else {
        Err(PersistenceError::UnsupportedProviderUrl)
    }
}

pub(crate) fn effective_provider_base_url(explicit: Option<&str>) -> String {
    explicit
        .map(ToOwned::to_owned)
        .or_else(|| env::var("NAN_BASE_URL").ok())
        .unwrap_or_else(|| DEFAULT_PROVIDER_BASE_URL.to_owned())
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, PersistenceError> {
    match fs::read(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(PersistenceError::ReadFile {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn permissions(path: &Path) -> Result<Option<Permissions>, PersistenceError> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(Some(metadata.permissions())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(PersistenceError::ReadFile {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn write_private_file(
    path: &Path,
    payload: &[u8],
    permissions: Option<&Permissions>,
) -> Result<(), PersistenceError> {
    let parent = path
        .parent()
        .ok_or_else(|| PersistenceError::InvalidPath(path.to_path_buf()))?;
    fs::create_dir_all(parent).map_err(|source| PersistenceError::CreateDirectory {
        path: parent.to_path_buf(),
        source,
    })?;
    let mut temporary = TempFileBuilder::new()
        .prefix(".nan-")
        .tempfile_in(parent)
        .map_err(|source| PersistenceError::WriteFile {
            path: path.to_path_buf(),
            source,
        })?;
    temporary
        .write_all(payload)
        .and_then(|()| temporary.flush())
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|source| PersistenceError::WriteFile {
            path: path.to_path_buf(),
            source,
        })?;
    set_permissions(temporary.as_file(), permissions).map_err(|source| {
        PersistenceError::WriteFile {
            path: path.to_path_buf(),
            source,
        }
    })?;
    temporary
        .persist(path)
        .map_err(|error| PersistenceError::WriteFile {
            path: path.to_path_buf(),
            source: error.error,
        })?;
    Ok(())
}

fn set_permissions(
    file: &fs::File,
    permissions: Option<&Permissions>,
) -> Result<(), std::io::Error> {
    if let Some(permissions) = permissions {
        return file.set_permissions(permissions.clone());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        file.set_permissions(Permissions::from_mode(0o600))
    }
    #[cfg(not(unix))]
    Ok(())
}

fn create_backup(path: &Path) -> Result<Option<PathBuf>, PersistenceError> {
    let file_name = file_name(path)?;
    let backup = path.with_file_name(format!("{file_name}.nan-backup"));
    if backup.exists() {
        return Ok(None);
    }
    fs::copy(path, &backup).map_err(|source| PersistenceError::BackupFile {
        path: path.to_path_buf(),
        backup: backup.clone(),
        source,
    })?;
    Ok(Some(backup))
}

fn rollback_file(path: &Path, original: Option<&[u8]>, permissions: Option<&Permissions>) {
    match original {
        Some(contents) => {
            let _ = write_private_file(path, contents, permissions);
        }
        None => {
            let _ = fs::remove_file(path);
        }
    }
}

fn file_name(path: &Path) -> Result<String, PersistenceError> {
    path.file_name()
        .and_then(|value| value.to_str())
        .map(ToOwned::to_owned)
        .ok_or_else(|| PersistenceError::InvalidPath(path.to_path_buf()))
}

fn validate_opencode_file_name(value: &str) -> Result<(), PersistenceError> {
    if matches!(value, OPENCODE_JSON | OPENCODE_JSONC) {
        Ok(())
    } else {
        Err(PersistenceError::InvalidReceiptPath(value.to_owned()))
    }
}

fn config_directory() -> Option<PathBuf> {
    if let Some(directory) = env::var_os(CONFIG_DIRECTORY_ENVIRONMENT_VARIABLE) {
        return Some(PathBuf::from(directory));
    }
    #[cfg(target_os = "macos")]
    {
        home_directory().map(|home| home.join("Library/Application Support/nan-harness"))
    }
    #[cfg(target_os = "windows")]
    {
        env::var_os("APPDATA")
            .map(PathBuf::from)
            .map(|directory| directory.join("nan-harness"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .map(|directory| directory.join("nan-harness"))
            .or_else(|| home_directory().map(|home| home.join(".config/nan-harness")))
    }
}

fn home_directory() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        env::var_os("HOME").map(PathBuf::from)
    }
}

#[derive(Debug, Error)]
pub(crate) enum PersistenceError {
    #[error("could not determine the NaN configuration directory")]
    MissingConfigDirectory,
    #[error("could not determine the current user's home directory")]
    MissingHomeDirectory,
    #[error("provider base URL is invalid: {0}")]
    InvalidProviderUrl(url::ParseError),
    #[error("provider base URL must be an absolute HTTP or HTTPS URL")]
    UnsupportedProviderUrl,
    #[error("could not generate the persistent Pi extension: {0}")]
    GeneratePiExtension(String),
    #[error("could not create configuration directory '{}': {source}", path.display())]
    CreateDirectory {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not read configuration file '{}': {source}", path.display())]
    ReadFile {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not write configuration file '{}': {source}", path.display())]
    WriteFile {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not remove configuration file '{}': {source}", path.display())]
    RemoveFile {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("configuration path '{}' is invalid", .0.display())]
    InvalidPath(PathBuf),
    #[error("persistent integration receipt contains unsupported file name '{0}'")]
    InvalidReceiptPath(String),
    #[error("'{}' already exists and is not managed by NaN", .0.display())]
    UnmanagedFileConflict(PathBuf),
    #[error("'{}' was changed after NaN created it; refusing to overwrite it", .0.display())]
    ManagedFileChanged(PathBuf),
    #[error("both opencode.json and opencode.jsonc exist in '{}'; consolidate them before persisting NaN", .0.display())]
    AmbiguousOpenCodeConfig(PathBuf),
    #[error("OpenCode configuration '{}' is not a JSON object", .0.display())]
    RootIsNotObject(PathBuf),
    #[error("OpenCode configuration field 'provider' in '{}' is not an object", .0.display())]
    ProviderIsNotObject(PathBuf),
    #[error("OpenCode provider 'nan' in '{}' is not a valid object", .0.display())]
    InvalidManagedProvider(PathBuf),
    #[error("OpenCode provider 'nan' already exists in '{}' and is not managed by NaN", .0.display())]
    UnmanagedProviderConflict(PathBuf),
    #[error("OpenCode provider 'nan' in '{}' was changed after NaN created it; refusing to overwrite it", .0.display())]
    ManagedProviderChanged(PathBuf),
    #[error("OpenCode configuration '{}' is not valid JSONC: {message}", path.display())]
    ParseOpenCodeConfig { path: PathBuf, message: String },
    #[error("could not generate the OpenCode provider configuration: {0}")]
    GenerateOpenCodeProvider(String),
    #[error("could not serialize the managed OpenCode provider: {0}")]
    SerializeProvider(serde_json::Error),
    #[error("could not back up '{}' to '{}': {source}", path.display(), backup.display())]
    BackupFile {
        path: PathBuf,
        backup: PathBuf,
        source: std::io::Error,
    },
    #[error("could not build the NaN model discovery client: {0}")]
    BuildClient(reqwest::Error),
    #[error("could not discover models from NaN: {0}")]
    DiscoverModels(reqwest::Error),
    #[error("NaN returned HTTP {0} during model discovery")]
    ModelDiscoveryStatus(u16),
    #[error("NaN returned an invalid model catalog: {0}")]
    ParseModels(reqwest::Error),
    #[error("NaN returned no models for this credential")]
    NoModels,
    #[error("could not access the NaN credential: {0}")]
    Secret(SecretError),
    #[error("could not create the integration state directory: {0}")]
    CreateStateDirectory(std::io::Error),
    #[error("could not read integration state: {0}")]
    ReadState(std::io::Error),
    #[error("integration state is not valid JSON: {0}")]
    ParseState(serde_json::Error),
    #[error("integration state schema {0} is not supported")]
    UnsupportedStateSchema(u8),
    #[error("could not serialize integration state: {0}")]
    SerializeState(serde_json::Error),
}

impl PersistenceError {
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::UnmanagedFileConflict(_)
            | Self::UnmanagedProviderConflict(_)
            | Self::AmbiguousOpenCodeConfig(_) => "NH-INTEGRATION-002",
            Self::ManagedFileChanged(_)
            | Self::ManagedProviderChanged(_)
            | Self::InvalidReceiptPath(_)
            | Self::UnsupportedStateSchema(_) => "NH-INTEGRATION-003",
            Self::BuildClient(_)
            | Self::DiscoverModels(_)
            | Self::ModelDiscoveryStatus(_)
            | Self::ParseModels(_)
            | Self::NoModels
            | Self::Secret(_) => "NH-INTEGRATION-004",
            Self::RootIsNotObject(_)
            | Self::ProviderIsNotObject(_)
            | Self::InvalidManagedProvider(_)
            | Self::ParseOpenCodeConfig { .. }
            | Self::GenerateOpenCodeProvider(_)
            | Self::SerializeProvider(_)
            | Self::GeneratePiExtension(_)
            | Self::InvalidProviderUrl(_)
            | Self::UnsupportedProviderUrl => "NH-INTEGRATION-005",
            Self::MissingConfigDirectory
            | Self::MissingHomeDirectory
            | Self::CreateDirectory { .. }
            | Self::ReadFile { .. }
            | Self::WriteFile { .. }
            | Self::RemoveFile { .. }
            | Self::InvalidPath(_)
            | Self::BackupFile { .. }
            | Self::CreateStateDirectory(_)
            | Self::ReadState(_)
            | Self::ParseState(_)
            | Self::SerializeState(_) => "NH-INTEGRATION-001",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{PersistenceError, PersistenceManager, RemovalOutcome};
    use nan_harness_core::SecretValue;
    use nan_harness_runtime::{ConfigOverrides, ConfigResolver, ProcessEnvironment};
    use nan_harness_test_support::scripted_provider::{ProviderScenario, ScriptedProvider};

    #[test]
    fn pi_persistence_is_reversible_and_detects_manual_changes() {
        let root = tempfile::tempdir().expect("temporary root should exist");
        let manager = PersistenceManager::new(root.path().join("state"), root.path().join("home"));

        let change = manager
            .persist_pi("https://api.nan.builders/v1")
            .expect("Pi integration should persist");
        let content = std::fs::read_to_string(&change.path).expect("extension should exist");
        assert!(change.changed);
        assert!(content.contains("await fetch(`${baseUrl}/models`"));
        assert!(content.contains("process.env.NAN_API_KEY"));
        assert!(!content.contains("nan-secret"));
        assert!(manager.pi_is_active());

        std::fs::write(&change.path, "user change\n").expect("extension should change");
        assert!(matches!(
            manager.unpersist_pi(),
            Err(PersistenceError::ManagedFileChanged(_))
        ));
        std::fs::write(
            &change.path,
            super::persistent_provider_extension("https://api.nan.builders/v1")
                .expect("extension should render"),
        )
        .expect("extension should be restored");
        assert_eq!(
            manager
                .unpersist_pi()
                .expect("Pi integration should be removed"),
            RemovalOutcome::Removed
        );
        assert!(!change.path.exists());
    }

    #[test]
    fn opencode_merge_preserves_comments_and_removes_only_nan() {
        let root = tempfile::tempdir().expect("temporary root should exist");
        let home = root.path().join("home");
        let config = home.join(".config/opencode/opencode.jsonc");
        std::fs::create_dir_all(config.parent().expect("config should have parent"))
            .expect("config directory should exist");
        std::fs::write(
            &config,
            "{\n  // keep this comment\n  \"provider\": {\n    \"custom\": { \"name\": \"Custom\" },\n  },\n}\n",
        )
        .expect("config should be written");
        let manager = PersistenceManager::new(root.path().join("state"), &home);
        let mut state = manager.load_state().expect("state should load");
        let path = manager
            .opencode_config_path(None)
            .expect("config path should resolve");
        let original = std::fs::read(&path).expect("config should be readable");
        let root_node = super::parse_jsonc(&String::from_utf8_lossy(&original), &path)
            .expect("config should parse");
        let root_object = root_node.object_value().expect("root should be object");
        let providers = root_object
            .object_value("provider")
            .expect("providers should exist");
        let models = nan_harness_core::coding_models_from_provider_ids([
            "qwen3.6".to_owned(),
            "mimo-v2.5".to_owned(),
        ]);
        let provider = super::opencode_provider(&models, "https://api.nan.builders/v1");
        let hash = super::hash_input_value(&provider).expect("provider should hash");
        providers.append("nan", provider);
        let rendered = root_node.to_string();
        super::write_private_file(&path, rendered.as_bytes(), None).expect("config should update");
        state.opencode = Some(super::ManagedOpenCode {
            provider_sha256: hash,
            file_name: "opencode.jsonc".to_owned(),
            created_file: false,
            created_provider_object: false,
        });
        manager.save_state(&state).expect("state should persist");

        let merged = std::fs::read_to_string(&config).expect("config should be readable");
        assert!(merged.contains("// keep this comment"));
        assert!(merged.contains("\"custom\""));
        assert!(merged.contains("\"nan\""));
        assert!(merged.contains("{env:NAN_API_KEY}"));

        assert_eq!(
            manager
                .unpersist_opencode()
                .expect("OpenCode integration should be removed"),
            RemovalOutcome::Removed
        );
        let restored = std::fs::read_to_string(&config).expect("config should remain");
        assert!(restored.contains("// keep this comment"));
        assert!(restored.contains("\"custom\""));
        assert!(!restored.contains("\"nan\""));
    }

    #[tokio::test]
    async fn opencode_persistence_discovers_the_current_credential_catalog() {
        let provider = ScriptedProvider::start(ProviderScenario::inventory("unused"))
            .await
            .expect("scripted provider should start");
        let root = tempfile::tempdir().expect("temporary root should exist");
        let manager = PersistenceManager::new(root.path().join("state"), root.path().join("home"));
        let config = ConfigResolver::resolve(
            &ProcessEnvironment,
            ConfigOverrides {
                provider_base_url: Some(provider.base_url().to_owned()),
                nan_api_key: Some(
                    SecretValue::new("test-api-key").expect("test credential should be valid"),
                ),
            },
        )
        .expect("test configuration should resolve");

        let change = manager
            .persist_opencode(&config)
            .await
            .expect("OpenCode integration should persist");
        let persisted = std::fs::read_to_string(&change.path)
            .expect("OpenCode configuration should be readable");
        for model in ["qwen3.6", "deepseek-v4-flash", "mimo-v2.5", "gemma4"] {
            assert!(
                persisted.contains(model),
                "missing discovered model {model}"
            );
        }
        assert!(persisted.contains("{env:NAN_API_KEY}"));
        assert!(!persisted.contains("test-api-key"));

        let closing_brace = persisted
            .rfind('}')
            .expect("OpenCode configuration should be an object");
        let mut user_modified = persisted;
        user_modified.insert_str(closing_brace, "  // user-owned note\n");
        std::fs::write(&change.path, user_modified)
            .expect("user comment should be added to the configuration");

        assert_eq!(
            manager
                .unpersist_opencode()
                .expect("OpenCode integration should be removed"),
            RemovalOutcome::Removed
        );
        let preserved = std::fs::read_to_string(&change.path)
            .expect("a user-modified configuration should remain");
        assert!(preserved.contains("// user-owned note"));
        assert!(!preserved.contains("\"nan\""));

        provider
            .shutdown()
            .await
            .expect("scripted provider should stop");
    }
}
