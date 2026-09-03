use super::*;

#[test]
fn persistent_search_plugins_have_valid_source_syntax() {
    use std::io::Write as _;
    use std::process::{Command, Stdio};

    let mut node = match Command::new("node")
        .args(["--input-type=module", "--check"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => panic!("Node syntax check should start: {error}"),
    };
    node.stdin
        .take()
        .expect("Node stdin should be available")
        .write_all(openclaw_search_plugin().as_bytes())
        .expect("plugin source should write");
    let output = node
        .wait_with_output()
        .expect("Node syntax check should finish");
    assert!(
        output.status.success(),
        "OpenClaw plugin syntax failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    for mode in [PiSearchMode::Auto, PiSearchMode::Force] {
        let mut node = Command::new("node")
            .args(["--input-type=module", "--check"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("Node syntax check should start after the first successful invocation");
        node.stdin
            .take()
            .expect("Node stdin should be available")
            .write_all(render_pi_search_extension("https://api.nan.test/v1", mode).as_bytes())
            .expect("Pi extension source should write");
        let output = node
            .wait_with_output()
            .expect("Node syntax check should finish");
        assert!(
            output.status.success(),
            "Pi extension syntax failed in {mode:?} mode: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let mut python = match Command::new("python3")
        .args([
            "-c",
            "import sys; compile(sys.stdin.read(), 'provider.py', 'exec')",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => panic!("Python syntax check should start: {error}"),
    };
    python
        .stdin
        .take()
        .expect("Python stdin should be available")
        .write_all(hermes_search_provider().as_bytes())
        .expect("provider source should write");
    let output = python
        .wait_with_output()
        .expect("Python syntax check should finish");
    assert!(
        output.status.success(),
        "Hermes provider syntax failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn pi_search_extension_runtime_detection_respects_auto_and_force() {
    use std::fmt::Write as _;
    use std::io::Write as _;
    use std::process::{Command, Stdio};

    for (mode, existing_search, expected_registrations) in [
        (PiSearchMode::Auto, true, 0),
        (PiSearchMode::Auto, false, 1),
        (PiSearchMode::Force, true, 1),
    ] {
        let mut source = render_pi_search_extension("https://api.nan.test/v1", mode)
            .replacen(
                "import { Type } from \"@earendil-works/pi-ai\";",
                "const Type = new Proxy({}, { get: () => (...args) => args[0] ?? {} });",
                1,
            )
            .replacen(
                "export default function registerNanSearch",
                "function registerNanSearch",
                1,
            );
        let inventory = if existing_search {
            "[{ name: \"web_search\" }]"
        } else {
            "[]"
        };
        write!(
            source,
            r#"
let discover;
const registrations = [];
const pi = {{
  on(event, handler) {{
    if (event !== "resources_discover") throw new Error(`unexpected event: ${{event}}`);
    discover = handler;
  }},
  getAllTools() {{ return {inventory}; }},
  registerTool(tool) {{ registrations.push(tool); }}
}};
registerNanSearch(pi);
discover();
if (registrations.length !== {expected_registrations}) {{
  throw new Error(`expected {expected_registrations} registrations, got ${{registrations.length}}`);
}}
"#
        )
        .expect("runtime check source should render");

        let mut node = match Command::new("node")
            .args(["--input-type=module"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => panic!("Node runtime check should start: {error}"),
        };
        node.stdin
            .take()
            .expect("Node stdin should be available")
            .write_all(source.as_bytes())
            .expect("runtime check source should write");
        let output = node
            .wait_with_output()
            .expect("Node runtime check should finish");
        assert!(
            output.status.success(),
            "Pi runtime detection failed for {mode:?} with existing_search={existing_search}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
