use nan_harness_core::HarnessKind;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConformanceManifest {
    pub schema_version: u32,
    pub harness: HarnessKind,
    pub profile: String,
    #[serde(default)]
    pub last_verified_version: String,
    #[serde(default)]
    pub last_compatible_version: String,
    #[serde(default)]
    pub inventory: Vec<String>,
    /// Tool names that may be present independently of the selected dynamic variant.
    #[serde(default)]
    pub optional_inventory: Vec<String>,
    /// Alternative tool-name sets supplied by environment-dependent providers.
    #[serde(default)]
    pub dynamic_inventory: Vec<Vec<String>>,
    #[serde(default)]
    pub tools: Vec<ToolManifestEntry>,
}

impl ConformanceManifest {
    /// Loads and validates a conformance manifest.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError`] when the manifest cannot be read, parsed, or violates its schema.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ManifestError> {
        let path = path.as_ref();
        let source = fs::read_to_string(path).map_err(|source| ManifestError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        Self::parse(&source, path)
    }

    /// Parses a manifest from an already available source.
    ///
    /// The published canary uses this entry point with [`include_str!`] sources so its
    /// conformance inventory does not depend on the repository being present at runtime.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError`] when the source is malformed or violates its schema.
    pub fn parse(source: &str, path: impl AsRef<Path>) -> Result<Self, ManifestError> {
        let path = path.as_ref();
        let manifest: Self = toml::from_str(source).map_err(|source| ManifestError::Parse {
            path: path.to_path_buf(),
            source,
        })?;
        manifest.validate()?;
        Ok(manifest)
    }

    #[must_use]
    pub fn tool_names(&self) -> BTreeSet<String> {
        self.inventory
            .iter()
            .map(String::as_str)
            .chain(self.optional_inventory.iter().map(String::as_str))
            .chain(self.dynamic_inventory.iter().flatten().map(String::as_str))
            .chain(self.tools.iter().flat_map(ToolManifestEntry::names))
            .map(ToOwned::to_owned)
            .collect()
    }

    #[must_use]
    pub fn compatibility_version(&self) -> &str {
        if self.last_compatible_version.is_empty() {
            &self.last_verified_version
        } else {
            &self.last_compatible_version
        }
    }

    fn validate(&self) -> Result<(), ManifestError> {
        if self.schema_version != 1 {
            return Err(ManifestError::UnsupportedSchema(self.schema_version));
        }
        if self
            .inventory
            .iter()
            .map(String::as_str)
            .chain(self.optional_inventory.iter().map(String::as_str))
            .chain(self.dynamic_inventory.iter().flatten().map(String::as_str))
            .chain(self.tools.iter().flat_map(ToolManifestEntry::names))
            .any(|name| name.trim().is_empty())
        {
            return Err(ManifestError::EmptyToolName);
        }
        let names = self.tool_names();
        let declared_name_count = self.inventory.len()
            + self.optional_inventory.len()
            + self.dynamic_inventory.iter().map(Vec::len).sum::<usize>()
            + self
                .tools
                .iter()
                .map(|tool| tool.aliases.len() + 1)
                .sum::<usize>();
        if names.len() != declared_name_count {
            return Err(ManifestError::DuplicateTool);
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolManifestEntry {
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub scenario: PathBuf,
    pub coverage: Coverage,
}

impl ToolManifestEntry {
    pub fn names(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.name.as_str()).chain(self.aliases.iter().map(String::as_str))
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Coverage {
    RoundTrip,
    NetworkRoundTrip,
    ExternalAuthentication,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolScenario {
    pub tool: String,
    pub steps: Vec<ToolStep>,
    pub final_marker: String,
    #[serde(default)]
    pub expected_error: Option<String>,
    #[serde(default)]
    pub arguments: Vec<String>,
    #[serde(default)]
    pub expectation: Expectation,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolStep {
    pub tool: String,
    pub input: Value,
}

impl ToolScenario {
    /// Loads one JSON tool scenario.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError`] when the scenario cannot be read or parsed.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ManifestError> {
        let path = path.as_ref();
        let source = fs::read_to_string(path).map_err(|source| ManifestError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        Self::parse(&source, path)
    }

    /// Parses a scenario from an already available source.
    ///
    /// # Errors
    ///
    /// Returns [`ManifestError`] when the source is malformed.
    pub fn parse(source: &str, path: impl AsRef<Path>) -> Result<Self, ManifestError> {
        let path = path.as_ref();
        serde_json::from_str(source).map_err(|source| ManifestError::ParseScenario {
            path: path.to_path_buf(),
            source,
        })
    }

    pub fn expand_workspace(&mut self, workspace: &Path, fixture_url: &str) {
        let workspace = workspace.to_string_lossy();
        for step in &mut self.steps {
            expand_value(&mut step.input, "{{workspace}}", &workspace);
            expand_value(&mut step.input, "{{fixture_url}}", fixture_url);
        }
        for argument in &mut self.arguments {
            *argument = argument
                .replace("{{workspace}}", &workspace)
                .replace("{{fixture_url}}", fixture_url);
        }
        self.expectation.expand_workspace(&workspace);
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Expectation {
    #[default]
    None,
    FileContains {
        path: String,
        text: String,
    },
    FileMissing {
        path: String,
    },
}

impl Expectation {
    fn expand_workspace(&mut self, workspace: &str) {
        match self {
            Self::None => {}
            Self::FileContains { path, .. } | Self::FileMissing { path } => {
                *path = path.replace("{{workspace}}", workspace);
            }
        }
    }
}

/// The conformance manifests shipped with the repository and embedded in every canary build.
///
/// Keep these as the source of inventory truth.  In particular, do not copy their tool lists
/// into the runner: aliases and environment-dependent contracts must move with the manifest.
#[must_use]
pub fn embedded_manifest_sources() -> &'static [(HarnessKind, &'static str)] {
    &[
        (
            HarnessKind::ClaudeCode,
            include_str!("../../../tests/conformance/claude-code/manifest.toml"),
        ),
        (
            HarnessKind::Codex,
            include_str!("../../../tests/conformance/codex/manifest.toml"),
        ),
        (
            HarnessKind::OpenCode,
            include_str!("../../../tests/conformance/opencode/manifest.toml"),
        ),
        (
            HarnessKind::Hermes,
            include_str!("../../../tests/conformance/hermes/manifest.toml"),
        ),
        (
            HarnessKind::Pi,
            include_str!("../../../tests/conformance/pi/manifest.toml"),
        ),
        (
            HarnessKind::Omp,
            include_str!("../../../tests/conformance/omp/manifest.toml"),
        ),
        (
            HarnessKind::PrimeAgent,
            include_str!("../../../tests/conformance/prime-agent/manifest.toml"),
        ),
        (
            HarnessKind::DeepSeekHarness,
            include_str!("../../../tests/conformance/deepseek-harness/manifest.toml"),
        ),
        (
            HarnessKind::OpenClaw,
            include_str!("../../../tests/conformance/openclaw/manifest.toml"),
        ),
        (
            HarnessKind::Cline,
            include_str!("../../../tests/conformance/cline/manifest.toml"),
        ),
        (
            HarnessKind::QwenCode,
            include_str!("../../../tests/conformance/qwen-code/manifest.toml"),
        ),
        (
            HarnessKind::KimiCode,
            include_str!("../../../tests/conformance/kimi-code/manifest.toml"),
        ),
        (
            HarnessKind::Aider,
            include_str!("../../../tests/conformance/aider/manifest.toml"),
        ),
        (
            HarnessKind::Goose,
            include_str!("../../../tests/conformance/goose/manifest.toml"),
        ),
        (
            HarnessKind::Fx,
            include_str!("../../../tests/conformance/fx/manifest.toml"),
        ),
    ]
}

/// Returns the embedded manifest for one canonical harness kind.
///
/// # Errors
///
/// Returns [`ManifestError`] when the embedded source is malformed.
pub fn embedded_manifest(kind: HarnessKind) -> Result<ConformanceManifest, ManifestError> {
    let source = embedded_manifest_sources()
        .iter()
        .find_map(|(entry_kind, source)| (*entry_kind == kind).then_some(*source))
        .ok_or(ManifestError::MissingEmbedded(kind))?;
    ConformanceManifest::parse(source, embedded_path(kind))
}

/// Loads an embedded tool scenario referenced by a manifest entry.
///
/// The published runner currently exercises Claude's `DesignSync` prerequisite scenario. Other
/// scenario files remain available to the native ignored tests, while adding a new published
/// scenario requires explicitly embedding its source here rather than falling back to a runtime
/// repository path.
///
/// # Errors
///
/// Returns [`ManifestError`] when the scenario is not embedded or is malformed.
pub fn embedded_tool_scenario(
    kind: HarnessKind,
    scenario: &Path,
) -> Result<ToolScenario, ManifestError> {
    let normalized = scenario.to_string_lossy().replace('\\', "/");
    let source = match (kind, normalized.as_str()) {
        (HarnessKind::ClaudeCode, "scenarios/design-sync.json") => {
            include_str!("../../../tests/conformance/claude-code/scenarios/design-sync.json")
        }
        _ => return Err(ManifestError::MissingEmbeddedScenario(normalized)),
    };
    ToolScenario::parse(source, embedded_scenario_path(kind, scenario))
}

fn embedded_path(kind: HarnessKind) -> PathBuf {
    PathBuf::from(format!("<embedded:{kind}/manifest.toml>"))
}

fn embedded_scenario_path(kind: HarnessKind, scenario: &Path) -> PathBuf {
    PathBuf::from(format!("<embedded:{kind}/{}>", scenario.display()))
}

fn expand_value(value: &mut Value, pattern: &str, replacement: &str) {
    match value {
        Value::String(text) => *text = text.replace(pattern, replacement),
        Value::Array(values) => {
            for value in values {
                expand_value(value, pattern, replacement);
            }
        }
        Value::Object(object) => {
            for value in object.values_mut() {
                expand_value(value, pattern, replacement);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("could not read conformance file '{}': {source}", path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not parse conformance manifest '{}': {source}", path.display())]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("could not parse conformance scenario '{}': {source}", path.display())]
    ParseScenario {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("unsupported conformance manifest schema version {0}")]
    UnsupportedSchema(u32),
    #[error("conformance manifest contains duplicate tool names")]
    DuplicateTool,
    #[error("conformance manifest contains an empty tool name")]
    EmptyToolName,
    #[error("no embedded conformance manifest exists for {0}")]
    MissingEmbedded(HarnessKind),
    #[error("no embedded conformance scenario exists for '{0}'")]
    MissingEmbeddedScenario(String),
}
