use super::constants::{ROUND_TRIP_MARKER, TEST_CREDENTIAL, WRAPPER_TIMEOUT};
use super::helpers::{call, duration_milliseconds, progress_event, tool_names};
use super::report::{ConformanceCheck, ConformanceStatus};
use crate::assertions::assert_tool_round_trip;
use crate::scripted_provider::{ProviderScenario, ScriptedProvider, ScriptedToolCall};
use crate::terminal::TerminalCommand;
use crate::workspace::ConformanceWorkspace;
use nan_harness_private_fs::{create_private_dir_all, open_private_truncate};
use serde_json::{Value, json};
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

const CHECK_NAME: &str = "native-configuration";
const ROTATED_CREDENTIAL: &str = "nan-harness-conformance-rotated-credential";

/// Checks the published binary's complete `MiMo` native configuration lifecycle.
///
/// Uses only disposable state, synthetic credentials and a local scripted provider. The native
/// process receives no managed `MiMo` overlay or API key environment variable.
pub async fn mimo_native_configuration_check(nan_harness: &Path) -> ConformanceCheck {
    let started = Instant::now();
    progress_event("tool-round-trip", CHECK_NAME, "started", started);
    let passed = verify(nan_harness).await.is_ok();
    let status = if passed {
        ConformanceStatus::Passed
    } else {
        ConformanceStatus::Failed
    };
    progress_event(
        "tool-round-trip",
        CHECK_NAME,
        if passed { "passed" } else { "failed" },
        started,
    );
    ConformanceCheck {
        name: CHECK_NAME.to_owned(),
        status,
        duration_milliseconds: duration_milliseconds(started.elapsed()),
    }
}

async fn verify(nan_harness: &Path) -> Result<(), ()> {
    let workspace = ConformanceWorkspace::create().map_err(|_| ())?;
    let binary = prepare(nan_harness, workspace.path()).map_err(|_| ())?;
    let discovery = discover_native_tools(&binary, workspace.path()).await?;
    let calls = native_calls(workspace.path(), discovery);
    let provider =
        ScriptedProvider::start(ProviderScenario::sequence(calls.clone(), ROUND_TRIP_MARKER))
            .await
            .map_err(|_| ())?;
    let result = lifecycle(&binary, workspace.path(), &provider, &calls).await;
    let bounded = provider.recording_bounded();
    let shutdown = provider.shutdown().await;
    result?;
    require(bounded && shutdown.is_ok())
}

async fn discover_native_tools(binary: &Path, root: &Path) -> Result<bool, ()> {
    let provider = ScriptedProvider::start(ProviderScenario::inventory(ROUND_TRIP_MARKER))
        .await
        .map_err(|_| ())?;
    let result = configure_and_inventory(binary, root, &provider).await;
    let shutdown = provider.shutdown().await;
    require(shutdown.is_ok())?;
    result
}

async fn configure_and_inventory(
    binary: &Path,
    root: &Path,
    provider: &ScriptedProvider,
) -> Result<bool, ()> {
    save_search_backend(root, provider.base_url()).map_err(|_| ())?;
    configure(
        binary,
        root,
        provider.base_url(),
        &["--yes", "--force-search"],
    )
    .await?;
    let output = native_command(root, provider.base_url())?
        .run()
        .await
        .map_err(|_| ())?;
    require(
        output.status.success()
            && output.stdout.contains(ROUND_TRIP_MARKER)
            && provider.completed()
            && provider.recording_bounded(),
    )?;
    let requests = provider.chat_requests();
    let tools = requests
        .iter()
        .filter_map(tool_names)
        .flatten()
        .collect::<std::collections::BTreeSet<_>>();
    if !tools.contains("nan-search_web_search") && !tools.contains("mcp_tool_search") {
        eprintln!("MiMo native MCP discovery unavailable: tools={tools:?}");
    }
    // MiMo 0.1.15 defers MCP schemas until its discovery tool is called.
    require(tools.contains("nan-search_web_search") || tools.contains("mcp_tool_search"))?;
    Ok(tools.contains("mcp_tool_search"))
}

fn prepare(nan_harness: &Path, root: &Path) -> std::io::Result<PathBuf> {
    // Keep native snapshots and file tools inside project fixtures, outside test state.
    fs::write(
        root.join(".gitignore"),
        "bin/\nhome/\nstate/\nmimo/\ntmp/\n",
    )?;
    for directory in ["bin", "home", "state", "mimo/config", "mimo/data"] {
        create_private_dir_all(&root.join(directory))?;
    }
    let binary = root.join("bin").join(if cfg!(windows) {
        "nan-harness.exe"
    } else {
        "nan-harness"
    });
    fs::copy(nan_harness, &binary)?;
    save_credential(root, TEST_CREDENTIAL)?;
    write_private(
        &root.join("state/credential.json"),
        r#"{"schemaVersion":1,"backend":"private-file"}"#,
    )?;
    write_private(
        &root.join("mimo/config/mimocode.json"),
        r#"{"theme":"system","model":"other/original"}"#,
    )?;
    write_private(
        &root.join("mimo/data/auth.json"),
        r#"{"other":{"type":"api","key":"synthetic-user-owned"}}"#,
    )?;
    Ok(binary)
}

async fn lifecycle(
    binary: &Path,
    root: &Path,
    provider: &ScriptedProvider,
    calls: &[ScriptedToolCall],
) -> Result<(), ()> {
    let base_url = provider.base_url();
    save_search_backend(root, base_url).map_err(|_| ())?;
    configure(binary, root, base_url, &["--refresh", "--force-search"]).await?;
    require(read_json(&root.join("mimo/data/auth.json"))?["nan"]["key"] == TEST_CREDENTIAL)?;
    configure(binary, root, base_url, &["--status"]).await?;
    run_native(root, provider, calls).await?;
    save_credential(root, ROTATED_CREDENTIAL).map_err(|_| ())?;
    configure(binary, root, base_url, &["--refresh", "--no-search"]).await?;
    require(read_json(&root.join("mimo/data/auth.json"))?["nan"]["key"] == ROTATED_CREDENTIAL)?;
    let config = read_json(&root.join("mimo/config/mimocode.json"))?;
    require(
        config["provider"]["nan"]["models"]["qwen3.6"].is_object()
            && config["mcp"].get("nan-search").is_none(),
    )?;
    configure(binary, root, base_url, &["--remove"]).await?;
    require(
        read_json(&root.join("mimo/config/mimocode.json"))?
            == json!({"theme":"system", "model":"other/original"}),
    )?;
    require(
        read_json(&root.join("mimo/data/auth.json"))?
            == json!({"other":{"type":"api", "key":"synthetic-user-owned"}}),
    )
}

async fn configure(
    binary: &Path,
    root: &Path,
    base_url: &str,
    arguments: &[&str],
) -> Result<(), ()> {
    let output = isolated_command(binary, root)?
        .env("NAN_BASE_URL", base_url)
        .args(["config", "mimo"])
        .args(arguments)
        .run()
        .await
        .map_err(|_| ())?;
    require(
        output.status.success()
            && !output.stdout.contains(TEST_CREDENTIAL)
            && !output.stdout.contains(ROTATED_CREDENTIAL),
    )
}

async fn run_native(
    root: &Path,
    provider: &ScriptedProvider,
    calls: &[ScriptedToolCall],
) -> Result<(), ()> {
    let output = native_command(root, provider.base_url())?
        .run()
        .await
        .map_err(|_| {
            eprintln!(
                "MiMo native process failed: requests={}, search_requests={}, provider_complete={}, tools={:?}",
                provider.chat_requests().len(),
                provider.search_requests().len(),
                provider.completed(),
                provider.chat_requests().last().and_then(tool_names)
            );
        })?;
    let requests = provider.chat_requests();
    assert_tool_round_trip(&output, &requests, calls, ROUND_TRIP_MARKER).map_err(|_| ())?;
    require(provider.completed() && requests.iter().all(|request| request["model"] == "qwen3.6"))?;
    require(
        provider
            .search_requests()
            .iter()
            .any(|request| request["q"] == "mimo conformance"),
    )?;
    for (file, marker) in [
        ("write-output.txt", "MIMO_WRITE_OK"),
        ("edit-target.txt", "MIMO_AFTER"),
        ("bash-output.txt", "MIMO_BASH_OK"),
    ] {
        require(
            fs::read_to_string(root.join(file)).is_ok_and(|contents| contents.contains(marker)),
        )?;
    }
    Ok(())
}

fn native_command(root: &Path, base_url: &str) -> Result<TerminalCommand, ()> {
    Ok(isolated_command(Path::new("mimo"), root)?
        .env("NAN_BASE_URL", base_url)
        .args([
            "run",
            "--pure",
            "--format",
            "json",
            "--dangerously-skip-permissions",
            "Complete the deterministic native tool sequence.",
        ]))
}

fn isolated_command(binary: &Path, root: &Path) -> Result<TerminalCommand, ()> {
    let mut paths = vec![root.join("bin")];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let command = TerminalCommand::new(binary, root)
        .clear_environment()
        .env("PATH", std::env::join_paths(paths).map_err(|_| ())?)
        .env("HOME", root.join("home"))
        .env("MIMOCODE_HOME", root.join("mimo"))
        .env("NAN_HARNESS_CONFIG_DIR", root.join("state"))
        .env("NAN_NO_COMPATIBILITY_CHECK", "1")
        .env("NAN_NO_UPDATE_CHECK", "1")
        .env("CI", "1")
        .timeout(WRAPPER_TIMEOUT);
    #[cfg(windows)]
    let command = super::environment::apply(command, root).map_err(|_| ())?;
    #[cfg(not(windows))]
    let command = super::environment::apply(command, root);
    Ok(command)
}

fn native_calls(root: &Path, discovery: bool) -> Vec<ScriptedToolCall> {
    let mut calls = vec![
        call("read", json!({"file_path": root.join("read-target.txt")})),
        call(
            "write",
            json!({"file_path": root.join("write-output.txt"), "content": "MIMO_WRITE_OK\n"}),
        ),
        call("read", json!({"file_path": root.join("edit-target.txt")})),
        call(
            "edit",
            json!({"file_path": root.join("edit-target.txt"), "old_string": "EDIT_TARGET_BEFORE", "new_string": "MIMO_AFTER"}),
        ),
        call(
            "bash",
            json!({"command": "printf MIMO_BASH_OK > bash-output.txt", "description": "Write a conformance marker"}),
        ),
        call("glob", json!({"pattern": "*.txt", "path": root})),
        call(
            "grep",
            json!({"pattern": "READ_TARGET_CONTENT", "path": root.join("read-target.txt")}),
        ),
        call(
            "nan-search_web_search",
            json!({"query": "mimo conformance", "max_results": 1}),
        ),
        call(
            "actor",
            json!({"operation": {"action": "run", "subagent_type": "general", "description": "Verify auxiliary routing", "prompt": "Reply with the helper marker without tools.", "model": "lite", "context": "none", "timeout_ms": 30000}}),
        ),
    ];
    if discovery {
        calls.insert(
            7,
            call("mcp_tool_search", json!({"query":"web_search", "limit":1})),
        );
    }
    calls
}

fn save_credential(root: &Path, credential: &str) -> std::io::Result<()> {
    write_private(&root.join("state/nan-api-key"), credential)
}

fn save_search_backend(root: &Path, base_url: &str) -> std::io::Result<()> {
    write_private(
        &root.join("state/search.json"),
        &json!({
            "schemaVersion": 1, "mode": "local", "baseUrl": base_url.trim_end_matches("/v1")
        })
        .to_string(),
    )
}

fn write_private(path: &Path, contents: &str) -> std::io::Result<()> {
    open_private_truncate(path)?.write_all(contents.as_bytes())
}

fn read_json(path: &Path) -> Result<Value, ()> {
    serde_json::from_str(&fs::read_to_string(path).map_err(|_| ())?).map_err(|_| ())
}

fn require(condition: bool) -> Result<(), ()> {
    condition.then_some(()).ok_or(())
}
