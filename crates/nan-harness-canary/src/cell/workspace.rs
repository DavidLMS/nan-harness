use super::{
    errors::CellError,
    spec::{CellSpec, LoadedSpec},
};
use crate::report::{CanaryObservation, CanaryObservationKind};
use nan_harness_core::HarnessKind;
use nan_harness_test_support::conformance::{ConformanceObservationKind, ConformanceReport};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

pub(crate) const MAX_CONFORMANCE_REPORT_SIZE: u64 = 64 * 1024;

pub(crate) struct CellWorkspace {
    pub(crate) _root: TempDir,
    pub(crate) input: PathBuf,
    pub(crate) output: PathBuf,
    pub(crate) logs: PathBuf,
    pub(crate) nan_harness_sha256: String,
}

impl CellWorkspace {
    pub(crate) fn prepare(spec: &LoadedSpec) -> Result<Self, CellError> {
        let root = tempfile::Builder::new()
            .prefix("nan-harness-canary-cell-")
            .tempdir()
            .map_err(CellError::CreateWorkspace)?;
        let input = root.path().join("input");
        let output = root.path().join("output");
        let logs = root.path().join("private-logs");
        fs::create_dir_all(&input).map_err(CellError::CreateWorkspace)?;
        fs::create_dir_all(&output).map_err(CellError::CreateWorkspace)?;
        fs::create_dir_all(&logs).map_err(CellError::CreateWorkspace)?;

        let nan_harness_source = spec.resolve(&spec.value.nan_harness.artifact)?;
        let nan_harness_contents =
            fs::read(&nan_harness_source).map_err(|source| CellError::ReadArtifact {
                path: nan_harness_source.clone(),
                source,
            })?;
        let nan_harness_name = spec
            .value
            .nan_harness
            .artifact
            .file_name()
            .ok_or_else(|| CellError::InvalidArtifactName("nanHarness.artifact".to_owned()))?;
        fs::write(input.join(nan_harness_name), &nan_harness_contents).map_err(|source| {
            CellError::CopyArtifact {
                path: nan_harness_source,
                source,
            }
        })?;

        for artifact in &spec.value.artifacts {
            let source_path = spec.resolve(&artifact.source)?;
            fs::copy(&source_path, input.join(&artifact.name)).map_err(|source| {
                CellError::CopyArtifact {
                    path: source_path,
                    source,
                }
            })?;
        }

        Ok(Self {
            _root: root,
            input,
            output,
            logs,
            nan_harness_sha256: crate::report::sha256_hex(&nan_harness_contents),
        })
    }

    pub(crate) fn log_path(&self, step: &str, attempt: u8) -> PathBuf {
        self.logs
            .join(format!("{}-{attempt}.log", safe_log_name(step)))
    }

    pub(crate) fn preserve_logs(&self, destination: &Path) -> Result<(), CellError> {
        fs::create_dir_all(destination).map_err(CellError::CreatePrivateLogDirectory)?;
        for entry in fs::read_dir(&self.logs).map_err(CellError::ReadPrivateLogs)? {
            let entry = entry.map_err(CellError::ReadPrivateLogs)?;
            if entry
                .file_type()
                .map_err(CellError::ReadPrivateLogs)?
                .is_file()
            {
                fs::copy(entry.path(), destination.join(entry.file_name()))
                    .map_err(CellError::PreservePrivateLog)?;
            }
        }
        Ok(())
    }

    pub(crate) fn harness_version(&self, spec: &CellSpec) -> Option<String> {
        let contents = fs::read_to_string(self.output.join(&spec.harness_version_file)).ok()?;
        contents.split_whitespace().find_map(|token| {
            let candidate = token.trim().trim_start_matches('v');
            semver::Version::parse(candidate)
                .ok()
                .map(|version| version.to_string())
        })
    }

    pub(crate) fn conformance_observations(
        &self,
        harness: HarnessKind,
    ) -> Result<Vec<CanaryObservation>, &'static str> {
        let path = self.output.join("conformance.json");
        let metadata = fs::metadata(&path).map_err(|_| "conformance report is unavailable")?;
        if !metadata.is_file() || metadata.len() > MAX_CONFORMANCE_REPORT_SIZE {
            return Err("conformance report is not a bounded regular file");
        }
        let contents = fs::read(path).map_err(|_| "conformance report could not be read")?;
        let report: ConformanceReport = serde_json::from_slice(&contents)
            .map_err(|_| "conformance report could not be parsed")?;
        report
            .validate_shape()
            .map_err(|_| "conformance report shape is invalid")?;
        if report.harness != harness {
            return Err("conformance report identifies a different harness");
        }
        Ok(report
            .observations
            .into_iter()
            .map(|observation| CanaryObservation {
                kind: match observation.kind {
                    ConformanceObservationKind::InventoryDrift => {
                        CanaryObservationKind::InventoryDrift
                    }
                },
                fingerprint: observation.fingerprint,
            })
            .collect())
    }
}

fn safe_log_name(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' {
                character
            } else {
                '-'
            }
        })
        .collect()
}
