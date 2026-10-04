//! Exclusive prelaunch custody of the frozen Linux Claude userData roots.
use super::{ProbeSpec, Reason};
use serde_json::{Value, json};
use std::{fs::File, os::unix::fs::MetadataExt as _, path::PathBuf, time::Instant};

struct HeldDirectory {
    path: PathBuf,
    handle: File,
}
pub(crate) struct FreshClaudeLinuxProfile {
    directories: Vec<HeldDirectory>,
}
fn private_directory(path: &std::path::Path) -> std::io::Result<std::fs::Metadata> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.mode() & 0o077 != 0
        || path.canonicalize()? != path
    {
        return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
    }
    Ok(metadata)
}
fn same(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
    a.dev() == b.dev() && a.ino() == b.ino() && a.uid() == b.uid()
}
fn retain_directory(path: PathBuf) -> Result<HeldDirectory, Reason> {
    let metadata = private_directory(&path).map_err(|_| Reason::IsolationUnavailable)?;
    if metadata.uid() != nix::unistd::Uid::effective().as_raw() {
        return Err(Reason::IsolationUnavailable);
    }
    let handle = File::open(&path).map_err(|_| Reason::IsolationUnavailable)?;
    if !same(
        &metadata,
        &handle
            .metadata()
            .map_err(|_| Reason::IsolationUnavailable)?,
    ) {
        return Err(Reason::IsolationUnavailable);
    }
    Ok(HeldDirectory { path, handle })
}
fn directories_valid(directories: &[HeldDirectory], deadline: Instant) -> bool {
    directories.iter().all(|d| {
        Instant::now() < deadline
            && private_directory(&d.path)
                .ok()
                .zip(d.handle.metadata().ok())
                .is_some_and(|(path, held)| {
                    path.uid() == nix::unistd::Uid::effective().as_raw() && same(&path, &held)
                })
    }) && Instant::now() < deadline
}
impl FreshClaudeLinuxProfile {
    pub(crate) fn prepare(
        spec: &ProbeSpec,
        command: &tokio::process::Command,
        deadline: Instant,
    ) -> Result<Option<Self>, Reason> {
        if !crate::gui::claude_linux_chat_policy() {
            return Ok(None);
        }
        if std::env::var("NANH_CLAUDE_LINUX_CHAT_ONLY").as_deref() != Ok("1") {
            return Err(Reason::IsolationUnavailable);
        }
        if spec.kind != nan_harness_core::DesktopHarnessKind::Claude
            || spec.session != crate::cli::SessionMode::GithubHosted
            || Instant::now() >= deadline
        {
            return Err(Reason::IsolationUnavailable);
        }
        Self::create_owned(&spec.workspace, command, deadline).map(Some)
    }
    fn create_owned(
        workspace: &std::path::Path,
        command: &tokio::process::Command,
        deadline: Instant,
    ) -> Result<Self, Reason> {
        Self::create_owned_with(workspace, command, deadline, |_| {})
    }
    fn create_owned_with(
        workspace: &std::path::Path,
        command: &tokio::process::Command,
        deadline: Instant,
        mut before_creation: impl FnMut(&std::path::Path),
    ) -> Result<Self, Reason> {
        if Instant::now() >= deadline {
            return Err(Reason::IsolationUnavailable);
        }
        let profile = workspace.join("profile");
        let required = [
            ("HOME", profile.join("home")),
            ("XDG_CONFIG_HOME", profile.join("config")),
            ("NAN_HARNESS_CONFIG_DIR", profile.join("nanh")),
        ];
        let command = command.as_std();
        if command.get_current_dir() != Some(workspace)
            || required.iter().any(|(key, path)| {
                !command
                    .get_envs()
                    .any(|(k, v)| k == *key && v == Some(path.as_os_str()))
            })
            // A missing override entry can inherit from the checker. Require
            // explicit removal in the exact command that will be spawned.
            || ["CLAUDE_USER_DATA_DIR", "CLAUDE_CDP_AUTH"].iter().any(|key| {
                !command.get_envs().any(|(k, v)| k == *key && v.is_none())
            })
        {
            return Err(Reason::IsolationUnavailable);
        }
        let paths = [
            workspace.to_owned(),
            profile.clone(),
            profile.join("home"),
            profile.join("config"),
            profile.join("nanh"),
        ];
        let mut directories = Vec::new();
        // Retain the original parents before mutation; never capture their
        // replacement as a new source of authority after root creation.
        for path in paths {
            directories.push(retain_directory(path)?);
        }
        let roots = [
            profile.join("config/Claude"),
            profile.join("config/Claude-3p"),
        ];
        if roots.iter().any(|p| !matches!(std::fs::symlink_metadata(p), Err(e) if e.kind() == std::io::ErrorKind::NotFound)) {
            return Err(Reason::IsolationUnavailable);
        }
        for path in roots {
            if !directories_valid(&directories, deadline) {
                return Err(Reason::IsolationUnavailable);
            }
            before_creation(&path);
            if !directories_valid(&directories, deadline) {
                return Err(Reason::IsolationUnavailable);
            }
            nan_harness_private_fs::create_private_dir(&path)
                .map_err(|_| Reason::IsolationUnavailable)?;
            directories.push(retain_directory(path)?);
            if !directories_valid(&directories, deadline) {
                return Err(Reason::IsolationUnavailable);
            }
        }
        let token = Self { directories };
        if !token.verifies_owned(deadline) {
            return Err(Reason::IsolationUnavailable);
        }
        Ok(token)
    }
    pub(crate) fn verifies_owned(&self, deadline: Instant) -> bool {
        self.directories.len() == 7 && directories_valid(&self.directories, deadline)
    }

    pub(crate) fn private_request(&self, deadline: Instant) -> Result<Value, Reason> {
        if !self.verifies_owned(deadline) {
            return Err(Reason::IsolationUnavailable);
        }
        let mut records = Vec::new();
        for directory in &self.directories {
            let m = directory
                .handle
                .metadata()
                .map_err(|_| Reason::IsolationUnavailable)?;
            records.push(
                json!({"path":directory.path,"device":m.dev(),"inode":m.ino(),"uid":m.uid()}),
            );
        }
        Ok(json!(records))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt as _, symlink};
    use std::time::Duration;

    fn fixture() -> (tempfile::TempDir, PathBuf, tokio::process::Command) {
        let temporary = tempfile::tempdir().unwrap();
        let workspace = temporary.path().canonicalize().unwrap();
        for suffix in [
            "",
            "profile",
            "profile/home",
            "profile/config",
            "profile/nanh",
        ] {
            let path = workspace.join(suffix);
            nan_harness_private_fs::create_private_dir_all(&path).unwrap();
        }
        let mut command = tokio::process::Command::new("synthetic-never-spawned");
        command
            .current_dir(&workspace)
            .env("HOME", workspace.join("profile/home"))
            .env("XDG_CONFIG_HOME", workspace.join("profile/config"))
            .env("NAN_HARNESS_CONFIG_DIR", workspace.join("profile/nanh"))
            .env_remove("CLAUDE_USER_DATA_DIR")
            .env_remove("CLAUDE_CDP_AUTH");
        (temporary, workspace, command)
    }
    fn deadline() -> Instant {
        Instant::now() + Duration::from_secs(1)
    }

    #[test]
    fn exclusive_custody_rejects_reuse_and_preserves_payload() {
        let (_temporary, workspace, command) = fixture();
        let token =
            FreshClaudeLinuxProfile::create_owned(&workspace, &command, deadline()).unwrap();
        let payload = workspace.join("profile/config/Claude-3p/owned");
        std::fs::write(&payload, b"private existing payload").unwrap();
        assert!(FreshClaudeLinuxProfile::create_owned(&workspace, &command, deadline()).is_err());
        assert_eq!(std::fs::read(payload).unwrap(), b"private existing payload");
        assert!(token.verifies_owned(deadline()));
    }
    #[test]
    fn wrong_command_or_inherited_override_never_creates_roots() {
        for invalid in ["cwd", "home", "config", "state", "override", "inherit"] {
            let (_temporary, workspace, mut command) = fixture();
            match invalid {
                "cwd" => {
                    command.current_dir(workspace.join("profile"));
                }
                "home" => {
                    command.env("HOME", workspace.join("profile"));
                }
                "config" => {
                    command.env("XDG_CONFIG_HOME", workspace.join("profile"));
                }
                "state" => {
                    command.env("NAN_HARNESS_CONFIG_DIR", workspace.join("profile"));
                }
                "override" => {
                    command.env("CLAUDE_USER_DATA_DIR", workspace.join("profile"));
                }
                _ => {
                    // No env mutation: a fresh command has no explicit removal.
                    command = tokio::process::Command::new("synthetic-never-spawned");
                    command
                        .current_dir(&workspace)
                        .env("HOME", workspace.join("profile/home"))
                        .env("XDG_CONFIG_HOME", workspace.join("profile/config"))
                        .env("NAN_HARNESS_CONFIG_DIR", workspace.join("profile/nanh"));
                }
            }
            assert!(
                FreshClaudeLinuxProfile::create_owned(&workspace, &command, deadline()).is_err()
            );
            assert!(!workspace.join("profile/config/Claude").exists());
            assert!(!workspace.join("profile/config/Claude-3p").exists());
        }
    }
    #[test]
    fn replacement_permissions_redirect_and_expiry_revoke_custody() {
        let (_temporary, workspace, command) = fixture();
        let token =
            FreshClaudeLinuxProfile::create_owned(&workspace, &command, deadline()).unwrap();
        let path = workspace.join("profile/config/Claude-3p");
        let retained = workspace.join("profile/config/original");
        std::fs::rename(&path, &retained).unwrap();
        nan_harness_private_fs::create_private_dir(&path).unwrap();
        assert!(!token.verifies_owned(deadline()));
        std::fs::remove_dir(&path).unwrap();
        symlink(&retained, &path).unwrap();
        assert!(!token.verifies_owned(deadline()));
        std::fs::remove_file(&path).unwrap();
        std::fs::rename(&retained, &path).unwrap();
        assert!(token.verifies_owned(deadline()));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(!token.verifies_owned(deadline()));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(!token.verifies_owned(Instant::now()));
        assert!(token.private_request(Instant::now()).is_err());
    }
    #[test]
    fn changed_parent_and_expired_prelaunch_never_grant_authority() {
        let (_temporary, workspace, command) = fixture();
        assert!(
            FreshClaudeLinuxProfile::create_owned(&workspace, &command, Instant::now()).is_err()
        );
        assert!(!workspace.join("profile/config/Claude").exists());
        let token =
            FreshClaudeLinuxProfile::create_owned(&workspace, &command, deadline()).unwrap();
        let config = workspace.join("profile/config");
        std::fs::rename(&config, workspace.join("profile/old-config")).unwrap();
        nan_harness_private_fs::create_private_dir(&config).unwrap();
        assert!(!token.verifies_owned(deadline()));
    }
    #[test]
    fn parent_replaced_during_creation_cannot_be_adopted() {
        let (_temporary, workspace, command) = fixture();
        let config = workspace.join("profile/config");
        let retained = workspace.join("profile/retained-config");
        let payload = config.join("replacement-payload");
        let result =
            FreshClaudeLinuxProfile::create_owned_with(&workspace, &command, deadline(), |_| {
                std::fs::rename(&config, &retained).unwrap();
                nan_harness_private_fs::create_private_dir(&config).unwrap();
                std::fs::write(&payload, b"replacement remains untouched").unwrap();
            });
        assert!(result.is_err());
        assert_eq!(
            std::fs::read(payload).unwrap(),
            b"replacement remains untouched"
        );
        assert!(!config.join("Claude").exists());
        assert!(!config.join("Claude-3p").exists());
    }
}
