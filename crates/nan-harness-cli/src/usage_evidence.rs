use nan_harness_private_fs::open_private_new;
use nan_harness_runtime::{ExecutionOutcome, ExecutionReport};
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use tempfile::Builder as TempFileBuilder;
use thiserror::Error;

pub(crate) const INTERNAL_CANARY_USAGE_FILE: &str = "NAN_HARNESS_INTERNAL_CANARY_USAGE_FILE";

#[derive(Debug, Error)]
#[error("could not write private usage evidence")]
pub(crate) struct UsageEvidenceError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UsageEvidenceStatus {
    Observed,
    NotObserved,
    Unsupported,
}

impl UsageEvidenceStatus {
    const fn payload(self) -> &'static [u8] {
        match self {
            Self::Observed => b"{\"schemaVersion\":1,\"status\":\"observed\"}\n",
            Self::NotObserved => b"{\"schemaVersion\":1,\"status\":\"not-observed\"}\n",
            Self::Unsupported => b"{\"schemaVersion\":1,\"status\":\"unsupported\"}\n",
        }
    }
}

pub(crate) fn write_if_configured(report: &ExecutionReport) -> Result<(), UsageEvidenceError> {
    if report.outcome != ExecutionOutcome::Succeeded {
        return Ok(());
    }

    write_report(report, env::var_os(INTERNAL_CANARY_USAGE_FILE).as_deref())
}

fn write_report(report: &ExecutionReport, path: Option<&OsStr>) -> Result<(), UsageEvidenceError> {
    if report.outcome != ExecutionOutcome::Succeeded {
        return Ok(());
    }
    let Some(path) = path else {
        return Ok(());
    };

    let status = match report.provider_usage.as_ref() {
        Some(usage) if usage.responses_with_usage() > 0 => UsageEvidenceStatus::Observed,
        Some(_) => UsageEvidenceStatus::NotObserved,
        None => UsageEvidenceStatus::Unsupported,
    };
    write_destination(&PathBuf::from(path), status)
}

fn write_destination(path: &Path, status: UsageEvidenceStatus) -> Result<(), UsageEvidenceError> {
    let destination = canonical_destination(path)?;
    let parent = destination.parent().ok_or(UsageEvidenceError)?;
    let mut temporary = TempFileBuilder::new()
        .prefix(".nan-harness-usage-")
        .make_in(parent, open_private_new)
        .map_err(|_| UsageEvidenceError)?;
    temporary
        .write_all(status.payload())
        .and_then(|()| temporary.flush())
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|_| UsageEvidenceError)?;
    temporary
        .persist_noclobber(destination)
        .map_err(|_| UsageEvidenceError)?;
    Ok(())
}

fn canonical_destination(path: &Path) -> Result<PathBuf, UsageEvidenceError> {
    if !path.is_absolute() {
        return Err(UsageEvidenceError);
    }
    let file_name = path.file_name().ok_or(UsageEvidenceError)?;
    let parent = path.parent().ok_or(UsageEvidenceError)?;
    let parent = fs::canonicalize(parent).map_err(|_| UsageEvidenceError)?;
    let destination = parent.join(file_name);
    match fs::symlink_metadata(&destination) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(destination),
        Ok(_) | Err(_) => Err(UsageEvidenceError),
    }
}

#[cfg(test)]
mod tests {
    use super::{UsageEvidenceStatus, write_destination, write_report};
    use nan_harness_runtime::{
        ExecutionOutcome, ExecutionReport, ModelUsageSnapshot, ProviderUsageSnapshot,
    };
    use std::collections::BTreeMap;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    fn report(outcome: ExecutionOutcome, observed: Option<bool>) -> ExecutionReport {
        let provider_usage = observed.map(|observed| ProviderUsageSnapshot {
            models: BTreeMap::from([(
                "qwen3.6".to_owned(),
                ModelUsageSnapshot {
                    responses_with_usage: u64::from(observed),
                    responses_without_usage: u64::from(!observed),
                    ..ModelUsageSnapshot::default()
                },
            )]),
        });
        ExecutionReport {
            outcome,
            exit_code: 0,
            temporary_root: None,
            selected_model: None,
            selected_reasoning: None,
            bridge_diagnostics: Vec::new(),
            provider_usage,
        }
    }

    #[test]
    fn absent_configuration_does_not_create_a_file() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let destination = directory.path().join("usage.json");

        write_report(&report(ExecutionOutcome::Succeeded, Some(true)), None)
            .expect("missing configuration should be a no-op");

        assert!(!destination.exists());
    }

    #[test]
    fn writes_the_closed_schema_with_exact_bytes_for_each_status() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        for (observed, expected) in [
            (
                Some(true),
                b"{\"schemaVersion\":1,\"status\":\"observed\"}\n".as_slice(),
            ),
            (
                Some(false),
                b"{\"schemaVersion\":1,\"status\":\"not-observed\"}\n".as_slice(),
            ),
            (
                None,
                b"{\"schemaVersion\":1,\"status\":\"unsupported\"}\n".as_slice(),
            ),
        ] {
            let destination = directory.path().join(format!("{observed:?}.json"));
            write_report(
                &report(ExecutionOutcome::Succeeded, observed),
                Some(destination.as_os_str()),
            )
            .expect("evidence should be written");
            assert_eq!(
                fs::read(&destination).expect("evidence should be readable"),
                expected
            );
        }
    }

    #[test]
    fn rejects_relative_paths_and_missing_parents_without_creating_them() {
        let relative = Path::new("usage.json");
        assert!(write_destination(relative, UsageEvidenceStatus::Observed).is_err());

        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let missing_parent = directory.path().join("missing").join("usage.json");
        assert!(write_destination(&missing_parent, UsageEvidenceStatus::Observed).is_err());
        assert!(
            !missing_parent
                .parent()
                .expect("parent should exist")
                .exists()
        );
    }

    #[test]
    fn rejects_existing_destinations_without_overwriting_them() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let destination = directory.path().join("usage.json");
        fs::write(&destination, b"sentinel").expect("sentinel should be written");

        assert!(write_destination(&destination, UsageEvidenceStatus::Observed).is_err());
        assert_eq!(
            fs::read(&destination).expect("sentinel should remain"),
            b"sentinel"
        );
    }

    #[cfg(unix)]
    #[test]
    fn writes_owner_only_files() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let destination = directory.path().join("usage.json");

        write_destination(&destination, UsageEvidenceStatus::Observed)
            .expect("evidence should be written");

        assert_eq!(
            fs::metadata(destination)
                .expect("evidence should exist")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_destinations_without_touching_the_target() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let target = directory.path().join("target.json");
        let destination = directory.path().join("usage.json");
        fs::write(&target, b"sentinel").expect("target should be written");
        symlink(&target, &destination).expect("symlink should be created");

        assert!(write_destination(&destination, UsageEvidenceStatus::Observed).is_err());
        assert_eq!(
            fs::read(&target).expect("target should remain"),
            b"sentinel"
        );
    }

    #[cfg(windows)]
    #[test]
    fn writes_a_private_windows_file() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let destination = directory.path().join("usage.json");

        write_destination(&destination, UsageEvidenceStatus::Observed)
            .expect("evidence should be written");
        nan_harness_test_support::windows_acl::assert_private_file(&destination)
            .expect("evidence should have a private ACL");
    }

    #[test]
    fn failed_execution_does_not_publish_evidence() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let destination = directory.path().join("usage.json");
        write_report(
            &report(ExecutionOutcome::Failed, Some(true)),
            Some(destination.as_os_str()),
        )
        .expect("failed execution should not attempt evidence");
        assert!(!destination.exists());
    }
}
