use std::process::Command;

#[test]
#[ignore = "requires Claude Code, network access, and NAN_API_KEY"]
fn claude_code_completes_a_real_read_tool_round_trip() {
    assert_read_tool_round_trip("qwen3.6");
}

#[test]
#[ignore = "requires Claude Code, network access, and NAN_API_KEY"]
fn deepseek_completes_a_real_read_tool_round_trip() {
    assert_read_tool_round_trip("deepseek-v4-flash");
}

#[test]
#[ignore = "requires Claude Code, network access, and NAN_API_KEY"]
fn mimo_completes_a_real_read_tool_round_trip() {
    assert_read_tool_round_trip("mimo-v2.5");
}

#[test]
#[ignore = "requires Claude Code, network access, and NAN_API_KEY"]
fn gemma_completes_a_real_read_tool_round_trip() {
    assert_read_tool_round_trip("gemma4");
}

fn assert_read_tool_round_trip(model: &str) {
    let output = run_claude(&[
        "claude",
        "--model",
        model,
        "--",
        "-p",
        concat!(
            "Use the Read tool to read Cargo.toml. Do not answer before using the tool. ",
            "After it succeeds, reply exactly READ_TOOL_OK."
        ),
        "--output-format",
        "stream-json",
        "--verbose",
        "--no-session-persistence",
        "--max-turns",
        "3",
        "--tools",
        "Read",
        "--allowedTools",
        "Read",
    ]);
    let stdout = String::from_utf8(output.stdout).expect("Claude output should be UTF-8");
    let stderr = String::from_utf8(output.stderr).expect("Claude errors should be UTF-8");

    assert!(output.status.success(), "{stderr}");
    assert!(stdout.contains("\"name\":\"Read\""), "{stdout}");
    assert!(stdout.contains("\"type\":\"tool_use\""), "{stdout}");
    assert!(stdout.contains("\"type\":\"tool_result\""), "{stdout}");
    assert!(stdout.contains("READ_TOOL_OK"), "{stdout}");
    assert!(stdout.contains("\"is_error\":false"), "{stdout}");
    assert!(!stdout.contains("\"is_error\":true"), "{stdout}");
}

#[test]
#[ignore = "requires Claude Code, network access, and NAN_API_KEY"]
fn claude_code_completes_a_real_web_search_round_trip() {
    let output = run_claude(&[
        "claude",
        "--",
        "-p",
        concat!(
            "Use WebSearch to search for the best Rust async runtime. ",
            "Do not answer before using the tool. After it succeeds, ",
            "reply with WEB_SEARCH_OK and include one result URL."
        ),
        "--output-format",
        "stream-json",
        "--verbose",
        "--no-session-persistence",
        "--max-turns",
        "5",
        "--tools",
        "WebSearch",
        "--allowedTools",
        "WebSearch",
    ]);
    let stdout = String::from_utf8(output.stdout).expect("Claude output should be UTF-8");
    let stderr = String::from_utf8(output.stderr).expect("Claude errors should be UTF-8");

    assert!(output.status.success(), "{stderr}\n{stdout}");
    assert!(stdout.contains("\"name\":\"WebSearch\""), "{stdout}");
    assert!(stdout.contains("\"type\":\"tool_result\""), "{stdout}");
    assert!(stdout.contains("WEB_SEARCH_OK"), "{stdout}");
    assert!(!stdout.contains("NH-BRIDGE-102"), "{stdout}");
    assert!(stdout.contains("\"is_error\":false"), "{stdout}");
    assert!(!stdout.contains("\"is_error\":true"), "{stdout}");
}

#[test]
#[ignore = "requires Claude Code, network access, and NAN_API_KEY"]
fn claude_code_plan_mode_remains_read_only() {
    let workspace = tempfile::tempdir().expect("temporary workspace should exist");
    let probe = workspace.path().join("plan-mode-probe.txt");
    let prompt = format!(
        "Attempt to use the Write tool to create '{}' containing SHOULD_NOT_EXIST. You are in Plan mode. Do not use Bash. After the attempt, reply exactly PLAN_MODE_OK.",
        probe.display()
    );
    let output = run_claude_in(
        workspace.path(),
        &[
            "claude",
            "--",
            "-p",
            &prompt,
            "--permission-mode",
            "plan",
            "--output-format",
            "stream-json",
            "--verbose",
            "--no-session-persistence",
            "--max-turns",
            "3",
            "--tools",
            "Write",
            "--allowedTools",
            "Write",
        ],
    );
    let stdout = String::from_utf8(output.stdout).expect("Claude output should be UTF-8");
    let stderr = String::from_utf8(output.stderr).expect("Claude errors should be UTF-8");

    assert!(output.status.success(), "{stderr}\n{stdout}");
    assert!(stdout.contains("PLAN_MODE_OK"), "{stdout}");
    assert!(!probe.exists(), "Plan mode unexpectedly created {probe:?}");
    assert!(
        !stderr.contains("Permission mode forced to default"),
        "{stderr}"
    );
}

#[test]
#[ignore = "requires Claude Code, network access, and NAN_API_KEY"]
fn claude_code_accept_edits_mode_writes_without_a_prompt() {
    let workspace = tempfile::tempdir().expect("temporary workspace should exist");
    let probe = workspace.path().join("accept-edits-probe.txt");
    let prompt = format!(
        "Use the Write tool to create '{}' containing exactly ACCEPT_EDITS_FILE_OK. Do not use Bash. After the write succeeds, reply exactly ACCEPT_EDITS_MODE_OK.",
        probe.display()
    );
    let output = run_claude_in(
        workspace.path(),
        &[
            "claude",
            "--",
            "-p",
            &prompt,
            "--permission-mode",
            "acceptEdits",
            "--output-format",
            "stream-json",
            "--verbose",
            "--no-session-persistence",
            "--max-turns",
            "3",
            "--tools",
            "Write",
        ],
    );
    let stdout = String::from_utf8(output.stdout).expect("Claude output should be UTF-8");
    let stderr = String::from_utf8(output.stderr).expect("Claude errors should be UTF-8");

    assert!(output.status.success(), "{stderr}\n{stdout}");
    assert!(stdout.contains("\"name\":\"Write\""), "{stdout}");
    assert!(stdout.contains("ACCEPT_EDITS_MODE_OK"), "{stdout}");
    assert!(probe.exists(), "{stderr}\n{stdout}");
    assert_eq!(
        std::fs::read_to_string(&probe).expect("Accept Edits should create the probe"),
        "ACCEPT_EDITS_FILE_OK"
    );
    assert!(
        !stderr.contains("Permission mode forced to default"),
        "{stderr}"
    );
}

#[test]
#[ignore = "requires Claude Code, network access, and NAN_API_KEY"]
fn claude_code_native_auto_mode_writes_after_qwen_review() {
    let workspace = tempfile::tempdir().expect("temporary workspace should exist");
    let probe = workspace.path().join("auto-mode-probe.txt");
    let prompt = format!(
        "Use the Write tool to create '{}' containing exactly NATIVE_AUTO_FILE_OK. Do not use Bash and do not read the file back. After the write succeeds, reply exactly NATIVE_AUTO_MODE_OK.",
        probe.display()
    );
    let output = run_claude_in(
        workspace.path(),
        &[
            "claude",
            "--",
            "-p",
            &prompt,
            "--permission-mode",
            "auto",
            "--output-format",
            "stream-json",
            "--verbose",
            "--no-session-persistence",
            "--max-turns",
            "3",
            "--tools",
            "Write,Read",
        ],
    );
    let stdout = String::from_utf8(output.stdout).expect("Claude output should be UTF-8");
    let stderr = String::from_utf8(output.stderr).expect("Claude errors should be UTF-8");

    assert!(output.status.success(), "{stderr}\n{stdout}");
    assert!(stdout.contains("\"permissionMode\":\"auto\""), "{stdout}");
    assert!(stdout.contains("\"name\":\"Write\""), "{stdout}");
    assert!(stdout.contains("NATIVE_AUTO_MODE_OK"), "{stdout}");
    assert!(stdout.contains("anthropic/nan/qwen3.6"), "{stdout}");
    assert_eq!(
        std::fs::read_to_string(&probe).expect("Auto mode should create the probe"),
        "NATIVE_AUTO_FILE_OK"
    );
    assert!(!stdout.contains("\"is_error\":true"), "{stdout}");
}

#[test]
#[ignore = "requires Claude Code, network access, and NAN_API_KEY"]
fn claude_code_subagent_inherits_the_nan_model_and_completes_a_tool_cycle() {
    let output = run_claude(&[
        "claude",
        "--",
        "-p",
        concat!(
            "Use the Agent tool exactly once with subagent_type Explore and no explicit model. ",
            "Ask it to use Read on Cargo.toml and report the first workspace member. ",
            "After the subagent succeeds, reply exactly SUBAGENT_OK."
        ),
        "--output-format",
        "stream-json",
        "--verbose",
        "--no-session-persistence",
        "--max-turns",
        "6",
        "--tools",
        "Agent,Read,Bash,Glob,Grep",
        "--allowedTools",
        "Agent,Read,Bash,Glob,Grep",
    ]);
    let stdout = String::from_utf8(output.stdout).expect("Claude output should be UTF-8");
    let stderr = String::from_utf8(output.stderr).expect("Claude errors should be UTF-8");

    assert!(output.status.success(), "{stderr}\n{stdout}");
    assert!(stdout.contains("\"name\":\"Agent\""), "{stdout}");
    assert!(stdout.contains("\"subagent_type\":\"Explore\""), "{stdout}");
    assert!(stdout.contains("\"name\":\"Read\""), "{stdout}");
    assert!(
        stdout.contains("\"resolvedModel\":\"anthropic/nan/qwen3.6\""),
        "{stdout}"
    );
    assert!(stdout.contains("SUBAGENT_OK"), "{stdout}");
    assert!(stdout.contains("\"is_error\":false"), "{stdout}");
    assert!(!stdout.contains("\"is_error\":true"), "{stdout}");
}

#[test]
#[ignore = "requires Claude Code, network access, and NAN_API_KEY"]
fn claude_code_continues_and_resumes_local_sessions_through_nan() {
    let workspace = tempfile::tempdir().expect("temporary workspace should exist");
    let config = tempfile::tempdir().expect("temporary Claude config should exist");
    let marker = "NAN_LOCAL_SESSION_6F2C";
    let initial_prompt =
        format!("Remember the exact marker {marker}. Reply exactly SESSION_CREATED.");
    let initial = run_claude_in_with_config(
        workspace.path(),
        config.path(),
        &[
            "claude",
            "--model",
            "qwen3.6",
            "--",
            "-p",
            &initial_prompt,
            "--output-format",
            "json",
            "--max-turns",
            "1",
            "--tools",
            "",
        ],
    );
    let initial_json = successful_json(initial);
    let session_id = initial_json["session_id"]
        .as_str()
        .expect("initial response should include a session ID")
        .to_owned();

    let continued = run_claude_in_with_config(
        workspace.path(),
        config.path(),
        &[
            "claude",
            "--model",
            "deepseek-v4-flash",
            "--",
            "--continue",
            "-p",
            "Reply exactly CONTINUE_OK followed by the marker I asked you to remember.",
            "--output-format",
            "json",
            "--max-turns",
            "1",
            "--tools",
            "",
        ],
    );
    let continued_json = successful_json(continued);
    let continued_result = continued_json["result"]
        .as_str()
        .expect("continued response should contain text");
    assert!(continued_result.contains("CONTINUE_OK"), "{continued_json}");
    assert!(continued_result.contains(marker), "{continued_json}");
    assert!(
        continued_json["modelUsage"]
            .get("anthropic/nan/deepseek-v4-flash")
            .is_some(),
        "{continued_json}"
    );

    let resumed = run_claude_in_with_config(
        workspace.path(),
        config.path(),
        &[
            "claude",
            "--model",
            "qwen3.6",
            "--",
            "--resume",
            &session_id,
            "-p",
            "Reply exactly RESUME_OK followed by the marker from this conversation.",
            "--output-format",
            "json",
            "--max-turns",
            "1",
            "--tools",
            "",
        ],
    );
    let resumed_json = successful_json(resumed);
    let resumed_result = resumed_json["result"]
        .as_str()
        .expect("resumed response should contain text");
    assert!(resumed_result.contains("RESUME_OK"), "{resumed_json}");
    assert!(resumed_result.contains(marker), "{resumed_json}");
    assert_eq!(
        resumed_json["session_id"].as_str(),
        Some(session_id.as_str())
    );
    assert!(
        resumed_json["modelUsage"]
            .get("anthropic/nan/qwen3.6")
            .is_some(),
        "{resumed_json}"
    );
}

fn run_claude(arguments: &[&str]) -> std::process::Output {
    run_claude_in(workspace_root(), arguments)
}

fn run_claude_in(current_directory: &std::path::Path, arguments: &[&str]) -> std::process::Output {
    let config = tempfile::tempdir().expect("temporary Claude config should exist");
    run_claude_in_with_config(current_directory, config.path(), arguments)
}

fn run_claude_in_with_config(
    current_directory: &std::path::Path,
    config_directory: &std::path::Path,
    arguments: &[&str],
) -> std::process::Output {
    assert!(
        std::env::var_os("NAN_API_KEY").is_some(),
        "NAN_API_KEY must be set for the live test"
    );
    Command::new(env!("CARGO_BIN_EXE_nan-harness"))
        .current_dir(current_directory)
        .env("CLAUDE_CONFIG_DIR", config_directory)
        .args(arguments)
        .output()
        .expect("nan-harness should start")
}

fn successful_json(output: std::process::Output) -> serde_json::Value {
    let stdout = String::from_utf8(output.stdout).expect("Claude output should be UTF-8");
    let stderr = String::from_utf8(output.stderr).expect("Claude errors should be UTF-8");

    assert!(output.status.success(), "{stderr}\n{stdout}");
    serde_json::from_str(&stdout).unwrap_or_else(|error| {
        panic!("Claude output should be one JSON object: {error}\n{stderr}\n{stdout}")
    })
}

fn workspace_root() -> &'static std::path::Path {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root should exist")
}
