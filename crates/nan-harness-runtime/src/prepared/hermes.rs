use super::PreparedError;
use crate::temporary::{TemporaryError, TemporaryWorkspace};
use nan_harness_core::{HarnessKind, LaunchPlan};
use nan_harness_private_fs::{
    PrivatePathKind, create_private_dir, create_private_dir_all, open_private_new,
    open_private_read, open_private_read_write, restrict_path,
};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};
mod files;
#[cfg(windows)]
mod native_path;

/// Even --version can normalize native configuration. Borrow dependencies into
/// an empty home before invoking it, so discovery cannot rewrite the user's home.
pub(crate) fn version_home() -> io::Result<tempfile::TempDir> {
    let user_home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("NH-HERMES-HOME"))?;
    let source = source_home(&user_home);
    let home = tempfile::Builder::new()
        .prefix("nan-harness-hermes-version-")
        .tempdir()?;
    restrict_path(home.path(), PrivatePathKind::Directory)?;
    borrow_dependencies(&source, home.path())?;
    Ok(home)
}

fn borrow_dependencies(source: &Path, home: &Path) -> io::Result<()> {
    open_private_new(&home.join("config.yaml"))?.write_all(b"updates:\n  check: false\n")?;
    files::dependencies(&source.join("installs"), &home.join("installs"))?;
    if source.join("tools").is_dir() {
        let tools = fs::canonicalize(source.join("tools"))?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(tools, home.join("tools"))?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(tools, home.join("tools"))?;
    }
    Ok(())
}

/// A stable, exclusively leased native profile. Only its history survives a launch;
/// credentials and the foreground gateway remain owned by the prepared child.
pub(crate) struct ManagedProfile {
    _lease: File,
    pub(crate) home: PathBuf,
    pub(crate) native_home: PathBuf,
    pub(crate) scripts: PathBuf,
    pub(crate) bootstrap: PathBuf,
    overlay: PathBuf,
    owned_files: Vec<PathBuf>,
}

#[cfg(test)]
impl ManagedProfile {
    pub(super) fn fixture(home: PathBuf, scripts: PathBuf) -> Self {
        let overlay = scripts.join("fixture-overlay");
        fs::create_dir(&overlay).expect("fixture overlay");
        fs::write(overlay.join("config.yaml"), "{}").expect("fixture config");
        Self {
            _lease: File::create(home.join("lease")).expect("fixture lease"),
            bootstrap: home.clone(),
            native_home: home.clone(),
            home,
            scripts,
            overlay,
            owned_files: Vec::new(),
        }
    }
}

impl ManagedProfile {
    /// The caller must establish native owner absence while retaining this lease.
    pub(crate) fn stage_files(&self) -> io::Result<()> {
        files::stage(&self.overlay, &self.home, &self.owned_files)
    }
}

pub(super) fn prepare(
    plan: &LaunchPlan,
    workspace: &TemporaryWorkspace,
    environment: &mut BTreeMap<String, String>,
    client_base_url: &str,
) -> Result<Option<ManagedProfile>, PreparedError> {
    if !uses_gateway(plan) {
        return Ok(None);
    }
    validate_arguments(&plan.process.arguments).map_err(|source| TemporaryError::Materialize {
        artifact_id: "hermes-managed-profile".into(),
        source,
    })?;
    environment.insert("NAN_HARNESS_HERMES_BASE_URL".into(), client_base_url.into());
    environment.insert(
        "NAN_HARNESS_HERMES_MODEL".into(),
        plan.model.resolved_id.clone(),
    );
    prepare_gateway(plan, workspace, environment)
        .map(Some)
        .map_err(|source| {
            TemporaryError::Materialize {
                artifact_id: "hermes-managed-profile".into(),
                source,
            }
            .into()
        })
}

fn validate_arguments(arguments: &[String]) -> io::Result<()> {
    let protected = [
        "--provider",
        "--model",
        "--profile",
        "--api-key",
        "--base-url",
        "--safe-mode",
        "--ignore-user-config",
    ];
    // argparse accepts unique long-option abbreviations. They must not bypass
    // the config that supplies NaN routing to canonical create/resume calls.
    if arguments.iter().skip(4).any(|argument| {
        let option = argument.split('=').next().unwrap_or_default();
        option.starts_with("--")
            && option.len() > 2
            && protected.iter().any(|name| name.starts_with(option))
    }) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "NH-HERMES-ROUTING",
        ));
    }
    Ok(())
}

fn uses_gateway(plan: &LaunchPlan) -> bool {
    plan.harness.kind == HarnessKind::Hermes
        && crate::discovery::parse_version(&plan.harness.detected_version)
            .is_none_or(|version| version >= semver::Version::new(0, 22, 0))
}

fn prepare_gateway(
    plan: &LaunchPlan,
    workspace: &TemporaryWorkspace,
    environment: &mut BTreeMap<String, String>,
) -> io::Result<ManagedProfile> {
    let source = fs::canonicalize(source_home(workspace.user_home()))?;
    let key = Sha256::digest(source.as_os_str().as_encoded_bytes())
        .iter()
        .fold(String::new(), |mut hex, byte| {
            use std::fmt::Write as _;
            let _ = write!(hex, "{byte:02x}");
            hex
        });
    let parent = config_directory(workspace.user_home()).join("hermes-profiles");
    create_private_dir_all(&parent)?;
    let home = parent.join(&key);
    let marker = format!("nan-harness-hermes-profile-v1\n{key}\n");
    open_owned_home(&home, &marker)?;
    let lease = open_private_read_write(&home.join(".nanh-launch.lock"))?;
    lease
        .try_lock()
        .map_err(|_| io::Error::new(io::ErrorKind::WouldBlock, "NH-HERMES-PROFILE-BUSY"))?;
    let scripts = workspace.root().join("hermes-supervisor");
    create_private_dir(&scripts)?;
    for (name, content) in [
        ("main.py", include_str!("hermes/main.py")),
        ("nanh_profile.py", include_str!("hermes/nanh_profile.py")),
    ] {
        open_private_new(&scripts.join(name))?.write_all(content.as_bytes())?;
    }
    let home = fs::canonicalize(home)?;
    #[cfg(windows)]
    let native_home = native_path::for_sdk(&home)?;
    #[cfg(not(windows))]
    let native_home = home.clone();
    environment.insert(
        "NAN_HARNESS_HERMES_MANAGED_HOME".into(),
        native_home.to_string_lossy().into(),
    );
    environment.insert("PYTHONDONTWRITEBYTECODE".into(), "1".into());
    let bootstrap = workspace.root().join("hermes-bootstrap");
    create_private_dir(&bootstrap)?;
    borrow_dependencies(&source, &bootstrap)?;
    let overlay = workspace
        .path("hermes-home")
        .ok_or_else(|| io::Error::other("NH-HERMES-OVERLAY"))?
        .to_owned();
    environment.insert(
        "NAN_HARNESS_HERMES_OVERLAY".into(),
        overlay.to_string_lossy().into(),
    );
    Ok(ManagedProfile {
        _lease: lease,
        home,
        native_home,
        scripts,
        overlay,
        bootstrap,
        owned_files: plan
            .configuration_overlays
            .iter()
            .filter(|overlay| overlay.id == "hermes-home")
            .flat_map(|overlay| &overlay.files)
            .filter(|file| file.path.starts_with("plugins/"))
            .map(|file| PathBuf::from(&file.path))
            .collect(),
    })
}

fn open_owned_home(home: &Path, marker: &str) -> io::Result<()> {
    match create_private_dir(home) {
        Ok(()) => open_private_new(&home.join(".nanh-owner"))?.write_all(marker.as_bytes())?,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            if !fs::symlink_metadata(home)?.file_type().is_dir() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "NH-HERMES-PROFILE-OWNER",
                ));
            }
            let (mut file, _) = open_private_read(&home.join(".nanh-owner"))?;
            let mut actual = String::new();
            file.read_to_string(&mut actual)?;
            if actual != marker {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "NH-HERMES-PROFILE-OWNER",
                ));
            }
            restrict_path(home, PrivatePathKind::Directory)?;
        }
        Err(error) => return Err(error),
    }
    Ok(())
}

fn source_home(user_home: &Path) -> PathBuf {
    if let Some(home) = std::env::var_os("HERMES_HOME").filter(|home| !home.is_empty()) {
        return home.into();
    }
    #[cfg(windows)]
    if let Some(home) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(home).join("hermes");
    }
    user_home.join(".hermes")
}

fn config_directory(user_home: &Path) -> PathBuf {
    if let Some(directory) = std::env::var_os("NAN_HARNESS_CONFIG_DIR") {
        return directory.into();
    }
    #[cfg(target_os = "macos")]
    return user_home.join("Library/Application Support/nan-harness");
    #[cfg(windows)]
    return std::env::var_os("APPDATA").map_or_else(
        || user_home.join("AppData/Roaming/nan-harness"),
        |directory| PathBuf::from(directory).join("nan-harness"),
    );
    #[cfg(not(any(target_os = "macos", windows)))]
    return std::env::var_os("XDG_CONFIG_HOME").map_or_else(
        || user_home.join(".config/nan-harness"),
        |directory| PathBuf::from(directory).join("nan-harness"),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_gateway_versions_and_unreadable_versions_cannot_use_legacy_ownership() {
        let mut plan: LaunchPlan = serde_json::from_str(include_str!(
            "../../../nan-harness-core/tests/fixtures/launch-plan.direct.json"
        ))
        .unwrap();
        plan.harness.kind = HarnessKind::Hermes;
        for version in ["Hermes Agent v0.22.5 (2026.9.24)", "0.22.0", "unreadable"] {
            plan.harness.detected_version = version.into();
            assert!(uses_gateway(&plan));
        }
        plan.harness.detected_version = "Hermes Agent v0.21.0".into();
        assert!(!uses_gateway(&plan));
    }

    #[test]
    fn canonical_arguments_cannot_abbreviate_or_disable_the_required_routing_config() {
        for argument in [
            "--prov=other",
            "--mod=other",
            "--safe-mode",
            "--ignore-user-config",
        ] {
            let arguments =
                ["--provider", "nan", "--model", "qwen3.6", argument].map(str::to_owned);
            assert!(validate_arguments(&arguments).is_err());
        }
        let arguments = [
            "--provider",
            "nan",
            "--model",
            "qwen3.6",
            "chat",
            "--query=--safe-mode",
        ]
        .map(str::to_owned);
        validate_arguments(&arguments).unwrap();
    }

    #[test]
    fn managed_home_refuses_an_unmarked_directory_and_symlink() {
        let root = tempfile::tempdir().unwrap();
        let existing = root.path().join("existing");
        fs::create_dir(&existing).unwrap();
        fs::write(existing.join("user-state"), "preserve").unwrap();
        assert!(open_owned_home(&existing, "marker").is_err());
        assert_eq!(
            fs::read_to_string(existing.join("user-state")).unwrap(),
            "preserve"
        );
        #[cfg(unix)]
        {
            let link = root.path().join("link");
            std::os::unix::fs::symlink(&existing, &link).unwrap();
            assert!(open_owned_home(&link, "marker").is_err());
        }
    }

    #[test]
    fn managed_home_keeps_history_and_requires_its_exact_owner_marker() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("managed");
        open_owned_home(&home, "marker").unwrap();
        fs::write(home.join("state.db"), "history").unwrap();
        open_owned_home(&home, "marker").unwrap();
        assert!(open_owned_home(&home, "foreign").is_err());
        assert_eq!(
            fs::read_to_string(home.join("state.db")).unwrap(),
            "history"
        );
    }
}
