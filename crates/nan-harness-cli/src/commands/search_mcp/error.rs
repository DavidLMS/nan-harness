use nan_harness_runtime::search_docker::DockerSearchError;
use nan_harness_runtime::{SearchConfigStoreError, SearchSupervisorError};
use std::process::ExitCode;
use thiserror::Error;

#[derive(Debug, Error)]
pub(super) enum SearchMcpError {
    #[error("invalid arguments")]
    InvalidArguments,
    #[error("invalid endpoint: {0}")]
    InvalidEndpoint(url::ParseError),
    #[error("unsafe endpoint")]
    UnsafeEndpoint,
    #[error("could not load SearXNG configuration: {0}")]
    LoadConfig(SearchConfigStoreError),
    #[error("could not build SearXNG client")]
    BuildSearchClient,
    #[error("could not acquire the SearXNG search session: {0}")]
    SearchLifecycle(#[source] SearchSupervisorError),
    #[error("could not verify managed Docker search state: {0}")]
    DockerLifecycle(#[source] DockerSearchError),
    #[error("could not read stdin: {0}")]
    ReadStdin(std::io::Error),
    #[error("message too large")]
    MessageTooLarge,
    #[error("could not serialize response: {0}")]
    SerializeResponse(serde_json::Error),
    #[error("could not write stdout: {0}")]
    WriteStdout(std::io::Error),
}

impl SearchMcpError {
    const fn code(&self) -> &'static str {
        match self {
            Self::InvalidArguments | Self::InvalidEndpoint(_) | Self::UnsafeEndpoint => {
                "NH-SEARCH-MCP-001"
            }
            Self::LoadConfig(_) | Self::BuildSearchClient => "NH-SEARCH-MCP-003",
            Self::SearchLifecycle(_) | Self::DockerLifecycle(_) => "NH-SEARCH-MCP-006",
            Self::ReadStdin(_) | Self::MessageTooLarge => "NH-SEARCH-MCP-010",
            Self::SerializeResponse(_) | Self::WriteStdout(_) => "NH-SEARCH-MCP-011",
        }
    }
}

pub(super) fn fail(error: &SearchMcpError) -> ExitCode {
    eprintln!("{}", error.code());
    ExitCode::FAILURE
}
