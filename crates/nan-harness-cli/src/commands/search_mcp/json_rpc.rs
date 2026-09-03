use super::arguments::Arguments;
use super::error::{SearchMcpError, fail};
use super::transport::SearchTransport;
use serde::Deserialize;
use serde_json::{Value, json};
use std::io::{BufRead as _, Read as _};
use std::process::ExitCode;

const DEFAULT_PROTOCOL_VERSION: &str = "2025-06-18";
const MAX_MESSAGE_BYTES: usize = 1024 * 1024;

pub(super) struct SearchMcp {
    transport: SearchTransport,
}

impl SearchMcp {
    pub(super) fn new(arguments: Arguments) -> Result<Self, SearchMcpError> {
        let transport = SearchTransport::new(arguments.endpoint, arguments.token_environment)?;
        Ok(Self { transport })
    }

    pub(super) async fn run(self) -> ExitCode {
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
            let bytes = read_message(&mut input, &mut buffer)?;
            if bytes == 0 {
                return Ok(());
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
        let body = self.transport.search(&body).await?;
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

fn read_message(
    input: &mut impl std::io::BufRead,
    buffer: &mut Vec<u8>,
) -> Result<usize, SearchMcpError> {
    buffer.clear();
    let bytes = input
        .take((MAX_MESSAGE_BYTES + 1) as u64)
        .read_until(b'\n', buffer)
        .map_err(SearchMcpError::ReadStdin)?;
    if buffer.len() > MAX_MESSAGE_BYTES {
        return Err(SearchMcpError::MessageTooLarge);
    }
    Ok(bytes)
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

#[cfg(test)]
mod tests {
    use super::{MAX_MESSAGE_BYTES, read_message};
    use crate::commands::search_mcp::error::SearchMcpError;
    use std::io::Cursor;

    #[test]
    fn message_limit_stops_before_buffering_an_unbounded_line() {
        let mut input = Cursor::new(vec![b'x'; MAX_MESSAGE_BYTES + 128]);
        let mut buffer = Vec::new();

        assert!(matches!(
            read_message(&mut input, &mut buffer),
            Err(SearchMcpError::MessageTooLarge)
        ));
        assert_eq!(buffer.len(), MAX_MESSAGE_BYTES + 1);
    }

    #[test]
    fn message_limit_accepts_the_exact_boundary() {
        let mut input = Cursor::new(vec![b'x'; MAX_MESSAGE_BYTES]);
        let mut buffer = Vec::new();

        assert_eq!(
            read_message(&mut input, &mut buffer).expect("bounded message"),
            MAX_MESSAGE_BYTES
        );
        assert_eq!(buffer.len(), MAX_MESSAGE_BYTES);
    }
}
