use crate::commands::credentials::CredentialError;
use crate::commands::pen_desktop::PenDesktopError;
use crate::commands::persistence::PersistenceError;
use nan_harness_core::HarnessKind;
use nan_harness_runtime::SearchPolicyError;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum ConfigurationError {
    #[error("a harness is required; use `nanh config <harness>` or `nanh config --status`")]
    HarnessRequired,
    #[error("--yes only applies to first-time native configuration or --remove-all")]
    UnusedYes,
    #[error("--no-search and --force-search apply only when configuring or refreshing one harness")]
    UnusedSearchPolicy,
    #[error(
        "{0} cannot store this provider configuration natively; launch it through nan-harness instead"
    )]
    BridgeOnly(HarnessKind),
    #[error("{0} is not configured by nan-harness; run `nanh config {0}` first")]
    RefreshRequiresConfiguration(HarnessKind),
    #[error("Pen Desktop is not configured by nan-harness; run `nanh config pen` first")]
    PenNotConfigured,
    #[error("this configuration change requires an interactive confirmation or --yes")]
    ConfirmationRequired,
    #[error("could not determine the nan-harness state directory")]
    MissingStateDirectory,
    #[error("could not determine the current user's home directory")]
    MissingHomeDirectory,
    #[error("could not read the current working directory: {0}")]
    CurrentDirectory(std::io::Error),
    #[error("managed configuration receipt does not match the current harness layout")]
    ReceiptMismatch,
    #[error("managed JSON path is empty")]
    InvalidManagedPath,
    #[error("managed text block markers are missing, duplicated, or out of order")]
    InvalidManagedBlock,
    #[error("'{}' already contains configuration that nan-harness does not own", .0.display())]
    UnmanagedDocumentConflict(PathBuf),
    #[error("'{}' changed after nan-harness configured it; refusing to overwrite user changes", .0.display())]
    ManagedDocumentChanged(PathBuf),
    #[error("configuration document '{}' must contain a JSON object", .0.display())]
    DocumentRootNotObject(PathBuf),
    #[error("configuration field '{field}' in '{}' must contain a JSON object", path.display())]
    DocumentFieldNotObject { path: PathBuf, field: String },
    #[error("configuration field '{field}' in '{}' must contain a JSON array", path.display())]
    DocumentFieldNotArray { path: PathBuf, field: String },
    #[error("could not read configuration document '{}': {source}", path.display())]
    ReadDocument {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not remove configuration document '{}': {source}", path.display())]
    RemoveDocument {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("configuration document '{}' is not valid JSON: {source}", path.display())]
    ParseDocument {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("configuration document '{}' is not valid YAML: {source}", path.display())]
    ParseYaml {
        path: PathBuf,
        source: serde_yaml_ng::Error,
    },
    #[error("could not serialize a YAML configuration document: {0}")]
    SerializeYaml(serde_yaml_ng::Error),
    #[error("configuration document '{}' must contain a YAML mapping", .0.display())]
    YamlRootNotMapping(PathBuf),
    #[error("configuration field '{field}' in '{}' must contain a YAML mapping", path.display())]
    YamlFieldNotMapping { path: PathBuf, field: String },
    #[error("configuration field '{field}' in '{}' must contain a YAML sequence", path.display())]
    YamlFieldNotSequence { path: PathBuf, field: String },
    #[error("configuration document '{}' is not valid TOML: {source}", path.display())]
    ParseToml {
        path: PathBuf,
        source: toml_edit::TomlError,
    },
    #[error("could not normalize managed TOML data: {0}")]
    NormalizeToml(toml_edit::de::Error),
    #[error("configuration field '{field}' in '{}' must contain a TOML table", path.display())]
    TomlFieldNotTable { path: PathBuf, field: String },
    #[error("configuration field '{field}' in '{}' must contain a TOML string", path.display())]
    TomlFieldNotString { path: PathBuf, field: String },
    #[error("configuration document '{}' is not UTF-8: {source}", path.display())]
    InvalidUtf8 {
        path: PathBuf,
        source: std::string::FromUtf8Error,
    },
    #[error("could not read nan-harness configuration receipts '{}': {source}", path.display())]
    ReadState {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("nan-harness configuration receipts are not valid JSON: {0}")]
    ParseState(serde_json::Error),
    #[error("nan-harness configuration receipt schema {0} is not supported")]
    UnsupportedStateSchema(u8),
    #[error("could not serialize nan-harness configuration receipts: {0}")]
    SerializeState(serde_json::Error),
    #[error("could not serialize a harness configuration document: {0}")]
    SerializeDocument(serde_json::Error),
    #[error(
        "model '{model}' has a {field} that cannot be represented in native TOML configuration"
    )]
    ModelValueOutOfRange { field: &'static str, model: String },
    #[error("could not read confirmation: {0}")]
    Prompt(std::io::Error),
    #[error(transparent)]
    Credential(#[from] CredentialError),
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    #[error(transparent)]
    SearchPolicy(#[from] SearchPolicyError),
    #[error(transparent)]
    Pen(#[from] PenDesktopError),
}

impl ConfigurationError {
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::BridgeOnly(_)
            | Self::RefreshRequiresConfiguration(_)
            | Self::PenNotConfigured
            | Self::HarnessRequired
            | Self::UnusedYes
            | Self::UnusedSearchPolicy => "NH-CONFIG-001",
            Self::ConfirmationRequired | Self::Prompt(_) => "NH-CONFIG-002",
            Self::UnmanagedDocumentConflict(_) => "NH-CONFIG-003",
            Self::ManagedDocumentChanged(_)
            | Self::ReceiptMismatch
            | Self::InvalidManagedBlock
            | Self::InvalidManagedPath => "NH-CONFIG-004",
            Self::Credential(error) => error.code(),
            Self::Persistence(error) => error.code(),
            Self::Pen(error) => error.code(),
            _ => "NH-CONFIG-005",
        }
    }
}

// Terminal localization is separate from canonical Display used by machine contracts.
impl nan_harness_i18n::TerminalMessage for ConfigurationError {
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
            Self::HarnessRequired => m::error_configuration_harness_required(locale),
            Self::UnusedYes => m::error_configuration_unused_yes(locale),
            Self::UnusedSearchPolicy => m::error_configuration_unused_search_policy(locale),
            Self::BridgeOnly(field_0) => m::error_configuration_bridge_only(locale, &(field_0)),
            Self::RefreshRequiresConfiguration(field_0) => {
                m::error_configuration_refresh_requires_configuration(locale, &(field_0))
            }
            Self::PenNotConfigured => m::error_configuration_pen_not_configured(locale),
            Self::ConfirmationRequired => m::error_configuration_confirmation_required(locale),
            Self::MissingStateDirectory => m::error_configuration_missing_state_directory(locale),
            Self::MissingHomeDirectory => m::error_configuration_missing_home_directory(locale),
            Self::CurrentDirectory(field_0) => {
                m::error_configuration_current_directory(locale, &(field_0))
            }
            Self::ReceiptMismatch => m::error_configuration_receipt_mismatch(locale),
            Self::InvalidManagedPath => m::error_configuration_invalid_managed_path(locale),
            Self::InvalidManagedBlock => m::error_configuration_invalid_managed_block(locale),
            Self::UnmanagedDocumentConflict(field_0) => {
                m::error_configuration_unmanaged_document_conflict(locale, &(field_0.display()))
            }
            Self::ManagedDocumentChanged(field_0) => {
                m::error_configuration_managed_document_changed(locale, &(field_0.display()))
            }
            Self::DocumentRootNotObject(field_0) => {
                m::error_configuration_document_root_not_object(locale, &(field_0.display()))
            }
            Self::DocumentFieldNotObject { path, field } => {
                m::error_configuration_document_field_not_object(
                    locale,
                    &(field),
                    &(path.display()),
                )
            }
            Self::DocumentFieldNotArray { path, field } => {
                m::error_configuration_document_field_not_array(locale, &(field), &(path.display()))
            }
            Self::ReadDocument { path, source } => {
                m::error_configuration_read_document(locale, &(source), &(path.display()))
            }
            Self::RemoveDocument { path, source } => {
                m::error_configuration_remove_document(locale, &(source), &(path.display()))
            }
            Self::ParseDocument { path, source } => {
                m::error_configuration_parse_document(locale, &(source), &(path.display()))
            }
            Self::ParseYaml { path, source } => {
                m::error_configuration_parse_yaml(locale, &(source), &(path.display()))
            }
            Self::SerializeYaml(field_0) => {
                m::error_configuration_serialize_yaml(locale, &(field_0))
            }
            Self::YamlRootNotMapping(field_0) => {
                m::error_configuration_yaml_root_not_mapping(locale, &(field_0.display()))
            }
            Self::YamlFieldNotMapping { path, field } => {
                m::error_configuration_yaml_field_not_mapping(locale, &(field), &(path.display()))
            }
            Self::YamlFieldNotSequence { path, field } => {
                m::error_configuration_yaml_field_not_sequence(locale, &(field), &(path.display()))
            }
            Self::ParseToml { path, source } => {
                m::error_configuration_parse_toml(locale, &(source), &(path.display()))
            }
            Self::NormalizeToml(field_0) => {
                m::error_configuration_normalize_toml(locale, &(field_0))
            }
            Self::TomlFieldNotTable { path, field } => {
                m::error_configuration_toml_field_not_table(locale, &(field), &(path.display()))
            }
            Self::TomlFieldNotString { path, field } => {
                m::error_configuration_toml_field_not_string(locale, &(field), &(path.display()))
            }
            Self::InvalidUtf8 { path, source } => {
                m::error_configuration_invalid_utf8(locale, &(source), &(path.display()))
            }
            Self::ReadState { path, source } => {
                m::error_configuration_read_state(locale, &(source), &(path.display()))
            }
            Self::ParseState(field_0) => m::error_configuration_parse_state(locale, &(field_0)),
            Self::UnsupportedStateSchema(field_0) => {
                m::error_configuration_unsupported_state_schema(locale, &(field_0))
            }
            Self::SerializeState(field_0) => {
                m::error_configuration_serialize_state(locale, &(field_0))
            }
            Self::SerializeDocument(field_0) => {
                m::error_configuration_serialize_document(locale, &(field_0))
            }
            Self::ModelValueOutOfRange { field, model } => {
                m::error_configuration_model_value_out_of_range(locale, &(field), &(model))
            }
            Self::Prompt(field_0) => m::error_configuration_prompt(locale, &(field_0)),
            Self::Credential(field_0) => {
                nan_harness_i18n::TerminalMessage::terminal_message(field_0, locale)
            }
            Self::Persistence(field_0) => {
                nan_harness_i18n::TerminalMessage::terminal_message(field_0, locale)
            }
            Self::SearchPolicy(field_0) => {
                nan_harness_i18n::TerminalMessage::terminal_message(field_0, locale)
            }
            Self::Pen(field_0) => {
                nan_harness_i18n::TerminalMessage::terminal_message(field_0, locale)
            }
        }
    }
}
