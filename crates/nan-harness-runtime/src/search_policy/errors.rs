use nan_harness_core::HarnessKind;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SearchPolicyError {
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
