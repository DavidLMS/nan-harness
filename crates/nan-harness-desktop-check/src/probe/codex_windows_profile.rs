//! Original Windows directory leases for explicitly prepared Codex DOM launches.
//! No global-state loan or input authority is exported by this boundary.
use super::{ProbeSpec, Reason};
use std::os::windows::fs::{MetadataExt as _, OpenOptionsExt as _};
use std::{
    ffi::OsString,
    fs::File,
    path::{Path, PathBuf},
    time::Instant,
};
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
            ("USERPROFILE", profile.join("home")),
            ("LOCALAPPDATA", profile.join("home/AppData/Local")),
            ("APPDATA", profile.join("home/AppData/Roaming")),
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

pub(crate) struct FreshCodexWindowsProfile {
    directories: Vec<File>,
    private_start: usize,
    command: PreparedCommand,
    launched: bool,
}
fn ordinary(file: &File) -> bool {
    file.metadata()
        .is_ok_and(|m| m.is_dir() && m.file_attributes() & 0x400 == 0)
}
fn retain(path: &Path) -> Result<File, Reason> {
    let file = std::fs::OpenOptions::new().read(true).access_mode(0x8002_0000)
        // Deny directory writes and DELETE sharing: the original path binding
        // and reparse metadata cannot be replaced while the renderer borrows it.
        .share_mode(1).custom_flags(0x0200_0000 | 0x0020_0000).open(path)
        .map_err(|_| Reason::IsolationUnavailable)?;
    if !ordinary(&file) {
        return Err(Reason::IsolationUnavailable);
    }
    Ok(file)
}
impl FreshCodexWindowsProfile {
    pub(crate) fn prepare(
        spec: &ProbeSpec,
        command: &tokio::process::Command,
        deadline: Instant,
    ) -> Result<Option<Self>, Reason> {
        if std::env::var("NANH_CODEX_INPUT_CHANNEL").as_deref() != Ok("cdp-dom")
            || spec.kind != nan_harness_core::DesktopHarnessKind::ChatGpt
        {
            return Ok(None);
        }
        if spec.session != crate::cli::SessionMode::GithubHosted
            || std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
            || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
            || std::env::var("RUNNER_OS").as_deref() != Ok("Windows")
        {
            return Err(Reason::IsolationUnavailable);
        }
        PreparedCommand::validate_profile(command, &spec.workspace, deadline)?;
        let mut directories = Vec::new();
        // Parent-first leases eliminate the pathname replacement gap while
        // acquiring children. None of these handles can mutate vendor state.
        let mut ancestors = spec.workspace.ancestors().collect::<Vec<_>>();
        ancestors.reverse();
        for path in ancestors {
            if Instant::now() >= deadline {
                return Err(Reason::IsolationUnavailable);
            }
            directories.push(retain(path)?);
        }
        let private_start = directories.len() - 1;
        for suffix in [
            "profile",
            "profile/home",
            "profile/config",
            "profile/nanh",
            "profile/nanh/chatgpt-desktop",
            "profile/nanh/chatgpt-desktop/profile",
            "profile/codex-desktop",
            "profile/home/AppData",
            "profile/home/AppData/Local",
            "profile/home/AppData/Roaming",
        ] {
            if Instant::now() >= deadline {
                return Err(Reason::IsolationUnavailable);
            }
            directories.push(retain(&spec.workspace.join(suffix))?);
        }
        for suffix in [
            "profile/codex-desktop",
            "profile/nanh/chatgpt-desktop/profile",
        ] {
            let mut entries = std::fs::read_dir(spec.workspace.join(suffix))
                .map_err(|_| Reason::IsolationUnavailable)?;
            if entries.next().is_some() {
                return Err(Reason::IsolationUnavailable);
            }
        }
        if !matches!(std::fs::symlink_metadata(spec.workspace.join("profile/home/.codex")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound)
        {
            return Err(Reason::IsolationUnavailable);
        }
        let profile = Self {
            directories,
            private_start,
            command: PreparedCommand::from(command),
            launched: false,
        };
        if !profile.verifies_owned(deadline) {
            return Err(Reason::IsolationUnavailable);
        }
        Ok(Some(profile))
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
        use nan_harness_private_fs::{
            OwnedWindowsDacl, PrivatePathKind, classify_owned_windows_dacl,
        };
        self.directories.len() == self.private_start + 11
            && self
                .directories
                .iter()
                .all(|file| Instant::now() < deadline && ordinary(file))
            && self.directories[self.private_start..].iter().all(|file| {
                Instant::now() < deadline
                    && classify_owned_windows_dacl(file, PrivatePathKind::Directory)
                        == OwnedWindowsDacl::Protected
            })
            && Instant::now() < deadline
    }
    pub(crate) fn prepared_for_renderer(&self, deadline: Instant) -> bool {
        self.launched && self.verifies_owned(deadline)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    fn deadline() -> Instant {
        Instant::now() + Duration::from_secs(2)
    }
    #[test]
    fn held_private_directory_rejects_replacement_and_expired_admission() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("synthetic-private");
        nan_harness_private_fs::create_private_dir(&root).unwrap();
        let held = retain(&root).unwrap();
        assert!(ordinary(&held));
        assert!(std::fs::rename(&root, temp.path().join("replacement")).is_err());
        assert!(std::fs::remove_dir(&root).is_err());
        assert!(
            std::fs::OpenOptions::new()
                .read(true)
                .access_mode(0x4000_0000)
                .share_mode(7)
                .custom_flags(0x0200_0000)
                .open(&root)
                .is_err()
        );
        // Creating ordinary child files does not require mutable directory handles.
        std::fs::write(root.join("synthetic-child"), b"neutral").unwrap();
        let command = tokio::process::Command::new("synthetic-never-spawned");
        let profile = FreshCodexWindowsProfile {
            directories: vec![held],
            private_start: 0,
            command: PreparedCommand::from(&command),
            launched: false,
        };
        assert!(!profile.prepared_for_renderer(deadline()));
        assert!(!profile.verifies_owned(Instant::now()));
    }
    #[test]
    fn original_leases_allow_one_exact_launch_and_no_expired_renderer_admission() {
        let temp = tempfile::tempdir().unwrap();
        let mut directories = Vec::new();
        for index in 0..11 {
            let path = temp.path().join(format!("synthetic-root-{index}"));
            nan_harness_private_fs::create_private_dir(&path).unwrap();
            directories.push(retain(&path).unwrap());
        }
        let mut command = tokio::process::Command::new("synthetic-never-spawned");
        command.env("HOME", "synthetic-home");
        let mut profile = FreshCodexWindowsProfile {
            directories,
            private_start: 0,
            command: PreparedCommand::from(&command),
            launched: false,
        };
        assert!(profile.verifies_owned(deadline()));
        assert!(!profile.prepared_for_renderer(deadline()));
        command.env("HOME", "foreign-home");
        assert!(profile.before_launch(&command, deadline()).is_err());
        command.env("HOME", "synthetic-home");
        profile.before_launch(&command, deadline()).unwrap();
        assert!(profile.prepared_for_renderer(deadline()));
        assert!(profile.before_launch(&command, deadline()).is_err());
        assert!(!profile.prepared_for_renderer(Instant::now()));
    }
    #[test]
    fn exact_command_snapshot_rejects_any_redirect() {
        let mut command = tokio::process::Command::new("synthetic-never-spawned");
        command
            .env("HOME", "synthetic-home")
            .current_dir("synthetic-workspace");
        let original = PreparedCommand::from(&command);
        command.env("HOME", "foreign-home");
        assert!(original != PreparedCommand::from(&command));
        command
            .env("HOME", "synthetic-home")
            .current_dir("foreign-workspace");
        assert!(original != PreparedCommand::from(&command));
        command.current_dir("synthetic-workspace").arg("unexpected");
        assert!(original != PreparedCommand::from(&command));
    }
}
