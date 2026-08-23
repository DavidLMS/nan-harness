use crate::app::UxArgs;
use nan_harness_diagnostics::{MessageLevel, ReportPolicy, UserMessage};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::io::Write as _;
use std::path::Path;
use tempfile::Builder as TempFileBuilder;
use thiserror::Error;

const SCENARIOS: &str = include_str!("../resources/ux-scenarios.json");
const CATALOG_STYLE: &str = r#"
    :root { color-scheme: dark; font-family: ui-sans-serif, system-ui, sans-serif; background: #090909; color: #f5f1e8; }
    * { box-sizing: border-box; }
    ::selection { background: #9b772f; color: #fffaf0; }
    body { margin: 0; background: radial-gradient(circle at 85% 0%, #221a0d 0, #090909 36rem); }
    main { width: min(1160px, calc(100% - 32px)); margin: 0 auto; padding: 64px 0 96px; }
    header { display: grid; grid-template-columns: minmax(0, 1.5fr) minmax(260px, .7fr); gap: 48px; align-items: end; margin-bottom: 40px; }
    h1 { max-width: 760px; margin: 0; font-size: clamp(2.4rem, 7vw, 5.5rem); letter-spacing: -0.04em; line-height: .95; text-wrap: balance; }
    header p { max-width: 68ch; margin: 20px 0 0; color: #b9b1a5; font-size: 1.05rem; line-height: 1.65; }
    .scope { display: grid; gap: 16px; padding: 20px 0 4px 24px; border-left: 1px solid #5c471f; }
    .scope div { display: grid; gap: 4px; }
    .scope strong { color: #f2e2be; font-size: 1.8rem; font-variant-numeric: tabular-nums; }
    .scope span { color: #9d968c; font-size: .9rem; line-height: 1.45; }
    .legend { display: grid; grid-template-columns: repeat(3, 1fr); gap: 1px; margin-bottom: 28px; overflow: hidden; border: 1px solid #2b2926; border-radius: 14px; background: #2b2926; }
    .legend div { min-width: 0; padding: 16px 18px; background: #11100f; }
    .legend strong { display: block; margin-bottom: 5px; font-size: .92rem; }
    .legend span { color: #9d968c; font-size: .82rem; line-height: 1.5; }
    .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(100%, 430px), 1fr)); gap: 16px; }
    .card { min-width: 0; border: 1px solid #2b2926; border-radius: 14px; padding: 22px; background: linear-gradient(145deg, #151412, #0d0d0c); }
    .card[data-level="setup"] { border-color: #5c471f; }
    .card[data-level="error"] { border-color: #592c2c; }
    .meta { display: flex; justify-content: space-between; gap: 12px; color: #bda36c; font-size: .76rem; text-transform: uppercase; letter-spacing: .08em; }
    .meta code { color: #77716a; text-transform: none; letter-spacing: 0; }
    h2 { margin: 18px 0 20px; font-size: 1.25rem; letter-spacing: -.015em; }
    h3 { margin: 22px 0 9px; color: #a9a198; font-size: .76rem; font-weight: 650; letter-spacing: .08em; text-transform: uppercase; }
    .invocation { display: grid; gap: 8px; margin-bottom: 20px; padding: 13px 15px; border-radius: 10px; background: #080808; }
    .invocation span { color: #8e877e; font-size: .72rem; font-weight: 650; letter-spacing: .06em; text-transform: uppercase; }
    .invocation code { overflow-wrap: anywhere; color: #f2e2be; font: .9rem/1.5 ui-monospace, SFMono-Regular, Menlo, monospace; }
    .context { display: grid; gap: 16px; margin: 0; }
    .context div { display: grid; gap: 5px; }
    dt { color: #8e877e; font-size: .76rem; font-weight: 650; letter-spacing: .06em; text-transform: uppercase; }
    dd { margin: 0; color: #ddd6cc; font-size: .92rem; line-height: 1.55; }
    .behavior { display: flex; flex-wrap: wrap; gap: 8px 16px; margin-top: 18px; padding-top: 15px; border-top: 1px solid #292622; color: #bda36c; font-size: .78rem; }
    .card[data-level="error"] .behavior { color: #d58d87; }
    pre { margin: 0; overflow-x: auto; white-space: pre-wrap; overflow-wrap: anywhere; border-radius: 12px; padding: 16px; background: #080808; color: #dfd9cf; font: .86rem/1.65 ui-monospace, SFMono-Regular, Menlo, monospace; }
    @media (max-width: 760px) {
      main { width: min(100% - 24px, 1160px); padding: 40px 0 72px; }
      header { grid-template-columns: 1fr; gap: 24px; }
      .scope { grid-template-columns: 1fr 1fr; padding: 16px 0 0; border-top: 1px solid #5c471f; border-left: 0; }
      .legend { grid-template-columns: 1fr; }
      .card { padding: 18px; }
      .meta { align-items: flex-start; flex-direction: column; gap: 5px; }
    }
"#;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UxScenario {
    id: String,
    title: String,
    category: String,
    command: String,
    appears_when: String,
    result: String,
    #[serde(default)]
    terminal_output: Option<String>,
    message: UserMessage,
}

pub(crate) fn run(arguments: &UxArgs) -> Result<(), UxError> {
    let scenarios = load_scenarios()?;
    if arguments.list {
        for scenario in &scenarios {
            println!("{}", scenario.id);
        }
        return Ok(());
    }

    let selected = select_scenarios(&scenarios, arguments.scenario.as_deref())?;
    for (index, scenario) in selected.iter().enumerate() {
        if index > 0 {
            println!();
        }
        println!("== {} ==", scenario.title);
        println!("Command: {}", scenario.command);
        println!("Appears when: {}", scenario.appears_when);
        println!("Result: {}", scenario.result);
        println!();
        println!("{}", terminal_output(scenario));
    }

    if let Some(path) = &arguments.html {
        write_html(path, &scenarios)?;
        eprintln!("UX catalog written to '{}'.", path.display());
    }
    Ok(())
}

fn load_scenarios() -> Result<Vec<UxScenario>, UxError> {
    let scenarios: Vec<UxScenario> = serde_json::from_str(SCENARIOS).map_err(UxError::Parse)?;
    validate_scenarios(&scenarios)?;
    Ok(scenarios)
}

fn validate_scenarios(scenarios: &[UxScenario]) -> Result<(), UxError> {
    let mut identifiers = BTreeSet::new();
    for scenario in scenarios {
        if !identifiers.insert(scenario.id.as_str()) {
            return Err(UxError::DuplicateScenario(scenario.id.clone()));
        }
        if scenario.id.trim().is_empty()
            || scenario.title.trim().is_empty()
            || scenario.category.trim().is_empty()
            || scenario.command.trim().is_empty()
            || scenario.appears_when.trim().is_empty()
            || scenario.result.trim().is_empty()
            || scenario.message.summary.trim().is_empty()
        {
            return Err(UxError::InvalidScenario(scenario.id.clone()));
        }
        if scenario
            .terminal_output
            .as_deref()
            .is_some_and(|output| output.trim().is_empty())
        {
            return Err(UxError::InvalidScenario(scenario.id.clone()));
        }
        match scenario.message.level {
            MessageLevel::Error => {
                if scenario.message.code.is_none()
                    || scenario.message.report_policy != ReportPolicy::ConsentAware
                {
                    return Err(UxError::InvalidScenario(scenario.id.clone()));
                }
            }
            MessageLevel::Warning | MessageLevel::SetupRequired => {
                if scenario.message.code.is_some()
                    || scenario.message.report_policy != ReportPolicy::Never
                {
                    return Err(UxError::InvalidScenario(scenario.id.clone()));
                }
            }
        }
    }
    Ok(())
}

fn select_scenarios<'a>(
    scenarios: &'a [UxScenario],
    identifier: Option<&str>,
) -> Result<Vec<&'a UxScenario>, UxError> {
    let Some(identifier) = identifier else {
        return Ok(scenarios.iter().collect());
    };
    scenarios
        .iter()
        .find(|scenario| scenario.id == identifier)
        .map(|scenario| vec![scenario])
        .ok_or_else(|| UxError::UnknownScenario(identifier.to_owned()))
}

fn write_html(path: &Path, scenarios: &[UxScenario]) -> Result<(), UxError> {
    let mut cards = String::new();
    for scenario in scenarios {
        let rendered = terminal_output(scenario);
        let (level, level_label, report_label) = presentation(&scenario.message);
        write!(
            cards,
            concat!(
                "<article class=\"card\" data-category=\"{}\" data-level=\"{}\">",
                "<div class=\"meta\"><span>{}</span><code>{}</code></div>",
                "<h2>{}</h2>",
                "<div class=\"invocation\"><span>Command being run</span><code>$ {}</code></div>",
                "<dl class=\"context\">",
                "<div><dt>Appears when</dt><dd>{}</dd></div>",
                "<div><dt>What happens next</dt><dd>{}</dd></div>",
                "</dl>",
                "<div class=\"behavior\"><span>{}</span><span>{}</span></div>",
                "<h3>What the user sees</h3><pre>{}</pre></article>"
            ),
            escape_html(&scenario.category),
            level,
            escape_html(&scenario.category),
            escape_html(&scenario.id),
            escape_html(&scenario.title),
            escape_html(&scenario.command),
            escape_html(&scenario.appears_when),
            escape_html(&scenario.result),
            level_label,
            report_label,
            escape_html(&rendered)
        )
        .expect("writing to a String cannot fail");
    }
    let category_count = scenarios
        .iter()
        .map(|scenario| scenario.category.as_str())
        .collect::<BTreeSet<_>>()
        .len();
    let html = format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>nan-harness UX diagnostics</title>
  <style>{style}</style>
</head>
<body>
  <main>
    <header>
      <div><h1>When each diagnostic appears</h1><p>This is the complete catalog of user-facing regression scenarios currently modeled by nan-harness. It covers nan-harness guidance and failures—not every message printed internally by third-party harnesses or their installers.</p></div>
      <aside class="scope" aria-label="Catalog coverage"><div><strong>{scenario_count}</strong><span>modeled user situations</span></div><div><strong>{category_count}</strong><span>installation and runtime areas</span></div></aside>
    </header>
    <section class="legend" aria-label="Message behavior">
      <div><strong>Setup required</strong><span>Your machine or account needs attention. nan-harness stops safely and never asks for an error report.</span></div>
      <div><strong>Warning</strong><span>nan-harness can continue, or you cancelled an optional action. No error report is requested.</span></div>
      <div><strong>nan-harness error</strong><span>nan-harness, its bridge, or an installer failed. The CLI shows an error code and may ask permission to send a report.</span></div>
    </section>
    <section class="grid">{cards}</section>
  </main>
</body>
</html>
"#,
        scenario_count = scenarios.len(),
        style = CATALOG_STYLE
    );
    atomic_write(path, html.as_bytes())
}

fn presentation(message: &UserMessage) -> (&'static str, &'static str, &'static str) {
    match message.level {
        MessageLevel::Warning => ("warning", "Warning", "No error report"),
        MessageLevel::SetupRequired => ("setup", "Setup required", "No error report"),
        MessageLevel::Error => ("error", "nan-harness error", "Report offered with consent"),
    }
}

fn terminal_output(scenario: &UxScenario) -> String {
    scenario
        .terminal_output
        .clone()
        .unwrap_or_else(|| scenario.message.render_terminal())
}

fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), UxError> {
    let parent = path
        .parent()
        .ok_or_else(|| UxError::InvalidOutputPath(path.to_owned()))?;
    fs::create_dir_all(parent).map_err(|source| UxError::CreateOutput {
        path: parent.to_owned(),
        source,
    })?;
    let mut temporary = TempFileBuilder::new()
        .prefix(".nan-harness-ux-")
        .tempfile_in(parent)
        .map_err(|source| UxError::WriteOutput {
            path: path.to_owned(),
            source,
        })?;
    temporary
        .write_all(contents)
        .and_then(|()| temporary.flush())
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|source| UxError::WriteOutput {
            path: path.to_owned(),
            source,
        })?;
    temporary
        .persist(path)
        .map_err(|error| UxError::WriteOutput {
            path: path.to_owned(),
            source: error.error,
        })?;
    Ok(())
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[derive(Debug, Error)]
pub(crate) enum UxError {
    #[error("could not parse the embedded UX scenarios: {0}")]
    Parse(serde_json::Error),
    #[error("UX scenario identifier '{0}' is duplicated")]
    DuplicateScenario(String),
    #[error("UX scenario '{0}' is invalid")]
    InvalidScenario(String),
    #[error("unknown UX scenario '{0}'")]
    UnknownScenario(String),
    #[error("UX output path '{}' has no parent directory", .0.display())]
    InvalidOutputPath(std::path::PathBuf),
    #[error("could not create UX output directory '{}': {source}", path.display())]
    CreateOutput {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not write UX output '{}': {source}", path.display())]
    WriteOutput {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::{escape_html, load_scenarios, write_html};

    #[test]
    fn embedded_scenarios_are_valid_and_cover_setup_and_errors() {
        let scenarios = load_scenarios().expect("embedded scenarios should be valid");
        assert!(scenarios.len() >= 23);
        assert!(
            scenarios
                .iter()
                .any(|scenario| scenario.id == "deepseek-node-old")
        );
        assert!(
            scenarios
                .iter()
                .any(|scenario| scenario.id == "tool-bridge-failed")
        );
        assert!(
            scenarios
                .iter()
                .any(|scenario| scenario.id == "native-config-user-change")
        );
    }

    #[test]
    fn html_catalog_contains_every_scenario_without_raw_markup() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let path = directory.path().join("index.html");
        let scenarios = load_scenarios().expect("embedded scenarios should be valid");
        write_html(&path, &scenarios).expect("catalog should be written");
        let html = std::fs::read_to_string(path).expect("catalog should be readable");
        assert!(html.contains("deepseek-node-old"));
        assert!(html.contains("Appears when"));
        assert!(html.contains("What happens next"));
        assert!(html.contains("Command being run"));
        assert!(html.contains("$ nan dsh"));
        assert!(html.contains("Install Claude Code [y/N]:"));
        assert!(!html.contains("Install the official release:"));
        assert!(html.contains("23</strong>"));
        assert!(html.contains("setup required:"));
        assert!(!html.contains("<script"));
    }

    #[test]
    fn html_escaping_covers_terminal_content() {
        assert_eq!(
            escape_html("<tag attr='x'>&"),
            "&lt;tag attr=&#39;x&#39;&gt;&amp;"
        );
    }
}
