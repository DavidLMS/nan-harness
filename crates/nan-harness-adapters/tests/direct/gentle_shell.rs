use super::support::{context, plan, with_search_block};
use nan_harness_adapters::GentleShellAdapter;
use nan_harness_core::{HarnessKind, WebSearchPolicy};
use std::io::Write as _;
use std::process::{Command, Stdio};

#[test]
fn gentle_shell_preserves_other_providers_and_launcher_arguments() {
    let context = context(HarnessKind::Pi, vec!["--home".into(), "/my/agent".into()]);
    let plan = plan(
        &GentleShellAdapter {
            preserve_provider: false,
            agent_home: "/gentle/agent".into(),
        },
        &context,
    );
    assert!(!plan.process.arguments.contains(&"--models".into()));
    assert!(
        plan.process
            .arguments
            .ends_with(&["--home".into(), "/my/agent".into()])
    );
    assert!(plan.configuration_overlays.is_empty());
}

#[test]
fn gentle_shell_provider_children_and_search_contract() {
    for preserve_provider in [false, true] {
        for search in [WebSearchPolicy::Auto, WebSearchPolicy::Force] {
            let mut context = context(HarnessKind::Pi, Vec::new());
            context.web_search_policy = search;
            let plan = plan(
                &GentleShellAdapter {
                    preserve_provider,
                    agent_home: "/gentle/agent".into(),
                },
                &context,
            );
            let source = with_search_block(
                plan.temporary_artifacts[0]
                    .content_template
                    .as_ref()
                    .unwrap(),
            )
            .replace(
                "import * as piAi from \"@earendil-works/pi-ai\";",
                "const calls = []; const piAi = {lazyApi: () => ({stream(...args) { calls.push(args); }, streamSimple(...args) { calls.push(args); }})};",
            )
            .replace(
                "import { Type } from \"@earendil-works/pi-ai\";",
                "const Type = new Proxy({}, {get: () => () => ({})});",
            )
            .replace("{runtime:pi_model_catalog}", "{}")
            .replace(
                "export default function registerNan",
                "function registerNan",
            );
            run(&format!(
                "const expectedForce = {};\n{source}\n{}",
                search == WebSearchPolicy::Force,
                include_str!("../fixtures/gentle_shell.js")
            ));
        }
    }
}

fn run(source: &str) {
    let mut child = Command::new("node")
        .args(["--input-type=module", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Node contract runner");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(source.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
