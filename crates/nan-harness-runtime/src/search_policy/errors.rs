use nan_harness_core::HarnessKind;
use nan_harness_search::SearchConfigStoreError;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SearchPolicyError {
    #[error("could not load persisted SearXNG configuration: {0}")]
    LoadConfiguration(#[from] SearchConfigStoreError),
    #[error(
        "could not determine the current user's home directory while checking web search configuration"
    )]
    MissingHomeDirectory,
    #[error("{0} does not support the NaN web search fallback")]
    UnsupportedHarness(HarnessKind),
    #[error(
        "NaN web search requires the local Chat Completions gateway; remove --no-chat-gateway or omit --force-search"
    )]
    RequiresDirectGateway,
    #[error(
        "configuration '{}' already defines 'nan-search', but that entry is not managed by nan-harness; rename or remove it, or use --no-search",
        .0.display()
    )]
    McpNameCollision(PathBuf),
    #[error("could not read web search configuration '{}': {source}", path.display())]
    ReadConfiguration {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("web search configuration '{}' exceeds the 2 MiB inspection limit", .0.display())]
    ConfigurationTooLarge(PathBuf),
    #[error("web search configuration '{}' is not valid JSON or JSONC: {source}", path.display())]
    ParseJson {
        path: PathBuf,
        #[source]
        source: jsonc_parser::errors::ParseError,
    },
    #[error("web search configuration '{}' is not valid TOML: {source}", path.display())]
    ParseToml {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("could not inspect TOML web search configuration '{}': {source}", path.display())]
    ConvertToml {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

// Terminal localization is separate from canonical Display used by machine contracts.
impl nan_harness_i18n::TerminalMessage for SearchPolicyError {
    fn terminal_message(&self, locale: nan_harness_i18n::Locale) -> String {
        use nan_harness_i18n::messages as m;
        if locale == nan_harness_i18n::Locale::En {
            return self.to_string();
        }
        match self {
            Self::LoadConfiguration(field_0) => m::error_search_policy_load_configuration(
                locale,
                &(nan_harness_i18n::TerminalMessage::terminal_message(field_0, locale)),
            ),
            Self::MissingHomeDirectory => m::error_search_policy_missing_home_directory(locale),
            Self::UnsupportedHarness(field_0) => {
                m::error_search_policy_unsupported_harness(locale, &(field_0))
            }
            Self::RequiresDirectGateway => m::error_search_policy_requires_direct_gateway(locale),
            Self::McpNameCollision(field_0) => {
                m::error_search_policy_mcp_name_collision(locale, &(field_0.display()))
            }
            Self::ReadConfiguration { path, source } => {
                m::error_search_policy_read_configuration(locale, &(source), &(path.display()))
            }
            Self::ConfigurationTooLarge(field_0) => {
                m::error_search_policy_configuration_too_large(locale, &(field_0.display()))
            }
            Self::ParseJson { path, source } => {
                m::error_search_policy_parse_json(locale, &(source), &(path.display()))
            }
            Self::ParseToml { path, source } => {
                m::error_search_policy_parse_toml(locale, &(source), &(path.display()))
            }
            Self::ConvertToml { path, source } => {
                m::error_search_policy_convert_toml(locale, &(source), &(path.display()))
            }
        }
    }
}
