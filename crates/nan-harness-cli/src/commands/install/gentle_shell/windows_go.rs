use crate::commands::install::InstallError;
use semver::Version;
use std::process::Command;

pub(super) fn check() -> Result<(), InstallError> {
    check_with(cfg!(windows), || {
        // Match Gentle AI's local-only Go policy; probing must not download a toolchain.
        let output = Command::new("go")
            .arg("version")
            .env("GOTOOLCHAIN", "local")
            .env("GOENV", "off")
            .env("GOWORK", "off")
            .env("GOFLAGS", "")
            .output()
            .ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
    })
}

fn check_with(windows: bool, probe: impl FnOnce() -> Option<String>) -> Result<(), InstallError> {
    if !windows {
        return Ok(());
    }
    let version = probe().and_then(|output| {
        output
            .strip_prefix("go version go")?
            .split_whitespace()
            .next()
            .and_then(|value| Version::parse(value).ok())
    });
    if version.is_some_and(|version| version >= Version::new(1, 25, 10)) {
        Ok(())
    } else {
        Err(InstallError::GentleShellGoPrerequisite)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nan_harness_i18n::TerminalMessage;

    #[test]
    fn windows_gentle_requires_supported_local_go() {
        for output in [
            None,
            Some("go version go1.25.9 windows/amd64"),
            Some("unknown"),
            Some("go version go1.25.10rc1 windows/amd64"),
        ] {
            let error = check_with(true, || output.map(str::to_owned)).unwrap_err();
            assert!(error.is_runtime_precondition());
            for locale in [nan_harness_i18n::Locale::En, nan_harness_i18n::Locale::Es] {
                let message = error.terminal_message(locale);
                for required in [
                    "1.25.10",
                    "https://go.dev/dl/",
                    "go version",
                    "nanh gentle-shell",
                ] {
                    assert!(message.contains(required), "{message}");
                }
                assert!(!message.contains("nanh pi"));
            }
        }
        for version in ["1.25.10", "1.25.11", "1.26.0"] {
            check_with(true, || {
                Some(format!("go version go{version} windows/amd64\n"))
            })
            .unwrap();
        }
    }

    #[test]
    fn unix_gentle_never_probes_go() {
        check_with(false, || {
            panic!("Go is only a Windows installation prerequisite")
        })
        .unwrap();
    }
}
