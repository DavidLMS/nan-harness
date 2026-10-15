use super::persistence::{OpenCodeInstallation, PreferencesStore};
use crate::error::CliError;
use nan_harness_core::{HarnessKind, PlanError};
use nan_harness_runtime::{
    DiscoveryOptions, inspect_harness, is_executable_file, locate_harness_executable,
};
use std::collections::BTreeSet;
use std::io::{self, IsTerminal, Write as _};
use std::path::{Path, PathBuf};

pub(crate) fn selection_error() -> CliError {
    CliError::InvalidPlan(PlanError::InvalidField {
        field: "opencode",
        message: nan_harness_i18n::DiagnosticText::new(
            nan_harness_i18n::messages::opencode_selection_error,
        ),
    })
}

pub(crate) fn major_at(path: &Path) -> Result<u8, CliError> {
    let report = inspect_harness(
        HarnessKind::OpenCode,
        path,
        DiscoveryOptions {
            allow_unsupported: true,
            allow_untested: true,
        },
    )?;
    version_major(&report.harness.detected_version).ok_or_else(selection_error)
}

fn version_major(output: &str) -> Option<u8> {
    output.split_whitespace().find_map(|token| {
        let version = token
            .rsplit('/')
            .next()?
            .trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '.')
            .trim_start_matches('v');
        match semver::Version::parse(version).ok()?.major {
            1 => Some(1),
            2 => Some(2),
            _ => None,
        }
    })
}

pub(crate) fn resolve(
    executable: Option<&Path>,
    major: Option<u8>,
    dry_run: bool,
    interactive: bool,
) -> Result<Option<PathBuf>, CliError> {
    if let Some(path) = executable {
        let path = locate_harness_executable(HarnessKind::OpenCode, Some(path))?;
        let actual = major_at(&path)?;
        return if major.is_none_or(|major| major == actual) {
            Ok(Some(path))
        } else {
            Err(selection_error())
        };
    }
    let store = PreferencesStore::from_environment()?;
    let saved = store.opencode_installation()?;
    let mut candidates = discover();
    if let Some(saved) = &saved
        && !candidates
            .iter()
            .any(|candidate| candidate.executable == saved.executable)
        && is_executable_file(&saved.executable)
        && major_at(&saved.executable).ok() == Some(saved.major)
    {
        candidates.insert(0, saved.clone());
    }
    candidates.retain(|candidate| major.is_none_or(|major| major == candidate.major));
    if let Some(candidate) = preferred_candidate(&candidates, saved.as_ref()) {
        return Ok(Some(candidate.executable.clone()));
    }
    if candidates.is_empty() {
        if dry_run {
            eprintln!("{}", nan_harness_i18n::messages::discovery_was_not_found_on_path_dry_run_does_not_install_harnesses(nan_harness_i18n::locale(), &HarnessKind::OpenCode));
            return Ok(None);
        }
        if !interactive {
            return Err(selection_error());
        }
        return super::install::opencode::offer(major.unwrap_or(2)).map_err(Into::into);
    }
    if !interactive || !io::stdin().is_terminal() || !io::stderr().is_terminal() || dry_run {
        return Err(selection_error());
    }
    eprintln!(
        "{}",
        nan_harness_i18n::messages::opencode_selection_prompt(nan_harness_i18n::locale())
    );
    for (index, candidate) in candidates.iter().enumerate() {
        let report = inspect_harness(
            HarnessKind::OpenCode,
            &candidate.executable,
            DiscoveryOptions {
                allow_unsupported: true,
                allow_untested: true,
            },
        )?;
        eprintln!(
            "  {}. {} — {}",
            index + 1,
            report.harness.detected_version,
            candidate.executable.display()
        );
    }
    eprint!("> ");
    io::stderr()
        .flush()
        .map_err(super::install::InstallError::Prompt)?;
    let mut response = String::new();
    io::stdin()
        .read_line(&mut response)
        .map_err(super::install::InstallError::Prompt)?;
    let candidate = response
        .trim()
        .parse::<usize>()
        .ok()
        .and_then(|index| index.checked_sub(1))
        .and_then(|index| candidates.get(index))
        .ok_or_else(selection_error)?;
    store.set_opencode_installation(candidate.clone())?;
    Ok(Some(candidate.executable.clone()))
}

fn preferred_candidate<'a>(
    candidates: &'a [OpenCodeInstallation],
    saved: Option<&OpenCodeInstallation>,
) -> Option<&'a OpenCodeInstallation> {
    if let Some(saved) = saved
        && let Some(candidate) = candidates.iter().find(|candidate| *candidate == saved)
    {
        return Some(candidate);
    }
    let first = candidates.first()?;
    candidates
        .iter()
        .all(|candidate| candidate.major == first.major)
        .then_some(first)
}

fn discover() -> Vec<OpenCodeInstallation> {
    let mut paths = Vec::new();
    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            for name in ["opencode", "opencode2"] {
                paths.push(directory.join(name));
                if cfg!(windows) {
                    paths.push(directory.join(format!("{name}.cmd")));
                    paths.push(directory.join(format!("{name}.exe")));
                }
            }
        }
    }
    paths.extend(super::install::opencode_known_locations());
    for major in [1, 2] {
        if let Some(path) = super::install::opencode::installed(major) {
            paths.push(path);
        }
    }
    let mut seen = BTreeSet::new();
    paths
        .into_iter()
        .filter(|path| is_executable_file(path))
        .filter_map(|path| {
            let canonical = std::fs::canonicalize(&path).ok()?;
            if !seen.insert(canonical) {
                return None;
            }
            major_at(&path).ok().map(|major| OpenCodeInstallation {
                major,
                executable: path,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_choice_and_single_generation_selection_are_deterministic() {
        let one = OpenCodeInstallation {
            major: 1,
            executable: PathBuf::from("/v1"),
        };
        let two = OpenCodeInstallation {
            major: 2,
            executable: PathBuf::from("/v2"),
        };
        let another = OpenCodeInstallation {
            major: 2,
            executable: PathBuf::from("/v2-other"),
        };
        assert_eq!(preferred_candidate(&[], None), None);
        assert_eq!(
            preferred_candidate(std::slice::from_ref(&one), None),
            Some(&one)
        );
        assert_eq!(preferred_candidate(&[one.clone(), two.clone()], None), None);
        assert_eq!(
            preferred_candidate(&[one.clone(), two.clone()], Some(&two)),
            Some(&two)
        );
        assert_eq!(
            preferred_candidate(&[two.clone(), another.clone()], None),
            Some(&two)
        );
        assert_eq!(
            preferred_candidate(&[two.clone(), another.clone()], Some(&another)),
            Some(&another)
        );
        assert_eq!(
            preferred_candidate(std::slice::from_ref(&two), Some(&one)),
            Some(&two)
        );
    }

    #[test]
    fn official_version_shapes_identify_generation() {
        for (output, expected) in [
            ("opencode v2.0.27", Some(2)),
            ("2.0.29", Some(2)),
            ("opencode/1.18.29", Some(1)),
            ("development", None),
            ("3.0.0", None),
        ] {
            assert_eq!(version_major(output), expected, "{output}");
        }
    }

    #[test]
    fn parser_limits_major_and_alias_conflicts() {
        use crate::app::Cli;
        for args in [
            vec!["nanh", "opencode", "--major", "1"],
            vec!["nanh", "opencode2"],
            vec!["nanh", "config", "opencode2", "--major", "2"],
        ] {
            assert!(Cli::try_parse_checked_from(args).is_ok());
        }
        for args in [
            vec!["nanh", "opencode", "--major", "3"],
            vec!["nanh", "opencode2", "--major", "1"],
            vec!["nanh", "config", "opencode2", "--major", "1"],
            vec!["nanh", "config", "codex", "--major", "2"],
        ] {
            assert!(Cli::try_parse_checked_from(args).is_err());
        }
    }

    #[cfg(unix)]
    #[test]
    fn explicit_executable_must_match_requested_generation() {
        use std::os::unix::fs::PermissionsExt as _;
        let temporary = tempfile::tempdir().unwrap();
        let executable = temporary.path().join("misleading-opencode2-name");
        std::fs::write(&executable, "#!/bin/sh\necho 'opencode v1.18.29'\n").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            resolve(Some(&executable), Some(1), true, false).unwrap(),
            Some(executable.clone())
        );
        assert!(resolve(Some(&executable), Some(2), true, false).is_err());
    }
}
