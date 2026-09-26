use nan_harness_adapters::{render_hermes_image_plugin, render_openclaw_media_plugin};
use nan_harness_core::ImageModel;
use std::io::Write as _;
use std::process::{Command, Stdio};

fn run_script(executable: &str, arguments: &[&str], source: &str) {
    let mut child = match Command::new(executable)
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => panic!("image contract should start: {error}"),
    };
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(source.as_bytes())
        .expect("script");
    let result = child.wait_with_output().expect("image contract");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn hermes_image_tool_preserves_native_dispatch_and_per_call_models() {
    for model in [ImageModel::Flux2Klein, ImageModel::QwenImage21] {
        run_script(
            "python3",
            &[
                "-c",
                include_str!("fixtures/media/hermes.py"),
                model.as_str(),
            ],
            &render_hermes_image_plugin("https://api.nan.test/v1", model),
        );
    }
}

#[test]
fn openclaw_image_tool_forwards_default_and_explicit_models() {
    for model in [ImageModel::Flux2Klein, ImageModel::QwenImage21] {
        let mut source = render_openclaw_media_plugin("https://api.nan.test/v1", model)
            .replace(
                "import { definePluginEntry } from \"openclaw/plugin-sdk/plugin-entry\";",
                "const definePluginEntry = value => value;",
            )
            .replace(
                "export default definePluginEntry",
                "const plugin = definePluginEntry",
            )
            .replace(
                "import { spawn, spawnSync } from \"node:child_process\";",
                r"
const mediaCalls = [];
const spawnSync = () => ({status: 0});
function spawn(executable, args, options) {
  if (executable !== 'nanh' || options.env !== process.env) throw new Error('helper contract');
  mediaCalls.push(args);
  const handlers = {};
  queueMicrotask(async () => {
    await fs.writeFile(args[args.indexOf('--output') + 1], 'synthetic-image');
    handlers.close(0);
  });
  return {once(event, handler) { handlers[event] = handler; }};
}
",
            );
        source.push_str(include_str!("fixtures/media/openclaw.js"));
        run_script(
            "node",
            &["--input-type=module", "-", model.as_str()],
            &source,
        );
    }
}
