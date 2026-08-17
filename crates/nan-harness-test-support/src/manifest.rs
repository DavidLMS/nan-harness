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
    pub harness: String,
    pub profile: String,
    pub last_verified_version: String,
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
        let manifest: Self = toml::from_str(&source).map_err(|source| ManifestError::Parse {
            path: path.to_path_buf(),
            source,
        })?;
        manifest.validate()?;
        Ok(manifest)
    }

    #[must_use]
    pub fn tool_names(&self) -> BTreeSet<String> {
        self.tools
            .iter()
            .flat_map(ToolManifestEntry::names)
            .map(ToOwned::to_owned)
            .collect()
    }

    fn validate(&self) -> Result<(), ManifestError> {
        if self.schema_version != 1 {
            return Err(ManifestError::UnsupportedSchema(self.schema_version));
        }
        if self.harness != "claude-code" {
            return Err(ManifestError::UnsupportedHarness(self.harness.clone()));
        }
        let names = self.tool_names();
        let declared_name_count = self
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
        serde_json::from_str(&source).map_err(|source| ManifestError::ParseScenario {
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
    #[error("unsupported conformance harness '{0}'")]
    UnsupportedHarness(String),
    #[error("conformance manifest contains duplicate tool names")]
    DuplicateTool,
}
