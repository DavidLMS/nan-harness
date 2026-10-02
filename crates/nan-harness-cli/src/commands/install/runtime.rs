use super::error::InstallError;
use super::output::first_non_empty_output_line;
use nan_harness_core::{HarnessKind, RuntimeCompatibility};
use nan_harness_runtime::bundled_compatibility_manifest;
use semver::Version;
use std::process::Command;

fn runtime_requirement(kind: HarnessKind) -> Result<Option<RuntimeCompatibility>, InstallError> {
    let manifest = bundled_compatibility_manifest()
        .map_err(|error| InstallError::CompatibilityManifest(error.to_string()))?;
    Ok(manifest.entry(kind).and_then(|entry| entry.runtime.clone()))
}

fn runtime_command(
    kind: HarnessKind,
    requirement: &RuntimeCompatibility,
) -> Result<(String, Vec<String>), InstallError> {
    let mut parts = requirement.command.split_ascii_whitespace();
    let Some(program) = parts.next() else {
        return Err(InstallError::InvalidRuntimeCommand {
            harness: kind,
            command: requirement.command.clone(),
        });
    };
    let arguments = parts.map(ToOwned::to_owned).collect::<Vec<_>>();
    if arguments.is_empty() {
        return Err(InstallError::InvalidRuntimeCommand {
            harness: kind,
            command: requirement.command.clone(),
        });
    }
    Ok((program.to_owned(), arguments))
}

pub(super) fn runtime_hint(kind: HarnessKind, minimum: &Version) -> String {
    runtime_hint_for(kind, minimum, nan_harness_i18n::Locale::En)
}

pub(super) fn runtime_hint_for(
    kind: HarnessKind,
    minimum: &Version,
    locale: nan_harness_i18n::Locale,
) -> String {
    if cfg!(windows) {
        return nan_harness_i18n::messages::install_runtime_windows(
            locale,
            kind.binary_name(),
            minimum,
        );
    }
    let nvm_dir = std::env::var_os("NVM_DIR");
    let path = std::env::var_os("PATH");
    unix_runtime_hint(
        kind,
        minimum,
        locale,
        nvm_dir.as_deref(),
        path.as_deref(),
        cfg!(target_os = "macos"),
    )
}

fn unix_runtime_hint(
    kind: HarnessKind,
    minimum: &Version,
    locale: nan_harness_i18n::Locale,
    nvm_dir: Option<&std::ffi::OsStr>,
    path: Option<&std::ffi::OsStr>,
    macos: bool,
) -> String {
    if nvm_dir.is_some_and(|path| {
        let directory = std::path::Path::new(path);
        directory.is_absolute() && directory.join("nvm.sh").is_file()
    }) {
        nan_harness_i18n::messages::install_runtime_nvm(
            locale,
            kind.binary_name(),
            &minimum.major,
            minimum,
        )
    } else if macos
        && path.is_some_and(|path| {
            std::env::split_paths(path)
                .any(|directory| nan_harness_runtime::is_executable_file(&directory.join("brew")))
        })
    {
        nan_harness_i18n::messages::install_runtime_homebrew(
            locale,
            kind.binary_name(),
            &minimum.major,
            minimum,
        )
    } else {
        nan_harness_i18n::messages::install_runtime_official(locale, kind.binary_name(), minimum)
    }
}

pub(crate) fn check_required_runtime(kind: HarnessKind) -> Result<(), InstallError> {
    let Some(requirement) = runtime_requirement(kind)? else {
        return Ok(());
    };
    let (program, arguments) = runtime_command(kind, &requirement)?;
    let command = format!("{program} {}", arguments.join(" "));
    let hint = runtime_hint(kind, &requirement.minimum_version);
    let output = Command::new(&program)
        .args(&arguments)
        .output()
        .map_err(|source| InstallError::RuntimeCommandStart {
            harness: kind,
            command: command.clone(),
            minimum: requirement.minimum_version.clone(),
            hint: hint.clone(),
            source,
        })?;
    if !output.status.success() {
        return Err(InstallError::RuntimeCommandFailed {
            harness: kind,
            command,
            minimum: requirement.minimum_version,
            exit_code: output.status.code(),
            hint,
        });
    }

    let detected = first_non_empty_output_line(&output);
    validate_runtime_version(kind, &requirement, detected)
}

fn validate_runtime_version(
    kind: HarnessKind,
    requirement: &RuntimeCompatibility,
    detected: String,
) -> Result<(), InstallError> {
    let hint = runtime_hint(kind, &requirement.minimum_version);
    let parsed = detected
        .strip_prefix('v')
        .and_then(|value| Version::parse(value.trim()).ok());
    match parsed {
        Some(version) if version >= requirement.minimum_version => Ok(()),
        Some(_) => Err(InstallError::RuntimeUnsupported {
            harness: kind,
            detected,
            minimum: requirement.minimum_version.clone(),
            hint,
        }),
        None => Err(InstallError::RuntimeUnparseable {
            harness: kind,
            detected,
            minimum: requirement.minimum_version.clone(),
            hint,
        }),
    }
}

pub(super) fn npm_hint_for(kind: HarnessKind, locale: nan_harness_i18n::Locale) -> String {
    use nan_harness_i18n::messages as m;
    if cfg!(windows) {
        m::install_npm_windows(locale, kind.binary_name(), &kind)
    } else if cfg!(target_os = "macos") {
        m::install_npm_macos(locale, kind.binary_name(), &kind)
    } else {
        m::install_npm_linux(locale, kind.binary_name(), &kind)
    }
}

#[cfg(test)]
mod tests {
    use super::{runtime_hint, runtime_requirement};
    use nan_harness_core::HarnessKind;

    #[test]
    fn unix_recovery_requires_installed_tools_and_loads_nvm_before_using_it() {
        let directory = tempfile::tempdir().expect("temporary tool directory");
        let minimum = semver::Version::new(24, 14, 0);
        let hint = |nvm, path, macos| {
            super::unix_runtime_hint(
                HarnessKind::ZCode,
                &minimum,
                nan_harness_i18n::Locale::En,
                nvm,
                path,
                macos,
            )
        };
        let fallback = hint(None, None, true);
        assert!(fallback.contains("https://nodejs.org/en/download"));
        assert!(!fallback.contains("nvm install"));
        assert!(!fallback.contains("brew install"));
        let nvm = Some(directory.path().as_os_str());
        assert_eq!(hint(nvm, None, true), fallback);
        std::fs::write(directory.path().join("nvm.sh"), "# synthetic nvm").expect("nvm script");
        let installed = hint(nvm, None, false);
        assert!(installed.contains(". \"$NVM_DIR/nvm.sh\"\n  nvm install 24"));
        assert!(installed.contains("nvm use 24"));
        let brew = directory.path().join("brew");
        std::fs::write(&brew, "#!/bin/sh\n").expect("synthetic Homebrew");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&brew, std::fs::Permissions::from_mode(0o755))
                .expect("Homebrew executable");
        }
        let path = Some(directory.path().as_os_str());
        let homebrew = hint(None, path, true);
        assert!(homebrew.contains("brew install node@24"));
        assert!(homebrew.contains("export PATH=\"$(brew --prefix node@24)/bin:$PATH\""));
        assert!(!homebrew.contains("nvm install"));
        assert_eq!(hint(None, path, false), fallback);
        assert_eq!(hint(nvm, path, true), installed);
    }

    #[test]
    fn deepseek_harness_declares_the_node_runtime_requirement() {
        let requirement = runtime_requirement(HarnessKind::DeepSeekHarness)
            .expect("embedded compatibility manifest should be valid")
            .expect("DeepSeek Harness should declare a runtime");

        assert_eq!(requirement.command, "node --version");
        assert_eq!(requirement.minimum_version.to_string(), "22.19.0");
    }

    #[test]
    fn runtime_hint_explains_how_to_recover_and_retry() {
        let hint = runtime_hint(
            HarnessKind::DeepSeekHarness,
            &semver::Version::new(22, 19, 0),
        );

        if cfg!(windows) {
            assert!(hint.contains("https://nodejs.org/en/download"));
            assert!(hint.contains("where.exe node"));
            assert!(hint.contains("Open a new terminal"));
        }
        assert!(hint.contains("node --version"));
        assert!(hint.contains("nanh dsh"));
    }

    #[test]
    fn pi_rejects_old_node_with_recovery_guidance_and_accepts_supported_versions() {
        let requirement = runtime_requirement(HarnessKind::Pi)
            .expect("embedded manifest should be valid")
            .expect("Pi should require Node.js");
        assert_eq!(requirement.command, "node --version");
        assert_eq!(requirement.minimum_version, semver::Version::new(22, 19, 0));
        let error =
            super::validate_runtime_version(HarnessKind::Pi, &requirement, "v22.14.0".to_owned())
                .expect_err("Node without Pi's required APIs must be rejected");
        assert!(matches!(error, super::InstallError::RuntimeUnsupported {
            harness: HarnessKind::Pi, detected, minimum, hint,
        } if detected == "v22.14.0" && minimum == requirement.minimum_version
            && hint.contains("node --version") && hint.contains("nanh pi")));
        for version in ["v22.19.0", "v22.20.0", "v24.0.0"] {
            super::validate_runtime_version(HarnessKind::Pi, &requirement, version.to_owned())
                .expect("supported Node should pass");
        }
        assert!(matches!(
            super::validate_runtime_version(HarnessKind::Pi, &requirement, "unknown".to_owned(),),
            Err(super::InstallError::RuntimeUnparseable { .. })
        ));
    }
}
