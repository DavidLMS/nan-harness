mod error;
mod filesystem;
mod health;
mod helpers;
pub(crate) use health::{ConfigurationHealth, read_managed_document};
mod integrations;
mod managed;
mod models;
mod orchestration;
mod state;

pub(crate) use error::PersistenceError;
pub(crate) use filesystem::{config_directory, write_private_file};
use filesystem::{file_name, home_directory, permissions, read_optional, rollback_file};
use helpers::{
    AIDER_BLOCK_BEGIN, AIDER_BLOCK_END, AIDER_METADATA_RELATIVE_PATH, AIDER_SETTINGS_RELATIVE_PATH,
    DEEPSEEK_BLOCK_BEGIN, DEEPSEEK_BLOCK_END, LEGACY_PI_EXTENSION_RELATIVE_PATH,
    ManagedBlockFormat, OPENCODE_CONFIG_DIRECTORY, OPENCODE_JSON, OPENCODE_JSONC,
    PI_EXTENSION_RELATIVE_PATH, PRIME_EXTENSION_RELATIVE_PATH, PreparedFileChange,
    empty_jsonc_object_is_disposable, hash_input_value, hash_json_value, opencode_provider,
    parse_jsonc, parse_named_jsonc, sha256, validate_opencode_file_name,
};
use jsonc_parser::cst::CstObject;
use managed::{
    apply_prepared_file_change, ensure_qwen_auth_selection, ensure_qwen_list_directory,
    ensure_qwen_model_selection, inspect_managed_block, inspect_managed_json_entries,
    inspect_managed_json_property, inspect_qwen_auth_selection, inspect_qwen_list_directory,
    inspect_qwen_model_selection, optional_utf8, prepare_json_entries,
    prepare_json_entries_removal, prepare_managed_block, prepare_managed_block_removal,
    remove_qwen_auth_selection, remove_qwen_list_directory, remove_qwen_model_selection,
    rollback_prepared_file_change,
};
pub(crate) use models::discover_models;
use models::{
    aider_model_metadata, aider_model_settings, deepseek_provider_settings, qwen_code_provider,
};
pub(crate) use orchestration::{
    IntegrationChange, PersistenceManager, PersistentIntegration, RemovalOutcome,
};
pub(crate) use state::LastSelection;
use state::{
    IntegrationState, ManagedAider, ManagedBlock, ManagedFile, ManagedJsonEntries,
    ManagedJsonProperty, ManagedOpenCode, ManagedOpenCodeModel, ManagedOpenCodeSearch,
    ManagedQwenAuthSelection, ManagedQwenCode, ManagedQwenListDirectory, ManagedQwenModelSelection,
};

#[cfg(test)]
mod tests;
