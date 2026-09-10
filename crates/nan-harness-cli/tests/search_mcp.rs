use serde_json::{Value, json};
use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use tempfile::tempdir;

#[test]
fn search_mcp_stays_off_network_until_a_tool_call() {
    let config_directory = tempdir().expect("search config directory should exist");
    let listener = TcpListener::bind("127.0.0.1:0").expect("test listener should bind");
    listener
        .set_nonblocking(true)
        .expect("listener should become nonblocking");
    let base_url = format!("http://{}", listener.local_addr().expect("address"));
    let endpoint = format!("{base_url}/v1/search");
    let search_config = nan_harness_runtime::SearxngConfig::local(&base_url)
        .expect("test endpoint should validate");
    nan_harness_runtime::save_search_config(
        config_directory.path().join("search.json"),
        &search_config,
    )
    .expect("search config should save");
    let mut child = Command::new(env!("CARGO_BIN_EXE_nan-harness"))
        .args([
            "__search-mcp",
            "--endpoint",
            &endpoint,
            "--token-env",
            "NAN_TEST_SESSION_TOKEN",
        ])
        .env("NAN_TEST_SESSION_TOKEN", "local-session-token")
        .env("NAN_HARNESS_CONFIG_DIR", config_directory.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("MCP process should start");
    let mut input = child.stdin.take().expect("child stdin");
    let mut output = std::io::BufReader::new(child.stdout.take().expect("child stdout"));

    send(
        &mut input,
        &json!({
            "jsonrpc":"2.0",
            "id":1,
            "method":"initialize",
            "params":{"protocolVersion":"2025-06-18"}
        }),
    );
    let initialized = receive(&mut output);
    assert_eq!(initialized["id"], 1);
    assert_eq!(initialized["result"]["serverInfo"]["name"], "nan-search");
    assert_no_connection(&listener);

    send(
        &mut input,
        &json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
    );
    let tools = receive(&mut output);
    assert_eq!(tools["result"]["tools"][0]["name"], "web_search");
    assert_no_connection(&listener);

    listener
        .set_nonblocking(false)
        .expect("listener should become blocking");
    let server = std::thread::spawn(move || serve_search(&listener));
    send(
        &mut input,
        &json!({
            "jsonrpc":"2.0",
            "id":3,
            "method":"tools/call",
            "params":{"name":"web_search","arguments":{"query":"rust async","max_results":1}}
        }),
    );
    let result = receive(&mut output);
    assert_eq!(result["id"], 3);
    assert_eq!(
        result["result"]["structuredContent"]["results"][0]["title"],
        "Tokio"
    );
    assert!(result["result"].get("isError").is_none());
    server.join().expect("search server should finish");

    drop(input);
    let completed = child.wait_with_output().expect("MCP process should finish");
    assert!(completed.status.success());
    assert!(completed.stderr.is_empty());
}

fn send(input: &mut impl std::io::Write, value: &Value) {
    serde_json::to_writer(&mut *input, &value).expect("request should serialize");
    input.write_all(b"\n").expect("request should terminate");
    input.flush().expect("request should flush");
}

fn receive(output: &mut impl std::io::BufRead) -> Value {
    let mut line = String::new();
    output.read_line(&mut line).expect("response should read");
    assert!(!line.is_empty(), "MCP process ended before responding");
    serde_json::from_str(&line).expect("response should be JSON")
}

fn assert_no_connection(listener: &TcpListener) {
    let error = listener
        .accept()
        .expect_err("MCP must not connect before tools/call");
    assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
}

fn serve_search(listener: &TcpListener) {
    let (mut stream, _) = listener.accept().expect("search request should connect");
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let count = stream.read(&mut chunk).expect("request should read");
        assert!(count > 0, "request ended before its headers");
        buffer.extend_from_slice(&chunk[..count]);
        let Some(header_end) = find_bytes(&buffer, b"\r\n\r\n") else {
            continue;
        };
        let headers = String::from_utf8_lossy(&buffer[..header_end]);
        let request_line = headers
            .lines()
            .next()
            .expect("request line should be present");
        assert!(request_line.starts_with("GET /search?"), "{request_line}");
        let lowercase_headers = headers.to_ascii_lowercase();
        assert!(
            !lowercase_headers.contains("authorization:"),
            "SearXNG must not receive NaN credentials: {headers}"
        );
        let body_start = header_end + 4;
        assert_eq!(buffer.len(), body_start, "GET search must not have a body");
        assert!(request_line.contains("q=rust+async"), "{request_line}");
        assert!(request_line.contains("format=json"), "{request_line}");
        assert!(
            request_line.contains("number_of_results=1"),
            "{request_line}"
        );
        break;
    }

    let body = json!({
        "results":[{"title":"Tokio","url":"https://tokio.rs","snippet":"Async runtime"}],
        "summary":"1. Tokio\nURL: https://tokio.rs\nAsync runtime"
    })
    .to_string();
    write!(
        stream,
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
    .expect("response should write");
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
