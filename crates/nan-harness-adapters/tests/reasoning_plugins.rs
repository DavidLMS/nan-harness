use nan_harness_adapters::{render_hermes_model_provider, render_pi_reasoning_extension};
use nan_harness_core::{CodingModelProfile, coding_model_profile};
use std::io::Write as _;
use std::process::{Command, Stdio};

fn profiles() -> Vec<CodingModelProfile> {
    [
        "qwen3.6",
        "gemma4",
        "glm5.3-flash",
        "deepseek-v4-flash",
        "qwen3.8-flash",
        "mimo-v2.6-flash",
    ]
    .into_iter()
    .map(|id| coding_model_profile(id).expect("profile"))
    .collect()
}

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
        Err(error) => panic!("reasoning contract should start: {error}"),
    };
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(source.as_bytes())
        .expect("script");
    let result = child.wait_with_output().expect("reasoning contract");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn pi_native_selection_preserves_auto_off_resume_and_model_boundaries() {
    let mut source = render_pi_reasoning_extension(&profiles(), false)
        .replace("export default function", "function");
    source.push_str(include_str!("fixtures/reasoning/pi.js"));
    run_script("node", &["--input-type=module", "-"], &source);
}

#[test]
fn hermes_provider_translates_only_supported_explicit_reasoning() {
    let mut models = profiles();
    models.push(CodingModelProfile::generic("quoted-'\"-model"));
    run_script(
        "python3",
        &["-c", include_str!("fixtures/reasoning/hermes.py")],
        &render_hermes_model_provider("https://api.nan.test/v1", &models),
    );
}

#[test]
fn prime_preserves_native_defaults_until_explicit_nan_intent() {
    let mut source = render_pi_reasoning_extension(&profiles(), true)
        .replace("export default function", "function");
    source.push_str(include_str!("fixtures/reasoning/prime.js"));
    run_script("node", &["--input-type=module", "-"], &source);
}
