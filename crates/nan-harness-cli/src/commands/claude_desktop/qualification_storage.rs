//! Metadata-only observations in the disposable hosted desktop profile.
use super::DesktopPaths;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Presence {
    #[serde(flatten)]
    normal: NormalPresence,
    #[serde(flatten)]
    third_party: ThirdPartyPresence,
}
#[derive(Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct NormalPresence {
    claude_local_state: bool,
    claude_preferences: bool,
}
#[derive(Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThirdPartyPresence {
    third_party_local_state: bool,
    third_party_preferences: bool,
}
impl Presence {
    fn empty(self) -> bool {
        !self.normal.claude_local_state
            && !self.normal.claude_preferences
            && !self.third_party.third_party_local_state
            && !self.third_party.third_party_preferences
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Facts {
    schema_version: u8,
    mechanism: &'static str,
    diagnostics_only: bool,
    fresh_before: bool,
    observation_valid: bool,
    before: Presence,
    after: Presence,
}
pub(super) struct Snapshot {
    roots: [PathBuf; 2],
    directory: PathBuf,
    before: Presence,
}

fn private_directory(path: &Path) -> bool {
    super::qualification_config::private_directory(path)
}

// Walk only the fixed relative components; absent files are not read and
// symlinked parents cannot turn a diagnostic into access to foreign state.
fn present(root: &Path, components: &[&str]) -> Option<bool> {
    let mut path = root.canonicalize().ok()?;
    for (index, component) in components.iter().enumerate() {
        path.push(component);
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if path.canonicalize().ok().as_deref() != Some(path.as_path())
                    || metadata.file_type().is_symlink()
                    || (index + 1 == components.len() && !metadata.is_file())
                    || (index + 1 < components.len() && !metadata.is_dir())
                {
                    return None;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Some(false),
            Err(_) => return None,
        }
    }
    Some(true)
}
fn observe(roots: &[PathBuf; 2]) -> Option<Presence> {
    if roots.iter().any(|root| !private_directory(root)) {
        return None;
    }
    Some(Presence {
        normal: NormalPresence {
            claude_local_state: present(&roots[0], &["Local State"])?,
            claude_preferences: present(&roots[0], &["Default", "Preferences"])?,
        },
        third_party: ThirdPartyPresence {
            third_party_local_state: present(&roots[1], &["Local State"])?,
            third_party_preferences: present(&roots[1], &["Default", "Preferences"])?,
        },
    })
}
impl Snapshot {
    pub(super) fn capture(paths: &DesktopPaths) -> Option<Self> {
        if cfg!(windows) {
            let roots = super::qualification_config::windows_roots(paths)?;
            let directory = super::qualification_config::observation_directory(paths)?;
            return Some(Self {
                before: observe(&roots)?,
                roots,
                directory,
            });
        }
        let policy = std::env::var("NANH_CLAUDE_MAC_PROFILE_POLICY");
        let native = policy.as_deref() == Ok("native-known-folders");
        if !matches!(
            policy.as_deref(),
            Ok("electron-user-data-dir" | "native-known-folders")
        ) || std::env::var("NANH_DESKTOP_QUALIFICATION_MODE").as_deref()
            != Ok("startup-baseline")
            || std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
            || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
            || std::env::var("RUNNER_OS").as_deref() != Ok("macOS")
            || std::env::var_os("CLAUDE_USER_DATA_DIR").is_some()
            || std::env::var_os("CLAUDE_CDP_AUTH").is_some()
        {
            return None;
        }
        let workspace = std::env::current_dir().ok()?;
        let profile = workspace.join("profile");
        let home = if native {
            PathBuf::from(std::env::var_os("HOME")?)
        } else {
            profile.join("home")
        };
        let directory = PathBuf::from(std::env::var_os("NANH_DESKTOP_QUALIFICATION_FACTS")?);
        if std::env::var_os("HOME").map(PathBuf::from).as_ref() != Some(&home)
            || (!native && !private_directory(&home))
            || [&profile, &directory]
                .iter()
                .any(|path| !private_directory(path))
        {
            return None;
        }
        let support = home.join("Library/Application Support");
        let roots = [support.join("Claude"), support.join("Claude-3p")];
        if paths.normal_config != roots[0].join("claude_desktop_config.json")
            || paths.third_party_config != roots[1].join("claude_desktop_config.json")
        {
            return None;
        }
        Some(Self {
            before: observe(&roots)?,
            roots,
            directory,
        })
    }
    pub(super) fn record(&self) {
        if !private_directory(&self.directory) {
            return;
        }
        let after = observe(&self.roots);
        let facts = Facts {
            schema_version: 1,
            mechanism: "claude-storage-use",
            diagnostics_only: true,
            fresh_before: self.before.empty(),
            observation_valid: after.is_some(),
            before: self.before,
            after: after.unwrap_or_default(),
        };
        let path = self
            .directory
            .join(format!("claude-storage-use-{}.json", std::process::id()));
        if let Ok(file) = nan_harness_private_fs::open_private_new(&path) {
            let _ = serde_json::to_writer(file, &facts);
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt as _, symlink};
    #[test]
    fn observes_fixed_regular_presence_without_reading_private_payload() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let roots = [root.clone(), root.clone()];
        assert!(observe(&roots).unwrap().empty());
        std::fs::write(root.join("Local State"), "PRIVATE_SENTINEL").unwrap();
        std::fs::set_permissions(
            root.join("Local State"),
            std::fs::Permissions::from_mode(0o0),
        )
        .unwrap();
        let presence = observe(&roots).unwrap();
        assert!(presence.normal.claude_local_state);
        assert!(!presence.empty());
        assert!(
            !serde_json::to_string(&presence)
                .unwrap()
                .contains("PRIVATE_SENTINEL")
        );
        std::fs::create_dir(root.join("Default")).unwrap();
        std::fs::write(root.join("Default/Preferences"), "PRIVATE_SENTINEL").unwrap();
        assert!(observe(&roots).unwrap().third_party.third_party_preferences);
    }
    #[test]
    fn receipt_reports_unsafe_after_state_without_inference_or_payload() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let snapshot = Snapshot {
            roots: [root.clone(), root.clone()],
            directory: root.clone(),
            before: Presence::default(),
        };
        symlink(&root, root.join("Default")).unwrap();
        snapshot.record();
        let bytes =
            std::fs::read(root.join(format!("claude-storage-use-{}.json", std::process::id())))
                .unwrap();
        let facts: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(facts["freshBefore"], true);
        assert_eq!(facts["observationValid"], false);
        assert_eq!(facts["diagnosticsOnly"], true);
        assert!(
            !String::from_utf8(bytes)
                .unwrap()
                .contains(root.to_str().unwrap())
        );
    }
    #[test]
    fn rejects_symlinked_parent_and_nonregular_artifact() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        symlink(&root, root.join("Default")).unwrap();
        assert!(observe(&[root.clone(), root.clone()]).is_none());
        std::fs::remove_file(root.join("Default")).unwrap();
        std::fs::create_dir(root.join("Local State")).unwrap();
        assert!(observe(&[root.clone(), root]).is_none());
    }
}
