use nan_harness_core::{SecretError, SecretValue};
use reqwest::Url;
use serde::Deserialize;
use serde_json::{Value, json};
use std::ffi::OsString;
use std::io::BufRead as _;
use std::net::IpAddr;
use std::process::ExitCode;
use std::time::Duration;
use thiserror::Error;

const SUBCOMMAND: &str = "__search-mcp";
const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const DEFAULT_PROTOCOL_VERSION: &str = "2025-06-18";

pub(crate) async fn run_if_requested() -> Option<ExitCode> {
    let mut values = std::env::args_os();
    let _executable = values.next();
    if values.next().as_deref() != Some(std::ffi::OsStr::new(SUBCOMMAND)) {
        return None;
    }
    Some(match Arguments::parse(values) {
        Ok(arguments) => match SearchMcp::new(arguments) {
            Ok(server) => server.run().await,
            Err(error) => fail(&error),
        },
        Err(error) => fail(&error),
    })
}

#[derive(Debug)]
struct Arguments {
    endpoint: Url,
    token_environment: String,
}

impl Arguments {
    fn parse(values: impl Iterator<Item = OsString>) -> Result<Self, SearchMcpError> {
        let mut endpoint = None;
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
                "--token-env" if token_environment.is_none() => token_environment = Some(value),
                _ => return Err(SearchMcpError::InvalidArguments),
            }
        }
        let endpoint = endpoint.ok_or(SearchMcpError::InvalidArguments)?;
        validate_endpoint(&endpoint)?;
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

struct SearchMcp {
    endpoint: Url,
    token: SecretValue,
    client: reqwest::Client,
}

impl SearchMcp {
    fn new(arguments: Arguments) -> Result<Self, SearchMcpError> {
        let token = std::env::var(&arguments.token_environment)
            .map_err(|_| SearchMcpError::MissingToken(arguments.token_environment))?;
        let token = SecretValue::new(token).map_err(SearchMcpError::InvalidToken)?;
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_mins(1))
            .build()
            .map_err(SearchMcpError::BuildClient)?;
        Ok(Self {
            endpoint: arguments.endpoint,
            token,
            client,
        })
    }

    async fn run(self) -> ExitCode {
        match self.serve().await {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => fail(&error),
        }
    }

    async fn serve(&self) -> Result<(), SearchMcpError> {
        let input = std::io::stdin();
        let mut input = std::io::BufReader::new(input.lock());
        let output = std::io::stdout();
        let mut output = output.lock();
        let mut buffer = Vec::new();
        loop {
            buffer.clear();
            let bytes = input
                .read_until(b'\n', &mut buffer)
                .map_err(SearchMcpError::ReadStdin)?;
            if bytes == 0 {
                return Ok(());
            }
            if buffer.len() > MAX_MESSAGE_BYTES {
                return Err(SearchMcpError::MessageTooLarge);
            }
            let Ok(request) = serde_json::from_slice::<Value>(&buffer) else {
                write_response(&mut output, &rpc_error(&Value::Null, -32700, "Parse error"))?;
                continue;
            };
            if let Some(response) = self.handle(&request).await {
                write_response(&mut output, &response)?;
            }
        }
    }

    async fn handle(&self, request: &Value) -> Option<Value> {
        let id = request.get("id").cloned();
        let method = request.get("method").and_then(Value::as_str);
        if request.get("jsonrpc").and_then(Value::as_str) != Some("2.0") || method.is_none() {
            return id.map(|id| rpc_error(&id, -32600, "Invalid Request"));
        }
        let method = method.unwrap_or_default();
        let id = id?;
        let result = match method {
            "initialize" => Ok(initialize_result(request)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(tools_result()),
            "tools/call" => self.call_tool(request).await,
            _ => return Some(rpc_error(&id, -32601, "Method not found")),
        };
        Some(match result {
            Ok(result) => rpc_result(&id, &result),
            Err(code) => rpc_result(&id, &tool_error(code)),
        })
    }

    async fn call_tool(&self, request: &Value) -> Result<Value, &'static str> {
        let name = request.pointer("/params/name").and_then(Value::as_str);
        if name != Some("web_search") {
            return Err("NH-SEARCH-MCP-004");
        }
        let arguments = request
            .pointer("/params/arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let arguments: ToolArguments =
            serde_json::from_value(arguments).map_err(|_| "NH-SEARCH-MCP-005")?;
        if arguments.query.trim().is_empty() {
            return Err("NH-SEARCH-MCP-005");
        }
        let body = json!({
            "query": arguments.query,
            "maxResults": arguments.max_results,
            "allowedDomains": arguments.allowed_domains,
            "blockedDomains": arguments.blocked_domains
        });
        let request = self.token.with_secret(|token| {
            self.client
                .post(self.endpoint.clone())
                .bearer_auth(token)
                .json(&body)
        });
        let response = request.send().await.map_err(|_| "NH-SEARCH-MCP-006")?;
        if !response.status().is_success() {
            return Err("NH-SEARCH-MCP-007");
        }
        if response
            .content_length()
            .is_some_and(|size| size > MAX_RESPONSE_BYTES as u64)
        {
            return Err("NH-SEARCH-MCP-008");
        }
        let body = response.bytes().await.map_err(|_| "NH-SEARCH-MCP-006")?;
        if body.len() > MAX_RESPONSE_BYTES {
            return Err("NH-SEARCH-MCP-008");
        }
        let body: Value = serde_json::from_slice(&body).map_err(|_| "NH-SEARCH-MCP-009")?;
        let summary = body
            .get("summary")
            .and_then(Value::as_str)
            .ok_or("NH-SEARCH-MCP-009")?;
        let results = body.get("results").cloned().ok_or("NH-SEARCH-MCP-009")?;
        Ok(json!({
            "content": [{"type":"text","text":summary}],
            "structuredContent": {"results": results}
        }))
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolArguments {
    query: String,
    #[serde(default = "default_max_results")]
    max_results: usize,
    #[serde(default)]
    allowed_domains: Vec<String>,
    #[serde(default)]
    blocked_domains: Vec<String>,
}

const fn default_max_results() -> usize {
    10
}

fn initialize_result(request: &Value) -> Value {
    let protocol_version = request
        .pointer("/params/protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or(DEFAULT_PROTOCOL_VERSION);
    json!({
        "protocolVersion": protocol_version,
        "capabilities": {"tools": {"listChanged": false}},
        "serverInfo": {"name":"nan-search","version":env!("CARGO_PKG_VERSION")}
    })
}

fn tools_result() -> Value {
    json!({
        "tools": [{
            "name": "web_search",
            "inputSchema": {
                "type": "object",
                "additionalProperties": false,
                "required": ["query"],
                "properties": {
                    "query": {"type":"string"},
                    "max_results": {"type":"integer","minimum":1,"maximum":20},
                    "allowed_domains": {"type":"array","items":{"type":"string"}},
                    "blocked_domains": {"type":"array","items":{"type":"string"}}
                }
            },
            "annotations": {"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":true}
        }]
    })
}

fn tool_error(code: &str) -> Value {
    json!({"content":[{"type":"text","text":code}],"isError":true})
}

fn rpc_result(id: &Value, result: &Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":result})
}

fn rpc_error(id: &Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}

fn write_response(
    output: &mut impl std::io::Write,
    response: &Value,
) -> Result<(), SearchMcpError> {
    let mut payload = serde_json::to_vec(&response).map_err(SearchMcpError::SerializeResponse)?;
    payload.push(b'\n');
    output
        .write_all(&payload)
        .map_err(SearchMcpError::WriteStdout)?;
    output.flush().map_err(SearchMcpError::WriteStdout)
}

fn validate_endpoint(endpoint: &Url) -> Result<(), SearchMcpError> {
    if endpoint.scheme() != "http"
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
        || endpoint.path() != "/v1/search"
    {
        return Err(SearchMcpError::UnsafeEndpoint);
    }
    let loopback = endpoint.host().is_some_and(|host| match host {
        url::Host::Domain(domain) => domain.eq_ignore_ascii_case("localhost"),
        url::Host::Ipv4(address) => IpAddr::V4(address).is_loopback(),
        url::Host::Ipv6(address) => IpAddr::V6(address).is_loopback(),
    });
    if loopback {
        Ok(())
    } else {
        Err(SearchMcpError::UnsafeEndpoint)
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

fn fail(error: &SearchMcpError) -> ExitCode {
    eprintln!("{}", error.code());
    ExitCode::FAILURE
}

#[derive(Debug, Error)]
enum SearchMcpError {
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

#[cfg(test)]
mod tests {
    use super::{Arguments, SearchMcpError, validate_endpoint};
    use reqwest::Url;
    use std::ffi::OsString;

    #[test]
    fn accepts_only_an_authenticated_loopback_search_endpoint() {
        for endpoint in [
            "http://127.0.0.1:4312/v1/search",
            "http://[::1]:4312/v1/search",
            "http://localhost:4312/v1/search",
        ] {
            validate_endpoint(&Url::parse(endpoint).expect("URL")).expect("loopback endpoint");
        }
        for endpoint in [
            "https://127.0.0.1:4312/v1/search",
            "http://example.com/v1/search",
            "http://127.0.0.1:4312/v1/models",
            "http://127.0.0.1:4312/v1/search?target=other",
        ] {
            assert!(validate_endpoint(&Url::parse(endpoint).expect("URL")).is_err());
        }
    }

    #[test]
    fn argument_parser_rejects_unknown_or_duplicate_options() {
        let valid = [
            "--endpoint",
            "http://127.0.0.1:4312/v1/search",
            "--token-env",
            "NAN_API_KEY",
        ]
        .map(OsString::from);
        Arguments::parse(valid.into_iter()).expect("arguments should parse");

        let invalid = ["--endpoint", "http://127.0.0.1:4312/v1/search"].map(OsString::from);
        assert!(matches!(
            Arguments::parse(invalid.into_iter()),
            Err(SearchMcpError::InvalidArguments)
        ));
    }
}
