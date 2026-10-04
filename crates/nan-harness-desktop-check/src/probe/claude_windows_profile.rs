//! Exclusive source-defined third-party storage loan for a disposable Windows launch.
use super::{ProbeSpec, Reason};
use crate::native::Native;
use std::{
    fs::File,
    path::{Path, PathBuf},
    time::Instant,
};
const PROFILE_ID: &str = "6e616e68-6172-4e65-8000-000000000001";
const MAX_CONFIG: u64 = 65_536;

#[derive(Clone, Copy, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
enum SealStage {
    InitialCustody,
    NativePolicy,
    LibraryMetadata,
    LibraryLock,
    DocumentMetadata,
    DocumentOpen,
    DocumentPrivacy,
    DocumentLock,
    DocumentJson,
    ConfigurationValues,
    FinalCustody,
    Deadline,
    Completed,
}

struct SealObservation {
    stage: SealStage,
    document_index: Option<usize>,
    root_privacy: Option<nan_harness_private_fs::OwnedWindowsDacl>,
    library_privacy: Option<nan_harness_private_fs::OwnedWindowsDacl>,
    document_privacy: [Option<nan_harness_private_fs::OwnedWindowsDacl>; 3],
}
impl SealObservation {
    fn new() -> Self {
        Self {
            stage: SealStage::InitialCustody,
            document_index: None,
            root_privacy: None,
            library_privacy: None,
            document_privacy: [None; 3],
        }
    }
    fn record(&self, completed: bool) {
        crate::process::windows_correlation::record_profile_seal(&serde_json::json!({
            "schemaVersion":1,"mechanism":"claude-windows-profile-seal","diagnosticsOnly":true,
            "stage":self.stage,"documentIndex":self.document_index,"completed":completed,
            "rootPrivacy":self.root_privacy.map(privacy_label),
            "libraryPrivacy":self.library_privacy.map(privacy_label),
            "documentPrivacy":self.document_privacy.map(|p|p.map(privacy_label))
        }));
    }
}

pub(crate) struct FreshClaudeWindowsProfile {
    workspace: PathBuf,
    root: PathBuf,
    directories: Vec<File>,
    root_directory_index: usize,
    library_directory_index: Option<usize>,
    configuration: Vec<File>,
    native: Native,
}
fn privacy_label(value: nan_harness_private_fs::OwnedWindowsDacl) -> &'static str {
    use nan_harness_private_fs::OwnedWindowsDacl;
    match value {
        OwnedWindowsDacl::Protected => "protected",
        OwnedWindowsDacl::Inherited => "inherited",
        OwnedWindowsDacl::Unexpected => "unexpected",
        OwnedWindowsDacl::Unavailable => "unavailable",
    }
}
fn regular(path: &Path, directory: bool) -> bool {
    use std::os::windows::fs::MetadataExt as _;
    std::fs::symlink_metadata(path).is_ok_and(|m| {
        m.file_attributes() & 0x400 == 0 && if directory { m.is_dir() } else { m.is_file() }
    })
}
fn retained_directory_regular(file: &File) -> bool {
    use std::os::windows::fs::MetadataExt as _;
    file.metadata()
        .is_ok_and(|m| m.is_dir() && m.file_attributes() & 0x400 == 0)
}

fn lock_directory(path: &Path) -> std::io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt as _;
    // Share reads/writes so child files can be atomically renamed. Withhold
    // DELETE sharing to prevent deletion or renaming of this retained directory.
    // Writable metadata can change; reject reparse points again during custody proof.
    let file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(3)
        .custom_flags(0x0200_0000 | 0x0020_0000)
        .open(path)?;
    let metadata = file.metadata()?;
    use std::os::windows::fs::MetadataExt as _;
    if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "owned directory changed",
        ));
    }
    Ok(file)
}
fn command_matches(command: &tokio::process::Command, key: &str, path: &Path) -> bool {
    command.as_std().get_envs().any(|(name, value)| {
        name == key
            && value
                .and_then(|p| Path::new(p).canonicalize().ok())
                .as_deref()
                == Some(path)
    })
}
fn document(file: &mut File) -> Option<serde_json::Value> {
    use std::io::{Read as _, Seek as _};
    if file.metadata().ok()?.len() > MAX_CONFIG {
        return None;
    }
    file.rewind().ok()?;
    let mut bytes = zeroize::Zeroizing::new(Vec::new());
    file.take(MAX_CONFIG + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() <= MAX_CONFIG as usize)
        .then(|| serde_json::from_slice(&bytes).ok())
        .flatten()
}
fn configured(values: &[serde_json::Value], base: &str, token: &str) -> bool {
    use serde_json::Value;
    values.len() == 3
        && values[0].get("deploymentMode").and_then(Value::as_str) == Some("3p")
        && values[1].get("appliedId").and_then(Value::as_str) == Some(PROFILE_ID)
        && values[1].get("hybridPointer").is_none()
        && values[1]
            .get("entries")
            .and_then(Value::as_array)
            .is_some_and(|entries| {
                entries.len() == 1
                    && entries[0].get("id").and_then(Value::as_str) == Some(PROFILE_ID)
            })
        && values[2].get("inferenceProvider").and_then(Value::as_str) == Some("gateway")
        && values[2]
            .get("inferenceGatewayBaseUrl")
            .and_then(Value::as_str)
            == Some(base)
        && values[2]
            .get("inferenceGatewayApiKey")
            .and_then(Value::as_str)
            == Some(token)
        && values[2]
            .get("inferenceGatewayAuthScheme")
            .and_then(Value::as_str)
            == Some("bearer")
        && values[2]
            .get("disableDeploymentModeChooser")
            .and_then(Value::as_bool)
            == Some(true)
        && values[2].get("coworkTabEnabled").and_then(Value::as_bool) == Some(false)
        && [
            "bootstrapUrl",
            "bootstrapEnabled",
            "bootstrap",
            "selfHosted",
            "selfHostedUrl",
            "selfHostedToken",
            "inference",
            "authentication",
        ]
        .iter()
        .all(|key| values[2].get(key).is_none())
}
// The CLI validates both source-defined roots before writing CHAT_ONLY config.
// Neither root may contain prior state; the token owns both exclusive creations.
fn create_fresh_roots(roots: &[PathBuf; 2]) -> Result<(), Reason> {
    for root in roots {
        if root
            .try_exists()
            .map_err(|_| Reason::IsolationUnavailable)?
        {
            return Err(Reason::IsolationUnavailable);
        }
    }
    for root in roots {
        nan_harness_private_fs::create_private_dir(root)
            .map_err(|_| Reason::IsolationUnavailable)?;
    }
    Ok(())
}
impl FreshClaudeWindowsProfile {
    pub(crate) fn prepare(
        spec: &ProbeSpec,
        command: &tokio::process::Command,
        deadline: Instant,
    ) -> Result<Option<Self>, Reason> {
        if std::env::var_os("NANH_CLAUDE_WINDOWS_FRESH_PROFILE").is_none() {
            return Ok(None);
        }
        if std::env::var("NANH_CLAUDE_WINDOWS_FRESH_PROFILE").as_deref() != Ok("1")
            || spec.kind != nan_harness_core::DesktopHarnessKind::Claude
            || spec.session != crate::cli::SessionMode::GithubHosted
            || std::env::var("RUNNER_OS").as_deref() != Ok("Windows")
            || std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
            || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
            || std::env::var("NANH_CLAUDE_WINDOWS_SOURCE_POLICY").as_deref()
                != Ok("official-2.19675.0-97910a066871")
            || std::env::var("NANH_CLAUDE_WINDOWS_PROFILE_POLICY").as_deref() != Ok("private-env")
            || std::env::var("NANH_CLAUDE_WINDOWS_CHAT_ONLY").as_deref() != Ok("1")
            || std::env::var("NANH_DESKTOP_QUALIFICATION_MODE").as_deref() != Ok("startup-baseline")
            || command
                .as_std()
                .get_envs()
                .any(|(key, value)| key == "CLAUDE_USER_DATA_DIR" && value.is_some())
        {
            return Err(Reason::IsolationUnavailable);
        }
        let facts = std::env::var_os("NANH_DESKTOP_QUALIFICATION_FACTS")
            .map(PathBuf::from)
            .ok_or(Reason::IsolationUnavailable)?;
        if !facts.is_absolute() || facts.ancestors().any(|path| !regular(path, true)) {
            return Err(Reason::IsolationUnavailable);
        }
        if !spec.workspace.is_absolute()
            || spec.workspace.ancestors().any(|path| !regular(path, true))
        {
            return Err(Reason::IsolationUnavailable);
        }
        let workspace = spec
            .workspace
            .canonicalize()
            .map_err(|_| Reason::IsolationUnavailable)?;
        let local = workspace.join("profile/home/AppData/Local");
        let roaming = workspace.join("profile/home/AppData/Roaming");
        if !command_matches(command, "LOCALAPPDATA", &local)
            || !command_matches(command, "APPDATA", &roaming)
            || roaming
                .join("Claude-3p")
                .try_exists()
                .map_err(|_| Reason::IsolationUnavailable)?
        {
            return Err(Reason::IsolationUnavailable);
        }
        let root = local.join("Claude-3p");
        let normal_root = roaming.join("Claude");
        let mut directories = Vec::new();
        for parent in root
            .parent()
            .ok_or(Reason::IsolationUnavailable)?
            .ancestors()
        {
            if !regular(parent, true) {
                return Err(Reason::IsolationUnavailable);
            }
            directories.push(lock_directory(parent).map_err(|_| Reason::IsolationUnavailable)?);
        }
        let native = Native::new()?;
        if !native.claude_policy_absent(deadline) {
            return Err(Reason::IsolationUnavailable);
        }
        directories.push(lock_directory(&roaming).map_err(|_| Reason::IsolationUnavailable)?);
        // CHAT_ONLY observes both roots before apply_gateway writes their docs.
        create_fresh_roots(&[normal_root.clone(), root.clone()])?;
        directories.push(lock_directory(&normal_root).map_err(|_| Reason::IsolationUnavailable)?);
        directories.push(lock_directory(&root).map_err(|_| Reason::IsolationUnavailable)?);
        if Instant::now() >= deadline {
            return Err(Reason::BudgetExceeded);
        }
        Ok(Some(Self {
            workspace,
            root,
            root_directory_index: directories.len() - 1,
            library_directory_index: None,
            directories,
            configuration: Vec::new(),
            native,
        }))
    }
    pub(crate) fn seal_configuration(
        &mut self,
        base: &str,
        token: &str,
        deadline: Instant,
    ) -> Result<(), Reason> {
        let mut observation = SealObservation::new();
        let result = (|| {
            self.check_initial_seal_custody(deadline, &mut observation)?;
            let library = self.hold_configuration_library(&mut observation)?;
            let mut documents = Vec::new();
            for (index, path) in [
                self.root.join("claude_desktop_config.json"),
                library.join("_meta.json"),
                library.join(format!("{PROFILE_ID}.json")),
            ]
            .into_iter()
            .enumerate()
            {
                documents.push(self.seal_owned_document(
                    &path,
                    index,
                    deadline,
                    &mut observation,
                )?);
            }
            self.retain_sealed_configuration(documents, base, token, deadline, &mut observation)
        })();
        observation.record(result.is_ok());
        result
    }
    fn check_initial_seal_custody(
        &self,
        deadline: Instant,
        observation: &mut SealObservation,
    ) -> Result<(), Reason> {
        if !self.configuration.is_empty()
            || !self.directories.iter().all(retained_directory_regular)
        {
            return Err(Reason::IsolationUnavailable);
        }
        observation.root_privacy = Some(nan_harness_private_fs::classify_owned_windows_dacl(
            &self.directories[self.root_directory_index],
            nan_harness_private_fs::PrivatePathKind::Directory,
        ));
        if observation.root_privacy != Some(nan_harness_private_fs::OwnedWindowsDacl::Protected) {
            return Err(Reason::IsolationUnavailable);
        }
        observation.stage = SealStage::NativePolicy;
        if !self.native.claude_policy_absent(deadline) {
            return Err(Reason::IsolationUnavailable);
        }
        Ok(())
    }
    fn hold_configuration_library(
        &mut self,
        observation: &mut SealObservation,
    ) -> Result<PathBuf, Reason> {
        let library = self.root.join("configLibrary");
        observation.stage = SealStage::LibraryMetadata;
        if !regular(&library, true) {
            return Err(Reason::IsolationUnavailable);
        }
        observation.stage = SealStage::LibraryLock;
        self.directories
            .push(lock_directory(&library).map_err(|_| Reason::IsolationUnavailable)?);
        self.library_directory_index = Some(self.directories.len() - 1);
        observation.library_privacy = Some(nan_harness_private_fs::classify_owned_windows_dacl(
            self.directories
                .last()
                .ok_or(Reason::IsolationUnavailable)?,
            nan_harness_private_fs::PrivatePathKind::Directory,
        ));
        if !matches!(
            observation.library_privacy,
            Some(
                nan_harness_private_fs::OwnedWindowsDacl::Protected
                    | nan_harness_private_fs::OwnedWindowsDacl::Inherited
            )
        ) || !self.private_ancestors()
        {
            return Err(Reason::IsolationUnavailable);
        }
        Ok(library)
    }
    fn seal_owned_document(
        &self,
        path: &Path,
        index: usize,
        deadline: Instant,
        observation: &mut SealObservation,
    ) -> Result<(File, serde_json::Value), Reason> {
        use std::os::windows::fs::{MetadataExt as _, OpenOptionsExt as _};
        observation.document_index = Some(index);
        observation.stage = SealStage::DocumentMetadata;
        if !regular(path, false) {
            return Err(Reason::IsolationUnavailable);
        }
        if !self.private_ancestors() || Instant::now() >= deadline {
            return Err(Reason::IsolationUnavailable);
        }
        observation.stage = SealStage::DocumentOpen;
        // One read-only retained handle; no WRITE_DAC and no repairing reader.
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .access_mode(0x8002_0000)
            .share_mode(1)
            .custom_flags(0x0020_0000)
            .open(path)
            .map_err(|_| Reason::IsolationUnavailable)?;
        observation.stage = SealStage::DocumentLock;
        if !file
            .metadata()
            .is_ok_and(|m| m.is_file() && m.file_attributes() & 0x400 == 0)
        {
            return Err(Reason::IsolationUnavailable);
        }
        observation.stage = SealStage::DocumentPrivacy;
        let privacy = nan_harness_private_fs::classify_owned_windows_dacl(
            &file,
            nan_harness_private_fs::PrivatePathKind::File,
        );
        observation.document_privacy[index] = Some(privacy);
        if !matches!(
            privacy,
            nan_harness_private_fs::OwnedWindowsDacl::Protected
                | nan_harness_private_fs::OwnedWindowsDacl::Inherited
        ) || !self.private_ancestors()
            || Instant::now() >= deadline
        {
            return Err(Reason::IsolationUnavailable);
        }
        observation.stage = SealStage::DocumentJson;
        let value = document(&mut file).ok_or(Reason::IsolationUnavailable)?;
        if nan_harness_private_fs::classify_owned_windows_dacl(
            &file,
            nan_harness_private_fs::PrivatePathKind::File,
        ) != privacy
            || !self.private_ancestors()
            || Instant::now() >= deadline
        {
            return Err(Reason::IsolationUnavailable);
        }
        Ok((file, value))
    }
    fn retain_sealed_configuration(
        &mut self,
        documents: Vec<(File, serde_json::Value)>,
        base: &str,
        token: &str,
        deadline: Instant,
        observation: &mut SealObservation,
    ) -> Result<(), Reason> {
        use std::os::windows::fs::MetadataExt as _;
        let (files, values): (Vec<_>, Vec<_>) = documents.into_iter().unzip();
        observation.document_index = None;
        observation.stage = SealStage::ConfigurationValues;
        if !configured(&values, base, token) {
            return Err(Reason::IsolationUnavailable);
        }
        observation.stage = SealStage::FinalCustody;
        if !self.private_ancestors()
            || !files.iter().enumerate().all(|(index, file)| {
                file.metadata()
                    .is_ok_and(|m| m.is_file() && m.file_attributes() & 0x400 == 0)
                    && Some(nan_harness_private_fs::classify_owned_windows_dacl(
                        file,
                        nan_harness_private_fs::PrivatePathKind::File,
                    )) == observation.document_privacy[index]
            })
        {
            return Err(Reason::IsolationUnavailable);
        }
        observation.stage = SealStage::Deadline;
        if Instant::now() >= deadline {
            return Err(Reason::IsolationUnavailable);
        }
        self.configuration = files;
        observation.stage = SealStage::Completed;
        Ok(())
    }
    fn private_ancestors(&self) -> bool {
        owned_directory_custody(
            &self.directories,
            self.root_directory_index,
            self.library_directory_index,
        )
    }
    pub(crate) fn verifies_owned(&self, workspace: &Path, deadline: Instant) -> bool {
        use std::os::windows::fs::MetadataExt as _;
        self.configuration.len() == 3
            && workspace.canonicalize().ok().as_deref() == Some(self.workspace.as_path())
            && regular(&self.root, true)
            && self.private_ancestors()
            && self.configuration.iter().all(|f| {
                f.metadata()
                    .is_ok_and(|m| m.is_file() && m.file_attributes() & 0x400 == 0)
                    && matches!(
                        nan_harness_private_fs::classify_owned_windows_dacl(
                            f,
                            nan_harness_private_fs::PrivatePathKind::File
                        ),
                        nan_harness_private_fs::OwnedWindowsDacl::Protected
                            | nan_harness_private_fs::OwnedWindowsDacl::Inherited
                    )
            })
            && self.native.claude_policy_absent(deadline)
            && Instant::now() < deadline
    }
}

fn owned_directory_custody(
    directories: &[File],
    root_directory_index: usize,
    library_directory_index: Option<usize>,
) -> bool {
    use nan_harness_private_fs::{OwnedWindowsDacl as Dacl, PrivatePathKind};
    directories.iter().all(retained_directory_regular)
        && directories.get(root_directory_index).is_some_and(|f| {
            nan_harness_private_fs::classify_owned_windows_dacl(f, PrivatePathKind::Directory)
                == Dacl::Protected
        })
        && library_directory_index
            .and_then(|i| directories.get(i))
            .is_some_and(|f| {
                matches!(
                    nan_harness_private_fs::classify_owned_windows_dacl(
                        f,
                        PrivatePathKind::Directory
                    ),
                    Dacl::Protected | Dacl::Inherited
                )
            })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retained_owned_root_and_inherited_library_admit_only_readonly_exact_files() {
        use nan_harness_private_fs::{OwnedWindowsDacl as Dacl, PrivatePathKind};
        use std::os::windows::fs::OpenOptionsExt as _;
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("owned-profile");
        nan_harness_private_fs::create_private_dir(&root).unwrap();
        let library = root.join("configLibrary");
        std::fs::create_dir(&library).unwrap();
        let directories = vec![
            lock_directory(&root).unwrap(),
            lock_directory(&library).unwrap(),
        ];
        assert!(owned_directory_custody(&directories, 0, Some(1)));
        assert!(!owned_directory_custody(&directories, 0, None));
        let mut retained = Vec::new();
        for path in [
            root.join("claude_desktop_config.json"),
            library.join("_meta.json"),
            library.join(format!("{PROFILE_ID}.json")),
        ] {
            std::fs::write(&path, b"{\"ordinary\":true}").unwrap();
            let mut file = std::fs::OpenOptions::new()
                .read(true)
                .access_mode(0x8002_0000)
                .share_mode(1)
                .custom_flags(0x0020_0000)
                .open(&path)
                .unwrap();
            assert!(owned_directory_custody(&directories, 0, Some(1)));
            assert_eq!(
                nan_harness_private_fs::classify_owned_windows_dacl(&file, PrivatePathKind::File),
                Dacl::Inherited
            );
            assert_eq!(
                document(&mut file).unwrap(),
                serde_json::json!({"ordinary":true})
            );
            assert_eq!(
                std::fs::OpenOptions::new()
                    .write(true)
                    .open(&path)
                    .unwrap_err()
                    .raw_os_error(),
                Some(32)
            );
            assert_eq!(
                nan_harness_private_fs::classify_owned_windows_dacl(&file, PrivatePathKind::File),
                Dacl::Inherited
            );
            retained.push(file);
        }
        assert_eq!(retained.len(), 3);
        assert!(owned_directory_custody(&directories, 0, Some(1)));
        drop(retained);
    }
    fn valid() -> Vec<serde_json::Value> {
        vec![
            serde_json::json!({"deploymentMode":"3p"}),
            serde_json::json!({"appliedId":PROFILE_ID,"entries":[{"id":PROFILE_ID}]}),
            serde_json::json!({"inferenceProvider":"gateway","inferenceGatewayBaseUrl":"http://127.0.0.1:1","inferenceGatewayApiKey":"private-synthetic-sentinel","inferenceGatewayAuthScheme":"bearer","disableDeploymentModeChooser":true,"coworkTabEnabled":false}),
        ]
    }
    #[test]
    fn retained_directory_allows_child_rename_without_delete_sharing() {
        use std::io::Write as _;
        use std::os::windows::fs::{MetadataExt as _, OpenOptionsExt as _};
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let held = lock_directory(&root).unwrap();
        let source = root.join("source.json");
        let destination = root.join("destination.json");
        let mut file = nan_harness_private_fs::open_private_new(&source).unwrap();
        file.write_all(b"private-fixture").unwrap();
        file.sync_all().unwrap();
        drop(file);
        std::fs::rename(&source, &destination).unwrap();
        assert_eq!(std::fs::read(&destination).unwrap(), b"private-fixture");
        let readonly = std::fs::OpenOptions::new()
            .read(true)
            .access_mode(0x8002_0000)
            .share_mode(1)
            .custom_flags(0x0020_0000)
            .open(&destination)
            .unwrap();
        assert_eq!(
            nan_harness_private_fs::classify_owned_windows_dacl(
                &readonly,
                nan_harness_private_fs::PrivatePathKind::File
            ),
            nan_harness_private_fs::OwnedWindowsDacl::Protected
        );
        drop(readonly);
        let delete = std::fs::OpenOptions::new()
            .access_mode(0x0001_0000)
            .share_mode(7)
            .custom_flags(0x0200_0000 | 0x0020_0000)
            .open(&root)
            .unwrap_err();
        assert_eq!(delete.raw_os_error(), Some(32));
        let metadata = held.metadata().unwrap();
        assert!(metadata.is_dir() && metadata.file_attributes() & 0x400 == 0);
        drop(held);
    }
    #[test]
    fn rejects_stale_or_redirected_configuration() {
        let values = valid();
        assert!(configured(
            &values,
            "http://127.0.0.1:1",
            "private-synthetic-sentinel"
        ));
        for (i, key, value) in [
            (0, "deploymentMode", serde_json::json!("1p")),
            (1, "appliedId", serde_json::json!("other")),
            (
                1,
                "hybridPointer",
                serde_json::json!("https://example.invalid"),
            ),
            (1, "entries", serde_json::json!([])),
            (2, "inferenceProvider", serde_json::json!("other")),
            (2, "inferenceGatewayApiKey", serde_json::json!("stale")),
            (2, "coworkTabEnabled", serde_json::json!(true)),
            (
                2,
                "bootstrapUrl",
                serde_json::json!("https://example.invalid"),
            ),
        ] {
            let mut modified = values.clone();
            modified[i][key] = value;
            assert!(!configured(
                &modified,
                "http://127.0.0.1:1",
                "private-synthetic-sentinel"
            ));
        }
        assert!(!configured(
            &values,
            "http://127.0.0.1:2",
            "private-synthetic-sentinel"
        ));
    }
    #[test]
    fn rejects_oversized_and_malformed_documents() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config");
        std::fs::write(&path, b"{}").unwrap();
        assert!(document(&mut File::open(&path).unwrap()).is_some());
        std::fs::write(&path, b"not-json").unwrap();
        assert!(document(&mut File::open(&path).unwrap()).is_none());
        std::fs::write(&path, vec![b' '; MAX_CONFIG as usize + 1]).unwrap();
        assert!(document(&mut File::open(&path).unwrap()).is_none());
    }
    #[test]
    fn fresh_roots_exist_before_cli_scope_validation_and_never_adopt_state() {
        let dir = tempfile::tempdir().unwrap();
        let roots = [dir.path().join("Claude"), dir.path().join("Claude-3p")];
        create_fresh_roots(&roots).unwrap();
        assert!(roots.iter().all(|root| root.canonicalize().is_ok()));
        std::fs::write(roots[0].join("sentinel"), b"existing").unwrap();
        assert!(create_fresh_roots(&roots).is_err());
        assert_eq!(
            std::fs::read(roots[0].join("sentinel")).unwrap(),
            b"existing"
        );
        let other = [roots[0].clone(), dir.path().join("new-third-party")];
        assert!(create_fresh_roots(&other).is_err());
        assert!(!other[1].exists());
    }
    #[test]
    fn fresh_root_never_adopts_existing_state() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Claude-3p");
        nan_harness_private_fs::create_private_dir(&root).unwrap();
        std::fs::write(root.join("sentinel"), b"existing").unwrap();
        assert!(nan_harness_private_fs::create_private_dir(&root).is_err());
        assert_eq!(std::fs::read(root.join("sentinel")).unwrap(), b"existing");
        std::fs::remove_dir_all(&root).unwrap();
        std::fs::write(&root, b"file").unwrap();
        assert!(nan_harness_private_fs::create_private_dir(&root).is_err());
        std::fs::remove_file(&root).unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.path(), &root).unwrap();
            assert!(nan_harness_private_fs::create_private_dir(&root).is_err());
            std::fs::remove_file(root).unwrap();
        }
    }
}
