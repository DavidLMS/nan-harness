use super::constants::{ROUND_TRIP_MARKER, TEST_CREDENTIAL, WRAPPER_TIMEOUT};
use super::helpers::{call, duration_milliseconds, progress_event, tool_names};
use super::report::{ConformanceCheck, ConformanceStatus};
use crate::assertions::assert_tool_round_trip;
use crate::scripted_provider::{ProviderScenario, ScriptedProvider, ScriptedToolCall};
use crate::terminal::TerminalCommand;
use crate::workspace::ConformanceWorkspace;
use nan_harness_private_fs::{create_private_dir_all, open_private_truncate};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

const CHECK_NAME: &str = "native-configuration";
const ROTATED_CREDENTIAL: &str = "nan-harness-conformance-rotated-credential";

pub(super) fn marker_write_command(file: &str, marker: &str) -> String {
    if cfg!(windows) {
        // MiMo 0.1.16 selects PowerShell on Windows. Python is already a required
        // hosted prerequisite; do not depend on Git's optional printf.exe on PATH.
        format!(
            "python -c \"from pathlib import Path; Path('{file}').write_text('{marker}', encoding='utf-8')\""
        )
    } else {
        format!("printf {marker} > {file}")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NativeMcpRoute {
    Direct,
    Search,
    Dispatch,
}

fn native_mcp_route(tools: &BTreeSet<String>) -> Result<NativeMcpRoute, ()> {
    if tools.contains("mcp_tool_use") {
        require(tools.contains("mcp_tool_search"))?;
        Ok(NativeMcpRoute::Dispatch)
    } else if tools.contains("mcp_tool_search") {
        Ok(NativeMcpRoute::Search)
    } else if tools.contains("nan-search_web_search") {
        Ok(NativeMcpRoute::Direct)
    } else {
        Err(())
    }
}

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
    let route = discover_native_tools(&binary, workspace.path()).await?;
    let calls = native_calls(workspace.path(), route);
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

async fn discover_native_tools(binary: &Path, root: &Path) -> Result<NativeMcpRoute, ()> {
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
) -> Result<NativeMcpRoute, ()> {
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
        .collect::<BTreeSet<_>>();
    let route = native_mcp_route(&tools);
    if route.is_err() {
        eprintln!("MiMo native MCP discovery unavailable: tools={tools:?}");
    }
    route
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

fn native_calls(root: &Path, route: NativeMcpRoute) -> Vec<ScriptedToolCall> {
    let search_arguments = json!({"query": "mimo conformance", "max_results": 1});
    let search = if route == NativeMcpRoute::Dispatch {
        call(
            "mcp_tool_use",
            json!({"name": "nan-search_web_search", "arguments": search_arguments}),
        )
    } else {
        call("nan-search_web_search", search_arguments)
    };
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
            json!({"command": marker_write_command("bash-output.txt", "MIMO_BASH_OK"), "description": "Write a conformance marker"}),
        ),
        call("glob", json!({"pattern": "*.txt", "path": root})),
        call(
            "grep",
            json!({"pattern": "READ_TARGET_CONTENT", "path": root.join("read-target.txt")}),
        ),
        search,
        call(
            "actor",
            json!({"operation": {"action": "run", "subagent_type": "general", "description": "Verify auxiliary routing", "prompt": "Reply with the helper marker without tools.", "model": "lite", "context": "none", "timeout_ms": 30000}}),
        ),
    ];
    if route != NativeMcpRoute::Direct {
        // Older search loads the named tool; native dispatch keeps schemas deferred.
        let arguments = if route == NativeMcpRoute::Dispatch {
            json!({"query":"web_search", "max_results":1})
        } else {
            json!({"query":"web_search", "limit":1})
        };
        calls.insert(7, call("mcp_tool_search", arguments));
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

#[cfg(test)]
mod marker_command_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assertions::{
        ProbeAssertionError, assert_provider_tool_round_trip, expected_tool_call_id,
    };

    fn catalog(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn advertised_mcp_contract_selects_native_dispatch_and_preserves_legacy_routes() {
        for (tools, route) in [
            (vec!["nan-search_web_search"], NativeMcpRoute::Direct),
            (vec!["mcp_tool_search"], NativeMcpRoute::Search),
            (
                vec!["mcp_tool_search", "mcp_tool_use"],
                NativeMcpRoute::Dispatch,
            ),
        ] {
            assert_eq!(native_mcp_route(&catalog(&tools)), Ok(route));
            let calls = native_calls(Path::new("fixture"), route);
            let search_index = usize::from(route != NativeMcpRoute::Direct) + 7;
            let search = &calls[search_index];
            let arguments = json!({"query":"mimo conformance", "max_results":1});
            if route == NativeMcpRoute::Dispatch {
                assert_eq!(search.name, "mcp_tool_use");
                assert_eq!(
                    search.input,
                    json!({"name":"nan-search_web_search", "arguments":arguments})
                );
            } else {
                assert_eq!(search.name, "nan-search_web_search");
                assert_eq!(search.input, arguments);
            }
            if route != NativeMcpRoute::Direct {
                assert_eq!(calls[7].name, "mcp_tool_search");
                assert_eq!(
                    calls[7].input,
                    if route == NativeMcpRoute::Dispatch {
                        json!({"query":"web_search", "max_results":1})
                    } else {
                        json!({"query":"web_search", "limit":1})
                    }
                );
            }
            assert_eq!(calls.last().unwrap().name, "actor");
        }
        assert!(native_mcp_route(&catalog(&[])).is_err());
        assert!(native_mcp_route(&catalog(&["mcp_tool_use"])).is_err());
    }

    fn exchange(calls: &[ScriptedToolCall]) -> Vec<Value> {
        let messages = calls
            .iter()
            .enumerate()
            .flat_map(|(index, call)| {
                let id = expected_tool_call_id(index);
                [
                    json!({"role":"assistant", "tool_calls":[{"id":id,"function":{
                "name":call.name,"arguments":call.input.to_string()}}]}),
                    json!({"role":"tool","tool_call_id":id,"content":"native result"}),
                ]
            })
            .collect::<Vec<_>>();
        vec![json!({"messages":messages})]
    }

    #[test]
    fn every_mcp_route_requires_the_native_search_result_and_rejects_tool_errors() {
        for route in [
            NativeMcpRoute::Direct,
            NativeMcpRoute::Search,
            NativeMcpRoute::Dispatch,
        ] {
            let calls = native_calls(Path::new("fixture"), route);
            let request = exchange(&calls);
            assert!(assert_provider_tool_round_trip(&request, &calls).is_ok());
            let search_index = usize::from(route != NativeMcpRoute::Direct) + 7;
            let mut missing = request.clone();
            missing[0]["messages"]
                .as_array_mut()
                .unwrap()
                .remove(2 * search_index + 1);
            assert!(matches!(
                assert_provider_tool_round_trip(&missing, &calls),
                Err(ProbeAssertionError::UnexpectedToolResults { .. })
            ));
            let mut failed = request;
            failed[0]["messages"][2 * search_index + 1]["content"] = json!({"isError":true});
            assert!(matches!(
                assert_provider_tool_round_trip(&failed, &calls),
                Err(ProbeAssertionError::ToolResultError)
            ));
        }
    }
}
