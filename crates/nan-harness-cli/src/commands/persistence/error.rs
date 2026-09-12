use nan_harness_core::SecretError;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum PersistenceError {
    #[error("could not determine the nan-harness configuration directory")]
    MissingConfigDirectory,
    #[error("could not determine the current user's home directory")]
    MissingHomeDirectory,
    #[error("could not render managed harness configuration: {0}")]
    RenderConfiguration(String),
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
    #[error("configuration file '{}' is not UTF-8: {source}", path.display())]
    InvalidUtf8 {
        path: PathBuf,
        source: std::string::FromUtf8Error,
    },
    #[error("legacy integration receipt contains unsupported file name '{0}'")]
    InvalidReceiptPath(String),
    #[error("'{}' was changed after nan-harness created it; refusing to overwrite it", .0.display())]
    ManagedFileChanged(PathBuf),
    #[error("both opencode.json and opencode.jsonc exist in '{}'; consolidate them before running `nanh config opencode`", .0.display())]
    AmbiguousOpenCodeConfig(PathBuf),
    #[error("OpenCode configuration '{}' is not a JSON object", .0.display())]
    RootIsNotObject(PathBuf),
    #[error("OpenCode configuration field 'provider' in '{}' is not an object", .0.display())]
    ProviderIsNotObject(PathBuf),
    #[error("OpenCode provider 'nan' in '{}' is not a valid object", .0.display())]
    InvalidManagedProvider(PathBuf),
    #[error("OpenCode provider 'nan' already exists in '{}' and is not managed by nan-harness", .0.display())]
    UnmanagedProviderConflict(PathBuf),
    #[error("OpenCode provider 'nan' in '{}' was changed after nan-harness created it; refusing to overwrite it", .0.display())]
    ManagedProviderChanged(PathBuf),
    #[error("managed configuration section in '{}' is invalid", .0.display())]
    InvalidManagedSection(PathBuf),
    #[error("'{}' contains a provider section that is not managed by nan-harness", .0.display())]
    UnmanagedSectionConflict(PathBuf),
    #[error("managed provider section in '{}' was changed after nan-harness created it", .0.display())]
    ManagedSectionChanged(PathBuf),
    #[error("managed configuration block markers are missing, duplicated, or out of order")]
    InvalidManagedBlock,
    #[error("{harness} configuration '{}' is not a JSON object", path.display())]
    ConfigRootIsNotObject {
        harness: &'static str,
        path: PathBuf,
    },
    #[error("{harness} configuration field '{field}' in '{}' is not an object", path.display())]
    ConfigFieldIsNotObject {
        harness: &'static str,
        field: &'static str,
        path: PathBuf,
    },
    #[error("{harness} configuration '{}' is not valid JSON: {message}", path.display())]
    ParseHarnessConfig {
        harness: &'static str,
        path: PathBuf,
        message: String,
    },
    #[error("OpenCode configuration '{}' is not valid JSONC: {message}", path.display())]
    ParseOpenCodeConfig { path: PathBuf, message: String },
    #[error("could not generate the OpenCode provider configuration: {0}")]
    GenerateOpenCodeProvider(String),
    #[error("could not serialize the managed OpenCode provider: {0}")]
    SerializeProvider(serde_json::Error),
    #[error("could not build the NaN model discovery client: {0}")]
    BuildClient(reqwest::Error),
    #[error("could not discover models from NaN: {0}")]
    DiscoverModels(reqwest::Error),
    #[error("NaN returned HTTP {0} during model discovery")]
    ModelDiscoveryStatus(u16),
    #[error("NaN returned a model catalog larger than the supported limit")]
    ModelDiscoveryTooLarge,
    #[error("NaN returned an invalid model catalog: {0}")]
    ParseModels(serde_json::Error),
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
    #[error("could not read user preferences: {0}")]
    ReadPreferences(std::io::Error),
    #[error("user preferences are not valid JSON: {0}")]
    ParsePreferences(serde_json::Error),
    #[error("user preferences schema {0} is not supported")]
    UnsupportedPreferencesSchema(u8),
    #[error("could not serialize user preferences: {0}")]
    SerializePreferences(serde_json::Error),
}

impl PersistenceError {
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::UnmanagedProviderConflict(_)
            | Self::UnmanagedSectionConflict(_)
            | Self::AmbiguousOpenCodeConfig(_) => "NH-INTEGRATION-002",
            Self::ManagedFileChanged(_)
            | Self::ManagedProviderChanged(_)
            | Self::ManagedSectionChanged(_)
            | Self::InvalidManagedBlock
            | Self::InvalidReceiptPath(_)
            | Self::UnsupportedStateSchema(_)
            | Self::UnsupportedPreferencesSchema(_) => "NH-INTEGRATION-003",
            Self::BuildClient(_)
            | Self::DiscoverModels(_)
            | Self::ModelDiscoveryStatus(_)
            | Self::ModelDiscoveryTooLarge
            | Self::ParseModels(_)
            | Self::NoModels
            | Self::Secret(_) => "NH-INTEGRATION-004",
            Self::RootIsNotObject(_)
            | Self::ProviderIsNotObject(_)
            | Self::InvalidManagedProvider(_)
            | Self::InvalidManagedSection(_)
            | Self::ParseOpenCodeConfig { .. }
            | Self::ParseHarnessConfig { .. }
            | Self::ConfigRootIsNotObject { .. }
            | Self::ConfigFieldIsNotObject { .. }
            | Self::GenerateOpenCodeProvider(_)
            | Self::SerializeProvider(_)
            | Self::RenderConfiguration(_) => "NH-INTEGRATION-005",
            Self::MissingConfigDirectory
            | Self::MissingHomeDirectory
            | Self::CreateDirectory { .. }
            | Self::ReadFile { .. }
            | Self::WriteFile { .. }
            | Self::RemoveFile { .. }
            | Self::InvalidPath(_)
            | Self::InvalidUtf8 { .. }
            | Self::CreateStateDirectory(_)
            | Self::ReadState(_)
            | Self::ParseState(_)
            | Self::SerializeState(_)
            | Self::ReadPreferences(_)
            | Self::ParsePreferences(_)
            | Self::SerializePreferences(_) => "NH-INTEGRATION-001",
        }
    }
}

// Terminal localization is separate from canonical Display used by machine contracts.
impl nan_harness_i18n::TerminalMessage for PersistenceError {
    #[expect(
        clippy::too_many_lines,
        reason = "exhaustive terminal projection keeps every error variant visible"
    )]
    fn terminal_message(&self, locale: nan_harness_i18n::Locale) -> String {
        use nan_harness_i18n::messages as m;
        if locale == nan_harness_i18n::Locale::En {
            return self.to_string();
        }
        match self {
            Self::MissingConfigDirectory => m::error_persistence_missing_config_directory(locale),
            Self::MissingHomeDirectory => m::error_persistence_missing_home_directory(locale),
            Self::RenderConfiguration(field_0) => {
                m::error_persistence_render_configuration(locale, &(field_0))
            }
            Self::CreateDirectory { path, source } => {
                m::error_persistence_create_directory(locale, &(source), &(path.display()))
            }
            Self::ReadFile { path, source } => {
                m::error_persistence_read_file(locale, &(source), &(path.display()))
            }
            Self::WriteFile { path, source } => {
                m::error_persistence_write_file(locale, &(source), &(path.display()))
            }
            Self::RemoveFile { path, source } => {
                m::error_persistence_remove_file(locale, &(source), &(path.display()))
            }
            Self::InvalidPath(field_0) => {
                m::error_persistence_invalid_path(locale, &(field_0.display()))
            }
            Self::InvalidUtf8 { path, source } => {
                m::error_persistence_invalid_utf8(locale, &(source), &(path.display()))
            }
            Self::InvalidReceiptPath(field_0) => {
                m::error_persistence_invalid_receipt_path(locale, &(field_0))
            }
            Self::ManagedFileChanged(field_0) => {
                m::error_persistence_managed_file_changed(locale, &(field_0.display()))
            }
            Self::AmbiguousOpenCodeConfig(field_0) => {
                m::error_persistence_ambiguous_open_code_config(locale, &(field_0.display()))
            }
            Self::RootIsNotObject(field_0) => {
                m::error_persistence_root_is_not_object(locale, &(field_0.display()))
            }
            Self::ProviderIsNotObject(field_0) => {
                m::error_persistence_provider_is_not_object(locale, &(field_0.display()))
            }
            Self::InvalidManagedProvider(field_0) => {
                m::error_persistence_invalid_managed_provider(locale, &(field_0.display()))
            }
            Self::UnmanagedProviderConflict(field_0) => {
                m::error_persistence_unmanaged_provider_conflict(locale, &(field_0.display()))
            }
            Self::ManagedProviderChanged(field_0) => {
                m::error_persistence_managed_provider_changed(locale, &(field_0.display()))
            }
            Self::InvalidManagedSection(field_0) => {
                m::error_persistence_invalid_managed_section(locale, &(field_0.display()))
            }
            Self::UnmanagedSectionConflict(field_0) => {
                m::error_persistence_unmanaged_section_conflict(locale, &(field_0.display()))
            }
            Self::ManagedSectionChanged(field_0) => {
                m::error_persistence_managed_section_changed(locale, &(field_0.display()))
            }
            Self::InvalidManagedBlock => m::error_persistence_invalid_managed_block(locale),
            Self::ConfigRootIsNotObject { harness, path } => {
                m::error_persistence_config_root_is_not_object(
                    locale,
                    &(harness),
                    &(path.display()),
                )
            }
            Self::ConfigFieldIsNotObject {
                harness,
                field,
                path,
            } => m::error_persistence_config_field_is_not_object(
                locale,
                &(field),
                &(harness),
                &(path.display()),
            ),
            Self::ParseHarnessConfig {
                harness,
                path,
                message,
            } => m::error_persistence_parse_harness_config(
                locale,
                &(harness),
                &(message),
                &(path.display()),
            ),
            Self::ParseOpenCodeConfig { path, message } => {
                m::error_persistence_parse_open_code_config(locale, &(message), &(path.display()))
            }
            Self::GenerateOpenCodeProvider(field_0) => {
                m::error_persistence_generate_open_code_provider(locale, &(field_0))
            }
            Self::SerializeProvider(field_0) => {
                m::error_persistence_serialize_provider(locale, &(field_0))
            }
            Self::BuildClient(field_0) => m::error_persistence_build_client(locale, &(field_0)),
            Self::DiscoverModels(field_0) => {
                m::error_persistence_discover_models(locale, &(field_0))
            }
            Self::ModelDiscoveryStatus(field_0) => {
                m::error_persistence_model_discovery_status(locale, &(field_0))
            }
            Self::ModelDiscoveryTooLarge => m::error_persistence_model_discovery_too_large(locale),
            Self::ParseModels(field_0) => m::error_persistence_parse_models(locale, &(field_0)),
            Self::NoModels => m::error_persistence_no_models(locale),
            Self::Secret(field_0) => m::error_persistence_secret(
                locale,
                &(nan_harness_i18n::TerminalMessage::terminal_message(field_0, locale)),
            ),
            Self::CreateStateDirectory(field_0) => {
                m::error_persistence_create_state_directory(locale, &(field_0))
            }
            Self::ReadState(field_0) => m::error_persistence_read_state(locale, &(field_0)),
            Self::ParseState(field_0) => m::error_persistence_parse_state(locale, &(field_0)),
            Self::UnsupportedStateSchema(field_0) => {
                m::error_persistence_unsupported_state_schema(locale, &(field_0))
            }
            Self::SerializeState(field_0) => {
                m::error_persistence_serialize_state(locale, &(field_0))
            }
            Self::ReadPreferences(field_0) => {
                m::error_persistence_read_preferences(locale, &(field_0))
            }
            Self::ParsePreferences(field_0) => {
                m::error_persistence_parse_preferences(locale, &(field_0))
            }
            Self::UnsupportedPreferencesSchema(field_0) => {
                m::error_persistence_unsupported_preferences_schema(locale, &(field_0))
            }
            Self::SerializePreferences(field_0) => {
                m::error_persistence_serialize_preferences(locale, &(field_0))
            }
        }
    }
}
