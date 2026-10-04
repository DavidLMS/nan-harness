//! Admit one official native project argument inside an owned hosted trial.
use super::super::ChatGptDesktopError;
use super::super::installation::ChatGptInstallation;
use super::super::profile::ManagedProfile;
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{Error, ErrorKind, Read as _};
use std::path::{Path, PathBuf};
use tokio::process::Command;

const WINDOWS_ARTIFACT: &str = "f7b0266d6c00d4743da01d62bc82488f7ec5560c642501758119cb9885f67c87";
const WINDOWS_EXECUTABLE: &str = "b35bf062c01d73da090c60e62186dc180c2a8545cb6fc9575b4403c8fa3db49e";
const CURRENT_UNIX_VERSION: &str = "26.930.41038";
const LINUX_ARTIFACT: &str = "ee7854145554718d7239d01ea37d44f6ba1e0ba4a93f47ac097d6e0f964da47c";
const LINUX_EXECUTABLE: &str = "207c4fbff7e2fcc1b0789448351ac6eed206206d94c5a0835e5f07c7cd73d6e3";
const VERSION: &str = "26.930.31730";
const MACOS_ARTIFACT: &str = "f6cf4d2e9b69aeefa33adda4bcd1a2d306357f5253a1ac6049700870c28dd0c7";
const MACOS_EXECUTABLE: &str = "418a460276b195f5642e43b320ec2821d6c34c646cb316ed2c0285546298243f";
const POLICY_KEYS: [&str; 7] = [
    "GITHUB_ACTIONS",
    "RUNNER_ENVIRONMENT",
    "RUNNER_OS",
    "NANH_DESKTOP_QUALIFICATION_MODE",
    "NANH_CODEX_PUBLIC_ONBOARDING",
    "NANH_CODEX_PROJECT_POLICY",
    "NANH_DESKTOP_RENDERER_APP",
];

fn inspected_target(platform: &str) -> Option<(&'static str, &'static str, &'static str)> {
    match platform {
        "windows" => Some(("Windows", WINDOWS_ARTIFACT, WINDOWS_EXECUTABLE)),
        "linux" => Some(("Linux", LINUX_ARTIFACT, LINUX_EXECUTABLE)),
        "macos" => Some(("macOS", MACOS_ARTIFACT, MACOS_EXECUTABLE)),
        _ => None,
    }
}

fn inspected_release(platform: &str, version: &str, artifact: &str) -> Option<&'static str> {
    let (_, expected_artifact, executable) = inspected_target(platform)?;
    let expected_version = if platform == "windows" {
        VERSION
    } else {
        CURRENT_UNIX_VERSION
    };
    (version == expected_version && artifact == expected_artifact).then_some(executable)
}

fn admitted(platform: &str, debug: bool, environment: &BTreeMap<&str, String>) -> bool {
    let Some((runner_os, _, _)) = inspected_target(platform) else {
        return false;
    };
    !debug
        && POLICY_KEYS
            .iter()
            .zip([
                "true",
                "github-hosted",
                runner_os,
                "renderer",
                "engineering",
                "open-project",
                "chatgpt-desktop",
            ])
            .all(|(key, expected)| environment.get(key).is_some_and(|value| value == expected))
}

fn denied() -> Error {
    // Deliberately exclude paths, application output and OS error messages.
    Error::from(ErrorKind::PermissionDenied)
}

fn private_directory(path: &Path) -> std::io::Result<PathBuf> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| denied())?;
    if !path.is_absolute() || !metadata.is_dir() || metadata.is_symlink() {
        return Err(denied());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(denied());
        }
    }
    #[cfg(not(unix))]
    nan_harness_private_fs::restrict_path(path, nan_harness_private_fs::PrivatePathKind::Directory)
        .map_err(|_| denied())?;
    path.canonicalize().map_err(|_| denied())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
enum Stage {
    Policy,
    Release,
    Facts,
    Electron,
    Workspace,
    RootComponent,
    ProfileBinding,
    Fixture,
    ExecutableHash,
}

fn record_failure(stage: Stage, debug: bool) {
    let expected = inspected_target(std::env::consts::OS).map(|target| target.0);
    if debug
        || expected.is_none()
        || std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
        || expected != std::env::var("RUNNER_OS").ok().as_deref()
    {
        return;
    }
    let Some(directory) = std::env::var_os("NANH_DESKTOP_QUALIFICATION_FACTS").map(PathBuf::from)
    else {
        return;
    };
    if private_directory(&directory).ok().as_ref() != Some(&directory) {
        return;
    }
    let value = serde_json::json!({"schemaVersion":1,"mechanism":"codex-project-preflight",
        "diagnosticsOnly":true,"stage":stage});
    let path = directory.join(format!(
        "codex-project-preflight-{}.json",
        std::process::id()
    ));
    if let Ok(bytes) = serde_json::to_vec(&value)
        && let Ok(mut file) = nan_harness_private_fs::open_private_new(&path)
    {
        use std::io::Write as _;
        let _ = file.write_all(&bytes).and_then(|()| file.sync_all());
    }
}

#[cfg(test)]
fn workspace(
    cwd: &Path,
    profile: &Path,
    facts: &Path,
    electron: &Path,
) -> std::io::Result<PathBuf> {
    workspace_staged(cwd, profile, facts, electron, &mut Stage::Workspace)
}

fn workspace_staged(
    cwd: &Path,
    profile: &Path,
    facts: &Path,
    electron: &Path,
    stage: &mut Stage,
) -> std::io::Result<PathBuf> {
    *stage = Stage::Workspace;
    let cwd = private_directory(cwd)?;
    let mut expected = cwd.clone();
    *stage = Stage::RootComponent;
    for component in ["profile", "nanh", "chatgpt-desktop", "profile"] {
        expected.push(component);
        private_directory(&expected)?;
    }
    *stage = Stage::ProfileBinding;
    if private_directory(profile)? != expected.canonicalize().map_err(|_| denied())?
        || private_directory(electron)? != private_directory(&cwd.join("profile/codex-desktop"))?
    {
        return Err(denied());
    }
    *stage = Stage::Facts;
    private_directory(facts)?;
    *stage = Stage::Fixture;
    let fixture = cwd.join("read-target.txt");
    let metadata = std::fs::symlink_metadata(&fixture).map_err(|_| denied())?;
    if !metadata.is_file()
        || metadata.is_symlink()
        || fixture.canonicalize().map_err(|_| denied())?.parent() != Some(cwd.as_path())
    {
        return Err(denied());
    }
    // Opening validates private ownership/access, but never reads fixture text.
    nan_harness_private_fs::open_private_read(&fixture).map_err(|_| denied())?;
    Ok(cwd)
}

fn executable_digest(path: &Path) -> std::io::Result<String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| denied())?;
    if !path.is_absolute()
        || !metadata.is_file()
        || metadata.is_symlink()
        || metadata.len() > 1024 * 1024 * 1024
    {
        return Err(denied());
    }
    let file = std::fs::File::open(path).map_err(|_| denied())?;
    let mut file = file.take(1024 * 1024 * 1024 + 1);
    let mut total = 0_u64;
    let mut digest = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|_| denied())?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > 1024 * 1024 * 1024 {
            return Err(denied());
        }
        digest.update(&buffer[..count]);
    }
    let mut encoded = String::with_capacity(64);
    for byte in digest.finalize() {
        write!(&mut encoded, "{byte:02x}").map_err(|_| denied())?;
    }
    Ok(encoded)
}

pub(super) fn apply(
    command: &mut Command,
    installation: &ChatGptInstallation,
    profile: &ManagedProfile,
    debug: bool,
) -> Result<(), ChatGptDesktopError> {
    if std::env::var_os("NANH_CODEX_PROJECT_POLICY").is_none() {
        return Ok(());
    }
    let environment: BTreeMap<_, _> = POLICY_KEYS
        .iter()
        .map(|&key| (key, std::env::var(key).unwrap_or_default()))
        .collect();
    let mut stage = Stage::Policy;
    let outcome = (|| {
        if !admitted(std::env::consts::OS, debug, &environment) {
            return Err(ChatGptDesktopError::InvalidInstallation);
        }
        stage = Stage::Release;
        let executable = inspected_release(
            std::env::consts::OS,
            &installation.app_version.to_string(),
            &std::env::var("NANH_CODEX_PROJECT_ARTIFACT_SHA256").unwrap_or_default(),
        )
        .ok_or(ChatGptDesktopError::InvalidInstallation)?;
        stage = Stage::Facts;
        let facts = std::env::var_os("NANH_DESKTOP_QUALIFICATION_FACTS")
            .map(PathBuf::from)
            .ok_or(ChatGptDesktopError::InvalidInstallation)?;
        stage = Stage::Electron;
        let electron = std::env::var_os("CODEX_ELECTRON_USER_DATA_PATH")
            .map(PathBuf::from)
            .ok_or(ChatGptDesktopError::InvalidInstallation)?;
        stage = Stage::Workspace;
        let cwd = std::env::current_dir().map_err(|_| ChatGptDesktopError::InvalidInstallation)?;
        let cwd = workspace_staged(&cwd, &profile.root, &facts, &electron, &mut stage)
            .map_err(|_| ChatGptDesktopError::InvalidInstallation)?;
        stage = Stage::ExecutableHash;
        if executable_digest(&installation.executable)
            .map_err(|_| ChatGptDesktopError::InvalidInstallation)?
            != executable
        {
            return Err(ChatGptDesktopError::InvalidInstallation);
        }
        Ok(cwd)
    })();
    let cwd = match outcome {
        Ok(cwd) => cwd,
        Err(error) => {
            record_failure(stage, debug);
            return Err(error);
        }
    };
    // Chromium does not expose the web subtree on macOS until accessibility
    // is enabled. Scope this launch flag to the fully admitted owned trial.
    #[cfg(target_os = "macos")]
    command.arg("--force-renderer-accessibility");
    command
        .arg("--open-project")
        .arg(cwd)
        .env_remove("NANH_CODEX_PROJECT_ARTIFACT_SHA256");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn private_tempdir() -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        nan_harness_private_fs::restrict_path(
            directory.path(),
            nan_harness_private_fs::PrivatePathKind::Directory,
        )
        .unwrap();
        directory
    }

    #[test]
    fn hosted_policy_requires_all_public_trial_conditions() {
        for (platform, runner_os) in [
            ("windows", "Windows"),
            ("linux", "Linux"),
            ("macos", "macOS"),
        ] {
            let mut environment: BTreeMap<_, _> = POLICY_KEYS
                .iter()
                .copied()
                .zip(
                    [
                        "true",
                        "github-hosted",
                        runner_os,
                        "renderer",
                        "engineering",
                        "open-project",
                        "chatgpt-desktop",
                    ]
                    .map(str::to_owned),
                )
                .collect();
            assert!(admitted(platform, false, &environment));
            assert!(!admitted(platform, true, &environment));
            assert!(!admitted("freebsd", false, &environment));
            let foreign = if platform == "linux" {
                "windows"
            } else {
                "linux"
            };
            assert!(!admitted(foreign, false, &environment));
            for key in POLICY_KEYS {
                let original = environment.remove(key).unwrap();
                assert!(!admitted(platform, false, &environment));
                environment.insert(key, "unknown".into());
                assert!(!admitted(platform, false, &environment));
                environment.insert(key, original);
            }
        }
    }

    #[test]
    fn inspected_release_binds_each_platform_to_its_exact_binary() {
        for (platform, artifact, executable, foreign_artifact) in [
            (
                "windows",
                WINDOWS_ARTIFACT,
                WINDOWS_EXECUTABLE,
                LINUX_ARTIFACT,
            ),
            ("linux", LINUX_ARTIFACT, LINUX_EXECUTABLE, WINDOWS_ARTIFACT),
            ("macos", MACOS_ARTIFACT, MACOS_EXECUTABLE, WINDOWS_ARTIFACT),
        ] {
            assert_eq!(
                inspected_release(
                    platform,
                    if platform == "windows" {
                        VERSION
                    } else {
                        CURRENT_UNIX_VERSION
                    },
                    artifact
                ),
                Some(executable)
            );
            for version in [
                "",
                "26.930.21537",
                "26.930.31731",
                if platform == "windows" {
                    CURRENT_UNIX_VERSION
                } else {
                    VERSION
                },
            ] {
                assert_eq!(inspected_release(platform, version, artifact), None);
            }
            for artifact in ["", "unknown", foreign_artifact] {
                assert_eq!(
                    inspected_release(
                        platform,
                        if platform == "windows" {
                            VERSION
                        } else {
                            CURRENT_UNIX_VERSION
                        },
                        artifact
                    ),
                    None
                );
            }
            assert_eq!(inspected_release("freebsd", VERSION, artifact), None);
        }
    }
    #[test]
    fn workspace_admission_requires_exact_private_profile_and_fixture() {
        let directory = private_tempdir();
        let cwd = directory.path();
        let profile = cwd.join("profile/nanh/chatgpt-desktop/profile");
        let electron = cwd.join("profile/codex-desktop");
        let facts = cwd.join("facts");
        nan_harness_private_fs::create_private_dir_all(&profile).unwrap();
        nan_harness_private_fs::create_private_dir_all(&electron).unwrap();
        nan_harness_private_fs::create_private_dir_all(&facts).unwrap();
        assert!(workspace(cwd, &profile, &facts, &electron).is_err());
        nan_harness_private_fs::open_private_new(&cwd.join("read-target.txt")).unwrap();
        assert_eq!(
            workspace(cwd, &profile, &facts, &electron).unwrap(),
            cwd.canonicalize().unwrap()
        );
        let outside = private_tempdir();
        let mut stage = Stage::Policy;
        assert!(workspace_staged(cwd, outside.path(), &facts, &electron, &mut stage).is_err());
        assert_eq!(stage, Stage::ProfileBinding);
        assert!(workspace(cwd, outside.path(), &facts, &electron).is_err());
        assert!(workspace(cwd, &profile, &facts, outside.path()).is_err());
        std::fs::remove_file(cwd.join("read-target.txt")).unwrap();
        std::fs::create_dir(cwd.join("read-target.txt")).unwrap();
        assert!(workspace(cwd, &profile, &facts, &electron).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn shared_parent_or_symlink_cannot_admit_owned_workspace() {
        use std::os::unix::{fs::PermissionsExt as _, fs::symlink};
        let directory = private_tempdir();
        let profile = directory
            .path()
            .join("profile/nanh/chatgpt-desktop/profile");
        nan_harness_private_fs::create_private_dir_all(&profile).unwrap();
        let electron = directory.path().join("profile/codex-desktop");
        nan_harness_private_fs::create_private_dir_all(&electron).unwrap();
        let fixture = directory.path().join("read-target.txt");
        nan_harness_private_fs::open_private_new(&fixture).unwrap();
        assert!(workspace(directory.path(), &profile, directory.path(), &electron).is_ok());
        let parent = directory.path().join("profile/nanh");
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut stage = Stage::Policy;
        assert!(
            workspace_staged(
                directory.path(),
                &profile,
                directory.path(),
                &electron,
                &mut stage
            )
            .is_err()
        );
        assert_eq!(stage, Stage::RootComponent);
        assert_eq!(serde_json::to_value(stage).unwrap(), "root-component");
        assert!(workspace(directory.path(), &profile, directory.path(), &electron).is_err());
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::remove_file(&fixture).unwrap();
        let outside = private_tempdir();
        let foreign = outside.path().join("read-target.txt");
        nan_harness_private_fs::open_private_new(&foreign).unwrap();
        symlink(foreign, fixture).unwrap();
        assert!(
            workspace_staged(
                directory.path(),
                &profile,
                directory.path(),
                &electron,
                &mut stage
            )
            .is_err()
        );
        assert_eq!(stage, Stage::Fixture);
        assert!(workspace(directory.path(), &profile, directory.path(), &electron).is_err());
        let target = directory.path().join("target");
        nan_harness_private_fs::create_private_dir_all(&target).unwrap();
        let link = directory.path().join("link");
        symlink(target, &link).unwrap();
        assert!(private_directory(&link).is_err());
    }
}
