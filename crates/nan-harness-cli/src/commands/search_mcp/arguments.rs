use super::error::SearchMcpError;
use super::transport::{provider_search_endpoint, validate_endpoint};
use reqwest::Url;
use std::ffi::OsString;

#[derive(Debug)]
pub(super) struct Arguments {
    pub(super) endpoint: Url,
    pub(super) token_environment: String,
}

impl Arguments {
    pub(super) fn parse(values: impl Iterator<Item = OsString>) -> Result<Self, SearchMcpError> {
        let mut endpoint = None;
        let mut provider_base_url = None;
        let mut token_environment = None;
        let mut values = values;
        while let Some(option) = values.next() {
            let option = option
                .into_string()
                .map_err(|_| SearchMcpError::InvalidArguments)?;
            let value = values
                .next()
                .ok_or(SearchMcpError::InvalidArguments)?
                .into_string()
                .map_err(|_| SearchMcpError::InvalidArguments)?;
            match option.as_str() {
                "--endpoint" if endpoint.is_none() => {
                    endpoint = Some(Url::parse(&value).map_err(SearchMcpError::InvalidEndpoint)?);
                }
                "--provider-base-url" if provider_base_url.is_none() => {
                    provider_base_url =
                        Some(Url::parse(&value).map_err(SearchMcpError::InvalidEndpoint)?);
                }
                "--token-env" if token_environment.is_none() => token_environment = Some(value),
                _ => return Err(SearchMcpError::InvalidArguments),
            }
        }
        let endpoint = match (endpoint, provider_base_url) {
            (Some(endpoint), None) => {
                validate_endpoint(&endpoint)?;
                endpoint
            }
            (None, Some(provider_base_url)) => provider_search_endpoint(provider_base_url)?,
            _ => return Err(SearchMcpError::InvalidArguments),
        };
        let token_environment = token_environment.ok_or(SearchMcpError::InvalidArguments)?;
        if !valid_environment_name(&token_environment) {
            return Err(SearchMcpError::InvalidArguments);
        }
        Ok(Self {
            endpoint,
            token_environment,
        })
    }
}

fn valid_environment_name(value: &str) -> bool {
    let mut characters = value.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_uppercase())
        && characters.all(|character| {
            character == '_' || character.is_ascii_uppercase() || character.is_ascii_digit()
        })
}

#[cfg(test)]
mod tests {
    use super::Arguments;
    use crate::commands::search_mcp::error::SearchMcpError;
    use std::ffi::OsString;

    #[test]
    fn argument_parser_accepts_local_and_persistent_modes() {
        let local = [
            "--endpoint",
            "http://127.0.0.1:4312/v1/search",
            "--token-env",
            "NAN_API_KEY",
        ]
        .map(OsString::from);
        let local = Arguments::parse(local.into_iter()).expect("local arguments should parse");
        assert_eq!(local.endpoint.as_str(), "http://127.0.0.1:4312/v1/search");

        let persistent = [
            "--provider-base-url",
            "https://api.nan.builders/v1",
            "--token-env",
            "NAN_API_KEY",
        ]
        .map(OsString::from);
        let persistent =
            Arguments::parse(persistent.into_iter()).expect("persistent arguments should parse");
        assert_eq!(
            persistent.endpoint.as_str(),
            "https://api.nan.builders/v1/search"
        );
    }

    #[test]
    fn argument_parser_rejects_unknown_duplicate_or_conflicting_options() {
        let missing_token = ["--endpoint", "http://127.0.0.1:4312/v1/search"].map(OsString::from);
        assert!(matches!(
            Arguments::parse(missing_token.into_iter()),
            Err(SearchMcpError::InvalidArguments)
        ));

        let conflicting = [
            "--endpoint",
            "http://127.0.0.1:4312/v1/search",
            "--provider-base-url",
            "https://api.nan.builders/v1",
            "--token-env",
            "NAN_API_KEY",
        ]
        .map(OsString::from);
        assert!(matches!(
            Arguments::parse(conflicting.into_iter()),
            Err(SearchMcpError::InvalidArguments)
        ));

        for options in [
            vec![
                "--endpoint",
                "http://127.0.0.1:4312/v1/search",
                "--endpoint",
                "http://localhost:4312/v1/search",
                "--token-env",
                "NAN_API_KEY",
            ],
            vec![
                "--endpoint",
                "http://127.0.0.1:4312/v1/search",
                "--unknown",
                "value",
                "--token-env",
                "NAN_API_KEY",
            ],
        ] {
            assert!(matches!(
                Arguments::parse(options.into_iter().map(OsString::from)),
                Err(SearchMcpError::InvalidArguments)
            ));
        }
    }

    #[test]
    fn argument_parser_rejects_unsafe_environment_names() {
        for name in ["", "lowercase", "1TOKEN", "NAN-TOKEN", "NAN TOKEN"] {
            let arguments = [
                "--endpoint",
                "http://127.0.0.1:4312/v1/search",
                "--token-env",
                name,
            ]
            .map(OsString::from);
            assert!(matches!(
                Arguments::parse(arguments.into_iter()),
                Err(SearchMcpError::InvalidArguments)
            ));
        }
    }
}
