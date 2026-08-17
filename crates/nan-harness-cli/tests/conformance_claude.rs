use nan_harness_core::HarnessKind;
use nan_harness_test_support::assertions::ClaudeTranscript;
use nan_harness_test_support::manifest::{
    ConformanceManifest, Coverage, Expectation, ToolScenario,
};
use nan_harness_test_support::scripted_provider::{
    ProviderScenario, ScriptedProvider, ScriptedToolCall,
};
use nan_harness_test_support::terminal::TerminalCommand;
use nan_harness_test_support::workspace::ConformanceWorkspace;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

const INVENTORY_MARKER: &str = "NAN_HARNESS_INVENTORY_OK";

#[test]
fn claude_code_conformance_manifest_is_self_consistent() {
    let manifest = ConformanceManifest::load(manifest_path())
        .expect("Claude Code conformance manifest should be valid");
    let compatibility = nan_harness_runtime::bundled_compatibility_manifest()
        .expect("bundled compatibility manifest should be valid");
    let claude = compatibility
        .entry(HarnessKind::ClaudeCode)
        .expect("Claude Code compatibility entry should exist");
    assert_eq!(
        manifest.last_verified_version,
        claude.last_verified_version.to_string()
    );
    let scenario_root = manifest_path()
        .parent()
        .expect("manifest should have a parent")
        .to_path_buf();
    for entry in &manifest.tools {
        let scenario = ToolScenario::load(scenario_root.join(&entry.scenario))
            .expect("every manifest entry should reference a valid scenario");
        assert_eq!(scenario.tool, entry.name);
        assert!(
            scenario.steps.iter().any(|step| step.tool == entry.name),
            "{} scenario never invokes its declared tool",
            entry.name
        );
    }
}

#[tokio::test]
#[ignore = "requires the pinned Claude Code executable"]
async fn claude_code_inventory_matches_the_conformance_manifest() {
    let manifest = ConformanceManifest::load(manifest_path())
        .expect("Claude Code conformance manifest should be valid");
    let workspace = ConformanceWorkspace::create().expect("conformance workspace should exist");
    let provider = ScriptedProvider::start(ProviderScenario::inventory(INVENTORY_MARKER))
        .await
        .expect("scripted provider should start");
    let output = run_harness(
        &workspace,
        provider.base_url(),
        [
            "-p".into(),
            "Reply exactly NAN_HARNESS_INVENTORY_OK without using tools.".into(),
            "--output-format".into(),
            "stream-json".into(),
            "--verbose".into(),
            "--no-session-persistence".into(),
            "--max-turns".into(),
            "1".into(),
        ],
    )
    .await;

    assert!(output.status.success(), "{}", output.diagnostic());
    let transcript = ClaudeTranscript::parse(output.stdout.clone())
        .expect("Claude Code should emit valid stream-json events");
    assert!(transcript.source().contains(INVENTORY_MARKER));
    let discovered_tools = transcript.tools();
    let unknown_tools = discovered_tools
        .difference(&manifest.tool_names())
        .cloned()
        .collect::<Vec<_>>();
    let uncovered_entries = manifest
        .tools
        .iter()
        .filter(|entry| !entry.names().any(|name| discovered_tools.contains(name)))
        .map(|entry| entry.name.clone())
        .collect::<Vec<_>>();
    assert!(
        unknown_tools.is_empty() && uncovered_entries.is_empty(),
        "the Claude Code tool inventory changed; unknown: {unknown_tools:?}, absent: {uncovered_entries:?}"
    );

    let schemas = provider
        .chat_requests()
        .iter()
        .find_map(tool_schemas)
        .expect("Claude should send tool schemas to the provider");
    let schema_names = schemas
        .as_object()
        .expect("tool schemas should be an object")
        .keys()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    assert!(
        schema_names.is_subset(&manifest.tool_names()),
        "Claude sent unknown tool schemas: {:?}",
        schema_names
            .difference(&manifest.tool_names())
            .collect::<Vec<_>>()
    );
    provider
        .shutdown()
        .await
        .expect("scripted provider should stop");
}

#[tokio::test]
#[ignore = "requires the pinned Claude Code executable"]
async fn claude_code_tools_complete_their_conformance_scenarios() {
    run_scenarios(Coverage::RoundTrip).await;
}

#[tokio::test]
#[ignore = "requires the pinned Claude Code executable and network access"]
async fn claude_code_network_tools_complete_their_conformance_scenarios() {
    run_scenarios(Coverage::NetworkRoundTrip).await;
}

#[tokio::test]
#[ignore = "requires the pinned Claude Code executable without Claude Design authentication"]
async fn claude_code_external_tools_report_their_authentication_prerequisites() {
    run_scenarios(Coverage::ExternalAuthentication).await;
}

async fn run_scenarios(coverage: Coverage) {
    let manifest = ConformanceManifest::load(manifest_path())
        .expect("Claude Code conformance manifest should be valid");
    let scenario_root = manifest_path()
        .parent()
        .expect("manifest should have a parent")
        .to_path_buf();
    let mut failures = Vec::new();

    for entry in manifest
        .tools
        .iter()
        .filter(|entry| entry.coverage == coverage)
    {
        if let Err(error) =
            run_tool_scenario(&scenario_root, &entry.scenario, &entry.name, &entry.name).await
        {
            failures.push(format!("{}: {error}", entry.name));
        }
    }

    assert!(
        failures.is_empty(),
        "Claude Code {coverage:?} conformance failures:\n{}",
        failures.join("\n\n")
    );
}

async fn run_tool_scenario(
    scenario_root: &Path,
    scenario_path: &Path,
    declared_tool: &str,
    runtime_tool: &str,
) -> Result<(), String> {
    let mut scenario =
        ToolScenario::load(scenario_root.join(scenario_path)).map_err(|error| error.to_string())?;
    if scenario.tool != declared_tool {
        return Err(format!(
            "scenario declares '{}' but manifest expects '{declared_tool}'",
            scenario.tool
        ));
    }
    for step in &mut scenario.steps {
        if step.tool == declared_tool {
            runtime_tool.clone_into(&mut step.tool);
        }
    }
    let workspace = ConformanceWorkspace::create().map_err(|error| error.to_string())?;
    scenario.expand_workspace(workspace.path(), "{{fixture_url}}");
    let provider = ScriptedProvider::start(ProviderScenario::sequence(
        scenario.steps.iter().map(|step| ScriptedToolCall {
            name: step.tool.clone(),
            input: step.input.clone(),
            result_expected: true,
        }),
        &scenario.final_marker,
    ))
    .await
    .map_err(|error| error.to_string())?;
    let enabled_tools = scenario
        .steps
        .iter()
        .map(|step| step.tool.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let mut arguments = vec![
        "-p".into(),
        format!(
            "Run the deterministic NaN Harness conformance scenario for {runtime_tool}. \
             Follow the tool calls and finish only after every result is available."
        )
        .into(),
        "--permission-mode".into(),
        "bypassPermissions".into(),
        "--output-format".into(),
        "stream-json".into(),
        "--verbose".into(),
        "--no-session-persistence".into(),
        "--max-turns".into(),
        "12".into(),
        "--tools".into(),
        enabled_tools.clone().into(),
        "--allowedTools".into(),
        enabled_tools.into(),
    ];
    arguments.extend(scenario.arguments.iter().map(Into::into));
    let output = run_harness(&workspace, provider.base_url(), arguments).await;
    let requests = provider.chat_requests();
    let search_requests = provider.search_requests();
    provider
        .shutdown()
        .await
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(output.diagnostic());
    }
    let transcript = ClaudeTranscript::parse(output.stdout.clone())
        .map_err(|error| format!("{error}\n{}", output.stderr))?;
    let assertion = scenario.expected_error.as_ref().map_or_else(
        || transcript.require_complete_tool_round_trip(runtime_tool, &scenario.final_marker),
        |expected_error| {
            transcript.require_expected_tool_error(
                runtime_tool,
                expected_error,
                &scenario.final_marker,
            )
        },
    );
    assertion.map_err(|error| {
        format!(
            "{error}\n{}\nprovider requests: {}\nsearch requests: {}",
            output.diagnostic(),
            requests.len(),
            search_requests.len()
        )
    })?;
    verify_expectation(&scenario.expectation)?;
    Ok(())
}

fn verify_expectation(expectation: &Expectation) -> Result<(), String> {
    match expectation {
        Expectation::None => Ok(()),
        Expectation::FileContains { path, text } => {
            let contents = std::fs::read_to_string(path)
                .map_err(|error| format!("could not read expected file '{path}': {error}"))?;
            if contents.contains(text) {
                Ok(())
            } else {
                Err(format!("file '{path}' does not contain '{text}'"))
            }
        }
        Expectation::FileMissing { path } if !Path::new(path).exists() => Ok(()),
        Expectation::FileMissing { path } => {
            Err(format!("file '{path}' exists but should be absent"))
        }
    }
}

async fn run_harness<I>(
    workspace: &ConformanceWorkspace,
    provider_base_url: &str,
    claude_arguments: I,
) -> nan_harness_test_support::terminal::TerminalOutput
where
    I: IntoIterator<Item = OsString>,
{
    let mut arguments = vec![
        "run".into(),
        "claude-code".into(),
        "--provider-base-url".into(),
        provider_base_url.into(),
        "--".into(),
    ];
    arguments.extend(claude_arguments);
    TerminalCommand::new(env!("CARGO_BIN_EXE_nan-harness"), workspace.path())
        .args(arguments)
        .env("NAN_API_KEY", "nan_test_key")
        .env("CLAUDE_CONFIG_DIR", workspace.claude_config_path())
        .env("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1")
        .timeout(Duration::from_secs(90))
        .run()
        .await
        .expect("NaN Harness should complete before the timeout")
}

fn tool_schemas(request: &Value) -> Option<Value> {
    let tools = request.get("tools")?.as_array()?;
    let schemas = tools
        .iter()
        .filter_map(|tool| {
            let function = tool.get("function")?;
            Some((
                function.get("name")?.as_str()?.to_owned(),
                json!({
                    "description": function.get("description"),
                    "parameters": function.get("parameters")
                }),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    (!schemas.is_empty()).then(|| json!(schemas))
}

fn manifest_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root should exist")
        .join("tests/conformance/claude-code/manifest.toml")
}
