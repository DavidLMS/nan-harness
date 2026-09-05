//! Installer release-channel coverage. The reviewed `release_installer` tests own installation
//! behaviour; these tests only assert which release source an installation resolves, and that a
//! freshly installed binary keeps following the recommended release.

use std::fs;
use std::path::{Path, PathBuf};

#[cfg(unix)]
const PUBLISHED_VERSION: &str = "9.9.9";
#[cfg(unix)]
const RECOMMENDED_VERSION: &str = "9.9.8";
#[cfg(unix)]
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
/// GitHub resolves `releases/latest/download` to the recommended release, which is what the
/// release gate deliberately does not move.
const RECOMMENDED_DOWNLOAD_PATH: &str = "releases/latest/download";
const AVAILABLE_FEED_PATH: &str = "releases/download/available";

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn both_installers_default_to_the_recommended_release() {
    for installer in ["install.sh", "install.ps1"] {
        let script = fs::read_to_string(repository_root().join(installer))
            .expect("installer should be readable");
        assert!(
            script.contains(RECOMMENDED_DOWNLOAD_PATH),
            "{installer} should default to the recommended release"
        );
        assert!(
            !script.contains(AVAILABLE_FEED_PATH),
            "{installer} must not install from the available-release feed"
        );
    }
}

#[cfg(unix)]
mod fresh_installation {
    use super::super::fixture::{Fixture, Route, candidate_script, manifest_response};
    use super::{
        AVAILABLE_FEED_PATH, CURRENT_VERSION, PUBLISHED_VERSION, RECOMMENDED_VERSION,
        repository_root,
    };
    use sha2::{Digest as _, Sha256};
    use std::fmt::Write as _;
    use std::process::Command;

    #[cfg(target_arch = "aarch64")]
    #[cfg(target_os = "macos")]
    const ARTIFACT: &str = "nan-harness-aarch64-apple-darwin";
    #[cfg(target_arch = "x86_64")]
    #[cfg(target_os = "macos")]
    const ARTIFACT: &str = "nan-harness-x86_64-apple-darwin";
    #[cfg(target_arch = "aarch64")]
    #[cfg(target_os = "linux")]
    const ARTIFACT: &str = "nan-harness-aarch64-unknown-linux-musl";
    #[cfg(target_arch = "x86_64")]
    #[cfg(target_os = "linux")]
    const ARTIFACT: &str = "nan-harness-x86_64-unknown-linux-musl";

    fn installation_routes() -> Vec<Route> {
        let binary =
            std::fs::read(env!("CARGO_BIN_EXE_nan-harness")).expect("binary should be readable");
        let mut checksum = String::new();
        for byte in Sha256::digest(&binary) {
            write!(checksum, "{byte:02x}").expect("writing to a String cannot fail");
        }
        let candidate = candidate_script(RECOMMENDED_VERSION);
        vec![
            (format!("/{ARTIFACT}"), (200, binary)),
            (
                format!("/{ARTIFACT}.sha256"),
                (200, format!("{checksum}  {ARTIFACT}\n").into_bytes()),
            ),
            (
                "/release-version.txt".to_owned(),
                (200, format!("{CURRENT_VERSION}\n").into_bytes()),
            ),
            (
                "/available.json".to_owned(),
                manifest_response(PUBLISHED_VERSION, &candidate),
            ),
            (
                "/recommended.json".to_owned(),
                manifest_response(RECOMMENDED_VERSION, &candidate),
            ),
        ]
    }

    /// A default installation resolves the recommended release, never the available feed, and the
    /// binary it leaves behind keeps that source for its own startup discovery.
    #[test]
    fn a_default_installation_uses_the_recommended_release() {
        let fixture = Fixture::new(installation_routes());
        let install_directory = fixture.home_path().join("install");
        std::fs::create_dir_all(&install_directory).expect("install directory should exist");

        let mut command = Command::new("sh");
        command.arg(repository_root().join("install.sh"));
        fixture.isolate(&mut command);
        command
            .env("NAN_INSTALL_BASE_URL", fixture.base_url())
            .env("NAN_INSTALL_DIR", &install_directory);
        let output = command.output().expect("installer should start");

        assert!(
            output.status.success(),
            "installer failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let installed = install_directory.join("nan-harness");
        let version = Command::new(&installed)
            .arg("--version")
            .output()
            .expect("installed binary should start");
        assert_eq!(
            String::from_utf8_lossy(&version.stdout).trim(),
            format!("nan-harness {CURRENT_VERSION}")
        );
        for request in fixture.requests() {
            assert!(
                !request.contains(AVAILABLE_FEED_PATH) && request != "/available.json",
                "a fresh installation must not read the available-release feed: {request}"
            );
        }
    }
}
