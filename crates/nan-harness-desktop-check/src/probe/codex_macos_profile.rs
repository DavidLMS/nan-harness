//! Prelaunch custody for a read-only Codex global-state loan. Never authorizes input.
use super::{ProbeSpec, Reason};
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    fs::File,
    os::unix::fs::{MetadataExt as _, OpenOptionsExt as _},
    path::{Path, PathBuf},
    time::Instant,
};

struct HeldDirectory {
    path: PathBuf,
    handle: File,
}
#[derive(PartialEq, Eq)]
struct PreparedCommand {
    program: OsString,
    arguments: Vec<OsString>,
    environment: Vec<(OsString, Option<OsString>)>,
    cwd: Option<PathBuf>,
}
impl PreparedCommand {
    // Reject redirects before creating any profile root.
    fn validate_profile(
        command: &tokio::process::Command,
        workspace: &Path,
        deadline: Instant,
    ) -> Result<(), Reason> {
        let profile = workspace.join("profile");
        let required = [
            ("HOME", profile.join("home")),
            ("XDG_CONFIG_HOME", profile.join("config")),
            ("NAN_HARNESS_CONFIG_DIR", profile.join("nanh")),
            ("CODEX_HOME", profile.join("home/.codex")),
            (
                "CODEX_ELECTRON_USER_DATA_PATH",
                profile.join("codex-desktop"),
            ),
        ];
        let prepared = command.as_std();
        let arguments = prepared.get_args().collect::<Vec<_>>();
        if arguments.len() != 9
            || arguments[1] != "--provider-base-url"
            || arguments[3] != "--model"
            || arguments[5] != "--startup-timeout"
            || arguments[6] != "120"
            || arguments[7] != "--executable"
            || !Path::new(arguments[8]).is_absolute()
        {
            return Err(Reason::IsolationUnavailable);
        }
        if Instant::now() >= deadline
            || prepared.get_current_dir() != Some(workspace)
            || prepared.get_args().next() != Some(std::ffi::OsStr::new("chatgpt-desktop"))
            || required.iter().any(|(key, path)| {
                !prepared
                    .get_envs()
                    .any(|(k, v)| k == *key && v == Some(path.as_os_str()))
            })
        {
            return Err(Reason::IsolationUnavailable);
        }
        Ok(())
    }
    fn from(command: &tokio::process::Command) -> Self {
        let command = command.as_std();
        Self {
            program: command.get_program().to_owned(),
            arguments: command.get_args().map(ToOwned::to_owned).collect(),
            environment: command
                .get_envs()
                .map(|(k, v)| (k.to_owned(), v.map(ToOwned::to_owned)))
                .collect(),
            cwd: command.get_current_dir().map(ToOwned::to_owned),
        }
    }
}
pub(crate) struct FreshCodexMacProfile {
    directories: Vec<HeldDirectory>,
    command: PreparedCommand,
    launched: bool,
}
fn same(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
    a.dev() == b.dev() && a.ino() == b.ino() && a.uid() == b.uid()
}
fn private_directory(path: &Path) -> Result<std::fs::Metadata, Reason> {
    let m = std::fs::symlink_metadata(path).map_err(|_| Reason::IsolationUnavailable)?;
    if !m.is_dir()
        || m.is_symlink()
        || m.mode() & 0o7777 != 0o700
        || m.uid() != nix::unistd::Uid::effective().as_raw()
        || path.canonicalize().ok().as_deref() != Some(path)
    {
        return Err(Reason::IsolationUnavailable);
    }
    Ok(m)
}
fn retain(path: PathBuf) -> Result<HeldDirectory, Reason> {
    let m = private_directory(&path)?;
    let handle = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_DIRECTORY | nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
        .open(&path)
        .map_err(|_| Reason::IsolationUnavailable)?;
    let opened = handle
        .metadata()
        .map_err(|_| Reason::IsolationUnavailable)?;
    if !same(&m, &opened) || !same(&private_directory(&path)?, &opened) {
        return Err(Reason::IsolationUnavailable);
    }
    Ok(HeldDirectory { path, handle })
}
fn directories_valid(directories: &[HeldDirectory], deadline: Instant) -> bool {
    directories.iter().all(|directory| {
        Instant::now() < deadline
            && private_directory(&directory.path)
                .ok()
                .zip(directory.handle.metadata().ok())
                .is_some_and(|(path, handle)| same(&path, &handle))
    }) && Instant::now() < deadline
}
impl FreshCodexMacProfile {
    pub(crate) fn prepare(
        spec: &ProbeSpec,
        command: &tokio::process::Command,
        deadline: Instant,
    ) -> Result<Option<Self>, Reason> {
        if std::env::var("NANH_CODEX_PUBLIC_ONBOARDING").as_deref() != Ok("engineering") {
            return Ok(None);
        }
        if spec.kind != nan_harness_core::DesktopHarnessKind::ChatGpt
            || spec.session != crate::cli::SessionMode::GithubHosted
            || std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
            || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
        {
            return Err(Reason::IsolationUnavailable);
        }
        Self::create_owned(&spec.workspace, command, deadline).map(Some)
    }
    fn create_owned(
        workspace: &Path,
        command: &tokio::process::Command,
        deadline: Instant,
    ) -> Result<Self, Reason> {
        let profile = workspace.join("profile");
        PreparedCommand::validate_profile(command, workspace, deadline)?;
        let mut directories = Vec::new();
        for suffix in ["", "profile", "profile/home", "profile/config"] {
            directories.push(retain(if suffix.is_empty() {
                workspace.to_path_buf()
            } else {
                workspace.join(suffix)
            })?);
        }
        let roots = [
            "profile/nanh",
            "profile/nanh/chatgpt-desktop",
            "profile/nanh/chatgpt-desktop/profile",
            "profile/codex-desktop",
        ];
        // Absence is checked before mutation. Never adopt an existing empty
        // profile; the token creates each root exclusively before launching.
        if roots.iter().any(|suffix| {
            !matches!(std::fs::symlink_metadata(workspace.join(suffix)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound)
        }) || !matches!(std::fs::symlink_metadata(profile.join("home/.codex")),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound)
        {
            return Err(Reason::IsolationUnavailable);
        }
        for (suffix, parent, basename) in [
            (roots[0], 1, "nanh"),
            (roots[1], 4, "chatgpt-desktop"),
            (roots[2], 5, "profile"),
            (roots[3], 1, "codex-desktop"),
        ] {
            if !directories_valid(&directories, deadline) {
                return Err(Reason::IsolationUnavailable);
            }
            let path = workspace.join(suffix);
            // Anchor mutation to the retained original parent, so a pathname
            // redirect cannot create a directory inside a replacement parent.
            nix::sys::stat::mkdirat(
                &directories[parent].handle,
                basename,
                nix::sys::stat::Mode::from_bits_truncate(0o700),
            )
            .map_err(|_| Reason::IsolationUnavailable)?;
            let descriptor = nix::fcntl::openat(
                &directories[parent].handle,
                basename,
                nix::fcntl::OFlag::O_RDONLY
                    | nix::fcntl::OFlag::O_DIRECTORY
                    | nix::fcntl::OFlag::O_NOFOLLOW
                    | nix::fcntl::OFlag::O_CLOEXEC
                    | nix::fcntl::OFlag::O_NONBLOCK,
                nix::sys::stat::Mode::empty(),
            )
            .map_err(|_| Reason::IsolationUnavailable)?;
            let handle = File::from(descriptor);
            let opened = handle
                .metadata()
                .map_err(|_| Reason::IsolationUnavailable)?;
            if !same(&private_directory(&path)?, &opened) {
                return Err(Reason::IsolationUnavailable);
            }
            if !directories_valid(&directories, deadline) {
                return Err(Reason::IsolationUnavailable);
            }
            directories.push(HeldDirectory { path, handle });
        }
        let token = Self {
            directories,
            command: PreparedCommand::from(command),
            launched: false,
        };
        if !token.verifies_owned(deadline) {
            return Err(Reason::IsolationUnavailable);
        }
        Ok(token)
    }
    pub(crate) fn before_launch(
        &mut self,
        command: &tokio::process::Command,
        deadline: Instant,
    ) -> Result<(), Reason> {
        if self.launched
            || self.command != PreparedCommand::from(command)
            || !self.verifies_owned(deadline)
        {
            return Err(Reason::IsolationUnavailable);
        }
        self.launched = true;
        Ok(())
    }
    pub(crate) fn verifies_owned(&self, deadline: Instant) -> bool {
        self.directories.len() == 8 && directories_valid(&self.directories, deadline)
    }
    pub(crate) fn private_request(&self, deadline: Instant) -> Result<Value, Reason> {
        if !self.launched || !self.verifies_owned(deadline) {
            return Err(Reason::IsolationUnavailable);
        }
        let mut records = Vec::new();
        for directory in &self.directories {
            let m = directory
                .handle
                .metadata()
                .map_err(|_| Reason::IsolationUnavailable)?;
            // Private request only: no directory identities enter public facts.
            records.push(json!({"path": directory.path, "device":m.dev().to_string(),
                "inode":m.ino().to_string(), "uid":m.uid(), "mode":m.mode() & 0o7777}));
        }
        Ok(
            json!({"schemaVersion":1,"platform":"macos","directories":records,
            "stateRootIndex":6,"stateBasename":".codex-global-state.json","diagnosticsOnly":true}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        os::unix::fs::{PermissionsExt as _, symlink},
        time::Duration,
    };
    fn fixture() -> (tempfile::TempDir, PathBuf, tokio::process::Command) {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().canonicalize().unwrap();
        std::fs::set_permissions(&workspace, std::fs::Permissions::from_mode(0o700)).unwrap();
        for suffix in ["", "profile", "profile/home", "profile/config"] {
            nan_harness_private_fs::create_private_dir_all(&workspace.join(suffix)).unwrap();
        }
        let mut command = tokio::process::Command::new("synthetic-never-spawned");
        command
            .args([
                "chatgpt-desktop",
                "--provider-base-url",
                "http://127.0.0.1:1/v1",
                "--model",
                "synthetic-model",
                "--startup-timeout",
                "120",
                "--executable",
                "/synthetic/never-spawned",
            ])
            .current_dir(&workspace)
            .env("HOME", workspace.join("profile/home"))
            .env("XDG_CONFIG_HOME", workspace.join("profile/config"))
            .env("NAN_HARNESS_CONFIG_DIR", workspace.join("profile/nanh"))
            .env("CODEX_HOME", workspace.join("profile/home/.codex"))
            .env(
                "CODEX_ELECTRON_USER_DATA_PATH",
                workspace.join("profile/codex-desktop"),
            );
        (temp, workspace, command)
    }
    fn deadline() -> Instant {
        Instant::now() + Duration::from_secs(1)
    }
    #[test]
    fn only_exact_prepared_command_lends_retained_managed_root() {
        let (_temp, workspace, mut command) = fixture();
        let mut token =
            FreshCodexMacProfile::create_owned(&workspace, &command, deadline()).unwrap();
        assert!(token.private_request(deadline()).is_err());
        command.env("CODEX_HOME", workspace.join("profile"));
        assert!(token.before_launch(&command, deadline()).is_err());
        command.env("CODEX_HOME", workspace.join("profile/home/.codex"));
        token.before_launch(&command, deadline()).unwrap();
        let loan = token.private_request(deadline()).unwrap();
        assert_eq!(loan["stateRootIndex"], 6);
        assert_eq!(
            loan["directories"][6]["path"],
            workspace
                .join("profile/nanh/chatgpt-desktop/profile")
                .to_str()
                .unwrap()
        );
        assert!(token.before_launch(&command, deadline()).is_err());
    }
    #[test]
    fn populated_roots_are_never_adopted_or_modified() {
        let (_temp, workspace, command) = fixture();
        let payload =
            workspace.join("profile/nanh/chatgpt-desktop/profile/.codex-global-state.json");
        nan_harness_private_fs::create_private_dir_all(payload.parent().unwrap()).unwrap();
        std::fs::write(&payload, b"synthetic existing state").unwrap();
        assert!(FreshCodexMacProfile::create_owned(&workspace, &command, deadline()).is_err());
        assert_eq!(std::fs::read(payload).unwrap(), b"synthetic existing state");
    }
    #[test]
    fn replaced_parent_redirect_and_mode_revoke_loan() {
        let (_temp, workspace, command) = fixture();
        let mut token =
            FreshCodexMacProfile::create_owned(&workspace, &command, deadline()).unwrap();
        token.before_launch(&command, deadline()).unwrap();
        let path = workspace.join("profile/nanh/chatgpt-desktop/profile");
        let original = workspace.join("profile/nanh/chatgpt-desktop/original");
        std::fs::rename(&path, &original).unwrap();
        nan_harness_private_fs::create_private_dir(&path).unwrap();
        assert!(token.private_request(deadline()).is_err());
        std::fs::remove_dir(&path).unwrap();
        symlink(&original, &path).unwrap();
        assert!(token.private_request(deadline()).is_err());
        std::fs::remove_file(&path).unwrap();
        std::fs::rename(&original, &path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(token.private_request(deadline()).is_err());
        assert!(token.private_request(Instant::now()).is_err());
    }
    #[test]
    fn changed_command_redirect_and_existing_empty_root_never_gain_custody() {
        for invalid in [
            "redirect",
            "inherited-home",
            "cwd",
            "existing-empty",
            "expired",
        ] {
            let (_temp, workspace, mut command) = fixture();
            match invalid {
                "redirect" => {
                    command.args(["--config-dir", "/synthetic/foreign"]);
                }
                "inherited-home" => {
                    command.env_remove("CODEX_HOME");
                }
                "cwd" => {
                    command.current_dir(workspace.join("profile"));
                }
                "existing-empty" => {
                    nan_harness_private_fs::create_private_dir(&workspace.join("profile/nanh"))
                        .unwrap();
                }
                _ => {}
            }
            let deadline = if invalid == "expired" {
                Instant::now()
            } else {
                deadline()
            };
            assert!(FreshCodexMacProfile::create_owned(&workspace, &command, deadline).is_err());
            assert!(!workspace.join("profile/codex-desktop").exists());
            assert!(
                !workspace
                    .join("profile/nanh/chatgpt-desktop/profile")
                    .exists()
            );
        }
    }
}
