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
    format!(
        "\n\nRecommended fix with nvm:\n  nvm install {}\n  nvm use {}\n  node --version\n  nan {}\n\nIf nvm is unavailable, install Node.js {minimum} or newer with fnm, Volta, asdf, or the official Node.js installer.",
        minimum.major,
        minimum.major,
        kind.binary_name()
    )
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
    let parsed = detected
        .strip_prefix('v')
        .and_then(|value| Version::parse(value.trim()).ok());
    match parsed {
        Some(version) if version >= requirement.minimum_version => Ok(()),
        Some(_) => Err(InstallError::RuntimeUnsupported {
            harness: kind,
            detected,
            minimum: requirement.minimum_version,
            hint,
        }),
        None => Err(InstallError::RuntimeUnparseable {
            harness: kind,
            detected,
            minimum: requirement.minimum_version,
            hint,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{runtime_hint, runtime_requirement};
    use nan_harness_core::HarnessKind;

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

        assert!(hint.contains("nvm install 22"));
        assert!(hint.contains("nvm use 22"));
        assert!(hint.contains("node --version"));
        assert!(hint.contains("nan dsh"));
        assert!(hint.contains("official Node.js installer"));
    }
}
