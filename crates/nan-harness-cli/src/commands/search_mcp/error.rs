use nan_harness_core::SecretError;
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
    #[error("missing token environment: {0}")]
    MissingToken(String),
    #[error("invalid token: {0}")]
    InvalidToken(SecretError),
    #[error("could not build client: {0}")]
    BuildClient(reqwest::Error),
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
            Self::MissingToken(_) | Self::InvalidToken(_) => "NH-SEARCH-MCP-002",
            Self::BuildClient(_) => "NH-SEARCH-MCP-003",
            Self::ReadStdin(_) | Self::MessageTooLarge => "NH-SEARCH-MCP-010",
            Self::SerializeResponse(_) | Self::WriteStdout(_) => "NH-SEARCH-MCP-011",
        }
    }
}

pub(super) fn fail(error: &SearchMcpError) -> ExitCode {
    eprintln!("{}", error.code());
    ExitCode::FAILURE
}
