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
    fn validate_profile(
        command: &tokio::process::Command,
        workspace: &Path,
        deadline: Instant,
        observation: &mut PrepareObservation,
    ) -> Result<(), Reason> {
        let profile = workspace.join("profile");
        let required = [
            ("HOME", profile.join("home")),
            ("XDG_CONFIG_HOME", profile.join("config")),
            ("USERPROFILE", profile.join("home")),
            (
                "LOCALAPPDATA",
                profile.join("home").join("AppData").join("Local"),
            ),
            (
                "APPDATA",
                profile.join("home").join("AppData").join("Roaming"),
            ),
            ("NAN_HARNESS_CONFIG_DIR", profile.join("nanh")),
            ("CODEX_HOME", profile.join("home").join(".codex")),
            (
                "CODEX_ELECTRON_USER_DATA_PATH",
                profile.join("codex-desktop"),
            ),
        ];
        observation.at("command", "arguments");
        let prepared = command.as_std();
        let arguments = prepared.get_args().collect::<Vec<_>>();
        if arguments.len() != 9
            || arguments[0] != "chatgpt-desktop"
            || arguments[1] != "--provider-base-url"
            || arguments[3] != "--model"
            || arguments[5] != "--startup-timeout"
            || arguments[6] != "120"
            || arguments[7] != "--executable"
            || !Path::new(arguments[8]).is_absolute()
        {
            return Err(Reason::IsolationUnavailable);
        }
        observation.cause = Some("cwd");
        if prepared.get_current_dir() != Some(workspace) || !workspace.is_absolute() {
            return Err(Reason::IsolationUnavailable);
        }
        observation.cause = Some("binding");
        for (index, (key, path)) in required.iter().enumerate() {
            observation.binding_index = Some(index);
            if !prepared
                .get_envs()
                .any(|(k, v)| k == *key && v == Some(path.as_os_str()))
            {
                return Err(Reason::IsolationUnavailable);
            }
        }
        observation.binding_index = None;
        observation.check_deadline(deadline)
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

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PrepareObservation {
    schema_version: u8,
    mechanism: &'static str,
    diagnostics_only: bool,
    stage: &'static str,
    cause: Option<&'static str>,
    binding_index: Option<usize>,
    ancestor_count: usize,
    owned_count: usize,
    privacy: [Option<&'static str>; 11],
    empty_roots: [Option<bool>; 2],
    code_home_absent: Option<bool>,
    completed: bool,
}
impl PrepareObservation {
    fn new() -> Self {
        Self {
            schema_version: 1,
            mechanism: "codex-windows-profile-prepare",
            diagnostics_only: true,
            stage: "policy",
            cause: Some("policy"),
            binding_index: None,
            ancestor_count: 0,
            owned_count: 0,
            privacy: [None; 11],
            empty_roots: [None; 2],
            code_home_absent: None,
            completed: false,
        }
    }
    fn at(&mut self, stage: &'static str, cause: &'static str) {
        self.stage = stage;
        self.cause = Some(cause);
    }
    fn check_deadline(&mut self, deadline: Instant) -> Result<(), Reason> {
        if Instant::now() >= deadline {
            self.cause = Some("original-cutoff");
            return Err(Reason::IsolationUnavailable);
        }
        Ok(())
    }
}
impl Drop for PrepareObservation {
    fn drop(&mut self) {
        if let Ok(value) = serde_json::to_value(&*self) {
            crate::process::windows_correlation::record_codex_prepare(&value);
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
#[cfg(test)]
fn retain(path: &Path) -> Result<File, Reason> {
    retain_native(path).map_err(|_| Reason::IsolationUnavailable)
}
fn retain_native(path: &Path) -> Result<File, &'static str> {
    let file = std::fs::OpenOptions::new().read(true).access_mode(0x8002_0000)
        // Child configuration files must support atomic replacement. Deny DELETE
        // sharing on the directory itself; recheck reparse metadata on every borrow.
        .share_mode(3).custom_flags(0x0200_0000 | 0x0020_0000).open(path)
        .map_err(|error| match error.raw_os_error() {
            Some(2 | 3) => "directory-missing",
            Some(5) => "directory-access",
            Some(32) => "directory-sharing",
            _ => "directory-open",
        })?;
    let metadata = file.metadata().map_err(|_| "directory-metadata")?;
    if metadata.file_attributes() & 0x400 != 0 {
        return Err("directory-reparse");
    }
    if !metadata.is_dir() {
        return Err("directory-type");
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
        let mut observation = PrepareObservation::new();
        if spec.session != crate::cli::SessionMode::GithubHosted
            || std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
            || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
            || std::env::var("RUNNER_OS").as_deref() != Ok("Windows")
        {
            return Err(Reason::IsolationUnavailable);
        }
        Self::prepare_owned(spec, command, deadline, &mut observation).map(Some)
    }
    fn prepare_owned(
        spec: &ProbeSpec,
        command: &tokio::process::Command,
        deadline: Instant,
        observation: &mut PrepareObservation,
    ) -> Result<Self, Reason> {
        PreparedCommand::validate_profile(command, &spec.workspace, deadline, observation)?;
        let mut directories = Self::retain_ancestors(&spec.workspace, deadline, observation)?;
        let private_start = directories.len() - 1;
        observation.at("owned-acquisition", "directory-open");
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
            observation.check_deadline(deadline)?;
            directories.push(
                retain_native(&spec.workspace.join(suffix)).map_err(|cause| {
                    observation.cause = Some(cause);
                    Reason::IsolationUnavailable
                })?,
            );
            observation.owned_count += 1;
        }
        observation.at("privacy", "privacy");
        for (index, file) in directories[private_start..].iter().enumerate() {
            observation.check_deadline(deadline)?;
            let privacy = nan_harness_private_fs::classify_owned_windows_dacl(
                file,
                nan_harness_private_fs::PrivatePathKind::Directory,
            );
            observation.privacy[index] = Some(match privacy {
                nan_harness_private_fs::OwnedWindowsDacl::Protected => "protected",
                nan_harness_private_fs::OwnedWindowsDacl::Inherited => "inherited",
                nan_harness_private_fs::OwnedWindowsDacl::Unexpected => "unexpected",
                nan_harness_private_fs::OwnedWindowsDacl::Unavailable => "unavailable",
            });
            if privacy != nan_harness_private_fs::OwnedWindowsDacl::Protected {
                return Err(Reason::IsolationUnavailable);
            }
        }
        Self::check_initial_roots(&spec.workspace, deadline, observation)?;
        let profile = Self {
            directories,
            private_start,
            command: PreparedCommand::from(command),
            launched: false,
        };
        observation.at("final-custody", "custody");
        observation.check_deadline(deadline)?;
        if !profile.verifies_owned(deadline) {
            return Err(Reason::IsolationUnavailable);
        }
        observation.stage = "completed";
        observation.cause = None;
        observation.completed = true;
        Ok(profile)
    }
    fn retain_ancestors(
        workspace: &Path,
        deadline: Instant,
        observation: &mut PrepareObservation,
    ) -> Result<Vec<File>, Reason> {
        observation.at("ancestor-acquisition", "directory-open");
        let mut ancestors = workspace.ancestors().filter(|path|
            // A bare drive/verbatim prefix is not an object until RootDir follows it.
            !path.as_os_str().is_empty() && !matches!(path.components().collect::<Vec<_>>().as_slice(),
                [std::path::Component::Prefix(_)])).collect::<Vec<_>>();
        ancestors.reverse();
        if ancestors.is_empty() || ancestors.len() > 64 {
            observation.cause = Some("ancestor-budget");
            return Err(Reason::IsolationUnavailable);
        }
        let mut directories = Vec::new();
        for path in ancestors {
            observation.check_deadline(deadline)?;
            directories.push(retain_native(path).map_err(|cause| {
                observation.cause = Some(cause);
                Reason::IsolationUnavailable
            })?);
            observation.ancestor_count += 1;
        }
        Ok(directories)
    }
    fn check_initial_roots(
        workspace: &Path,
        deadline: Instant,
        observation: &mut PrepareObservation,
    ) -> Result<(), Reason> {
        observation.at("empty-roots", "directory-enumeration");
        for (index, suffix) in [
            "profile/codex-desktop",
            "profile/nanh/chatgpt-desktop/profile",
        ]
        .iter()
        .enumerate()
        {
            observation.check_deadline(deadline)?;
            let mut entries = std::fs::read_dir(workspace.join(suffix))
                .map_err(|_| Reason::IsolationUnavailable)?;
            let empty = match entries.next() {
                None => true,
                Some(Ok(_)) => false,
                Some(Err(_)) => return Err(Reason::IsolationUnavailable),
            };
            observation.empty_roots[index] = Some(empty);
            if !empty {
                observation.cause = Some("root-populated");
                return Err(Reason::IsolationUnavailable);
            }
        }
        observation.at("code-home", "code-home-metadata");
        observation.check_deadline(deadline)?;
        let absent = match std::fs::symlink_metadata(
            workspace.join("profile").join("home").join(".codex"),
        ) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
            Ok(_) => false,
            Err(_) => return Err(Reason::IsolationUnavailable),
        };
        observation.code_home_absent = Some(absent);
        if !absent {
            observation.cause = Some("code-home-present");
            return Err(Reason::IsolationUnavailable);
        }
        Ok(())
    }
    #[cfg(test)]
    pub(super) fn prepare_fixture(
        spec: &ProbeSpec,
        command: &tokio::process::Command,
        deadline: Instant,
    ) -> Result<Self, Reason> {
        Self::prepare_owned(spec, command, deadline, &mut PrepareObservation::new())
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
                .is_ok()
        );
        let pending = root.join("synthetic-child.tmp");
        let config = root.join("synthetic-child");
        std::fs::write(&config, b"previous").unwrap();
        std::fs::write(&pending, b"neutral").unwrap();
        std::fs::rename(&pending, &config).unwrap();
        assert_eq!(std::fs::read(&config).unwrap(), b"neutral");
        assert!(ordinary(&held));
        assert!(std::fs::rename(&root, temp.path().join("replacement")).is_err());
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
