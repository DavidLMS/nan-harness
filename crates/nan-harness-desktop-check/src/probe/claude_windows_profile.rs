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
    BridgeAuthority,
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

#[derive(Clone, Copy, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
enum BridgeStage {
    RootCustody,
    ReceiptMetadata,
    ReceiptOpen,
    ReceiptLock,
    ReceiptPrivacy,
    ReceiptJson,
    ReceiptSchema,
    ReceiptValues,
    EndpointOwner,
    FinalCustody,
    Completed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
enum BridgeFailure {
    OriginalCutoff,
    RootCustody,
    ReceiptMissing,
    ReceiptMetadata,
    ReceiptOpen,
    ReceiptSharing,
    ReceiptLock,
    ReceiptPrivacy,
    ReceiptJson,
    ReceiptSchema,
    SchemaVersion,
    ProcessIdentity,
    TokenFormat,
    UrlParse,
    UrlPolicy,
    UrlPort,
    ProofEnvironment,
    ProofInput,
    ProofSpawn,
    ProofWait,
    ProofExit,
    ProofOutput,
    EndpointRejected,
}
#[derive(Default)]
struct BridgeObservation {
    stage: Option<BridgeStage>,
    failure: Option<BridgeFailure>,
    privacy: Option<nan_harness_private_fs::OwnedWindowsDacl>,
    endpoint_reason: Option<&'static str>,
}

struct SealObservation {
    stage: SealStage,
    bridge: BridgeObservation,
    document_index: Option<usize>,
    configuration_failure: Option<&'static str>,
    root_privacy: Option<nan_harness_private_fs::OwnedWindowsDacl>,
    library_privacy: Option<nan_harness_private_fs::OwnedWindowsDacl>,
    document_privacy: [Option<nan_harness_private_fs::OwnedWindowsDacl>; 3],
}
impl SealObservation {
    fn new() -> Self {
        Self {
            stage: SealStage::InitialCustody,
            bridge: BridgeObservation::default(),
            document_index: None,
            configuration_failure: None,
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
            "documentPrivacy":self.document_privacy.map(|p|p.map(privacy_label)),
            "configurationFailure":self.configuration_failure,
            "bridgeAuthority":{"stage":self.bridge.stage,"failure":self.bridge.failure,
                "privacy":self.bridge.privacy.map(privacy_label),"endpointReason":self.bridge.endpoint_reason}
        }));
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BridgeReceipt {
    schema_version: u8,
    process_id: u32,
    base_url: String,
    token: String,
}
impl Drop for BridgeReceipt {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.token);
    }
}
impl BridgeReceipt {
    fn validated_port(&self) -> Result<u16, BridgeFailure> {
        if self.schema_version != 1 {
            return Err(BridgeFailure::SchemaVersion);
        }
        if self.process_id <= 1 {
            return Err(BridgeFailure::ProcessIdentity);
        }
        if self.token.len() != 64
            || !self
                .token
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(BridgeFailure::TokenFormat);
        }
        let url = url::Url::parse(&self.base_url).map_err(|_| BridgeFailure::UrlParse)?;
        if url.scheme() != "http"
            || url.host_str() != Some("127.0.0.1")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(BridgeFailure::UrlPolicy);
        }
        url.port().filter(|p| *p > 1).ok_or(BridgeFailure::UrlPort)
    }
}

fn bridge_process_owned(
    port: u16,
    bridge: u32,
    launcher: u32,
    deadline: Instant,
) -> Result<&'static str, BridgeFailure> {
    use std::io::Read as _;
    use std::os::windows::process::CommandExt as _;
    use std::process::{Command, Stdio};
    let Some(python) = std::env::var_os("FEASIBILITY_WINDOWS_PROOF_PYTHON").map(PathBuf::from)
    else {
        return Err(BridgeFailure::ProofEnvironment);
    };
    let Some(script) = std::env::var_os("FEASIBILITY_WINDOWS_PROOF_SCRIPT").map(PathBuf::from)
    else {
        return Err(BridgeFailure::ProofEnvironment);
    };
    if [python.as_path(), script.as_path()]
        .iter()
        .any(|p| !p.is_absolute() || !p.is_file() || p.is_symlink())
        || launcher <= 1
        || Instant::now() >= deadline
    {
        return Err(if Instant::now() >= deadline {
            BridgeFailure::OriginalCutoff
        } else {
            BridgeFailure::ProofInput
        });
    }
    let mut command = Command::new(python);
    command
        .env_clear()
        .arg(script)
        .args([
            "bridge",
            &port.to_string(),
            &bridge.to_string(),
            &launcher.to_string(),
        ])
        .creation_flags(0x0800_0000)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", root);
    }
    let Ok(mut child) = command.spawn() else {
        return Err(BridgeFailure::ProofSpawn);
    };
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() && Instant::now() < deadline => break,
            Ok(Some(_)) => {
                return Err(if Instant::now() >= deadline {
                    BridgeFailure::OriginalCutoff
                } else {
                    BridgeFailure::ProofExit
                });
            }
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(5))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(if Instant::now() >= deadline {
                    BridgeFailure::OriginalCutoff
                } else {
                    BridgeFailure::ProofWait
                });
            }
        }
    }
    let Some(output) = child.stdout.take() else {
        return Err(BridgeFailure::ProofOutput);
    };
    let mut bytes = Vec::new();
    output
        .take(65)
        .read_to_end(&mut bytes)
        .map_err(|_| BridgeFailure::ProofOutput)?;
    if Instant::now() >= deadline {
        return Err(BridgeFailure::OriginalCutoff);
    }
    endpoint_reason(&bytes).ok_or(BridgeFailure::ProofOutput)
}

fn endpoint_reason(bytes: &[u8]) -> Option<&'static str> {
    Some(match bytes {
        b"true" => "owned",
        b"listener-missing" => "listener-missing",
        b"listener-ambiguous" => "listener-ambiguous",
        b"listener-nonloopback" => "listener-nonloopback",
        b"listener-owner-mismatch" => "listener-owner-mismatch",
        b"listener-changed" => "listener-changed",
        b"process-budget" => "process-budget",
        b"process-unavailable" => "process-unavailable",
        b"parent-unavailable" => "parent-unavailable",
        b"parent-reused" => "parent-reused",
        b"session-mismatch" => "session-mismatch",
        b"ancestry-cycle" => "ancestry-cycle",
        b"ancestry-limit" => "ancestry-limit",
        b"query-failed" => "query-failed",
        _ => return None,
    })
}

pub(crate) struct FreshClaudeWindowsProfile {
    workspace: PathBuf,
    root: PathBuf,
    directories: Vec<File>,
    root_directory_index: usize,
    library_directory_index: Option<usize>,
    configuration: Vec<File>,
    bridge_receipt: Option<File>,
    mcp_url: Option<String>,
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
fn configuration_failure(
    values: &[serde_json::Value],
    base: &str,
    token: &str,
) -> Option<&'static str> {
    use serde_json::Value;
    if values.len() != 3 {
        return Some("document-count");
    }
    for (value, key, expected, failure) in [
        (&values[0], "deploymentMode", "3p", "deployment-mode"),
        (&values[1], "appliedId", PROFILE_ID, "applied-profile"),
        (&values[2], "inferenceProvider", "gateway", "provider"),
        (&values[2], "inferenceGatewayBaseUrl", base, "base-url"),
        (
            &values[2],
            "inferenceGatewayApiKey",
            token,
            "authentication-key",
        ),
        (
            &values[2],
            "inferenceGatewayAuthScheme",
            "bearer",
            "authentication-scheme",
        ),
    ] {
        if value.get(key).and_then(Value::as_str) != Some(expected) {
            return Some(failure);
        }
    }
    if values[1].get("hybridPointer").is_some() {
        return Some("hybrid-pointer");
    }
    if !values[1]
        .get("entries")
        .and_then(Value::as_array)
        .is_some_and(|entries| {
            entries.len() == 1 && entries[0].get("id").and_then(Value::as_str) == Some(PROFILE_ID)
        })
    {
        return Some("profile-entries");
    }
    if values[2]
        .get("disableDeploymentModeChooser")
        .and_then(Value::as_bool)
        != Some(true)
    {
        return Some("deployment-chooser");
    }
    if values[2].get("coworkTabEnabled").and_then(Value::as_bool) != Some(false) {
        return Some("chat-only");
    }
    if [
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
    .any(|key| values[2].get(key).is_some())
    {
        return Some("alternate-configuration");
    }
    None
}
#[cfg(test)]
fn configured(values: &[serde_json::Value], base: &str, token: &str) -> bool {
    configuration_failure(values, base, token).is_none()
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
        let mcp_url = command
            .as_std()
            .get_envs()
            .find(|(key, _)| *key == super::read_fixture_http::ENDPOINT)
            .and_then(|(_, value)| value.and_then(|value| value.to_str()).map(str::to_owned));
        if std::env::var_os(super::read_fixture_http::POLICY).is_some() != mcp_url.is_some() {
            return Err(Reason::IsolationUnavailable);
        }
        Ok(Some(Self {
            workspace,
            root,
            root_directory_index: directories.len() - 1,
            library_directory_index: None,
            directories,
            configuration: Vec::new(),
            bridge_receipt: None,
            mcp_url,
            native,
        }))
    }
    pub(crate) fn seal_configuration(
        &mut self,
        launcher: u32,
        deadline: Instant,
    ) -> Result<(), Reason> {
        let mut observation = SealObservation::new();
        let result = (|| {
            self.check_initial_seal_custody(deadline, &mut observation)?;
            observation.stage = SealStage::BridgeAuthority;
            let (receipt_file, receipt) = self
                .seal_bridge_authority(launcher, deadline, &mut observation.bridge)
                .map_err(|failure| {
                    observation.bridge.failure = Some(failure);
                    Reason::IsolationUnavailable
                })?;
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
            self.retain_sealed_configuration(
                documents,
                &receipt.base_url,
                &receipt.token,
                deadline,
                &mut observation,
            )?;
            self.bridge_receipt = Some(receipt_file);
            Ok(())
        })();
        observation.record(result.is_ok());
        result
    }
    fn check_bridge_root(&self, deadline: Instant) -> Result<(), BridgeFailure> {
        if Instant::now() >= deadline {
            return Err(BridgeFailure::OriginalCutoff);
        }
        if !owned_root_custody(&self.directories, self.root_directory_index) {
            return Err(BridgeFailure::RootCustody);
        }
        if Instant::now() >= deadline {
            return Err(BridgeFailure::OriginalCutoff);
        }
        Ok(())
    }
    fn seal_bridge_authority(
        &self,
        launcher: u32,
        deadline: Instant,
        observation: &mut BridgeObservation,
    ) -> Result<(File, BridgeReceipt), BridgeFailure> {
        use std::os::windows::fs::{MetadataExt as _, OpenOptionsExt as _};
        observation.stage = Some(BridgeStage::RootCustody);
        self.check_bridge_root(deadline)?;
        let path = self.root.join(".nanh-bridge.private");
        observation.stage = Some(BridgeStage::ReceiptMetadata);
        let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                BridgeFailure::ReceiptMissing
            } else {
                BridgeFailure::ReceiptMetadata
            }
        })?;
        if !metadata.is_file() || metadata.file_attributes() & 0x400 != 0 {
            return Err(BridgeFailure::ReceiptMetadata);
        }
        self.check_bridge_root(deadline)?;
        observation.stage = Some(BridgeStage::ReceiptOpen);
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .access_mode(0x8002_0000)
            .share_mode(1)
            .custom_flags(0x0020_0000)
            .open(path)
            .map_err(|error| {
                if error.raw_os_error() == Some(32) {
                    BridgeFailure::ReceiptSharing
                } else {
                    BridgeFailure::ReceiptOpen
                }
            })?;
        observation.stage = Some(BridgeStage::ReceiptLock);
        if !file
            .metadata()
            .is_ok_and(|m| m.is_file() && m.file_attributes() & 0x400 == 0)
        {
            return Err(BridgeFailure::ReceiptLock);
        }
        observation.stage = Some(BridgeStage::ReceiptPrivacy);
        observation.privacy = Some(nan_harness_private_fs::classify_owned_windows_dacl(
            &file,
            nan_harness_private_fs::PrivatePathKind::File,
        ));
        if observation.privacy != Some(nan_harness_private_fs::OwnedWindowsDacl::Protected) {
            return Err(BridgeFailure::ReceiptPrivacy);
        }
        self.check_bridge_root(deadline)?;
        observation.stage = Some(BridgeStage::ReceiptJson);
        let value = document(&mut file).ok_or(BridgeFailure::ReceiptJson)?;
        observation.stage = Some(BridgeStage::ReceiptSchema);
        let receipt: BridgeReceipt =
            serde_json::from_value(value).map_err(|_| BridgeFailure::ReceiptSchema)?;
        observation.stage = Some(BridgeStage::ReceiptValues);
        let port = receipt.validated_port()?;
        self.check_bridge_root(deadline)?;
        observation.stage = Some(BridgeStage::EndpointOwner);
        let reason = bridge_process_owned(port, receipt.process_id, launcher, deadline)?;
        observation.endpoint_reason = Some(reason);
        if reason != "owned" {
            return Err(BridgeFailure::EndpointRejected);
        }
        observation.stage = Some(BridgeStage::FinalCustody);
        self.check_bridge_root(deadline)?;
        observation.privacy = Some(nan_harness_private_fs::classify_owned_windows_dacl(
            &file,
            nan_harness_private_fs::PrivatePathKind::File,
        ));
        if observation.privacy != Some(nan_harness_private_fs::OwnedWindowsDacl::Protected) {
            return Err(BridgeFailure::ReceiptPrivacy);
        }
        if !file
            .metadata()
            .is_ok_and(|m| m.is_file() && m.file_attributes() & 0x400 == 0)
        {
            return Err(BridgeFailure::ReceiptLock);
        }
        self.check_bridge_root(deadline)?;
        observation.stage = Some(BridgeStage::Completed);
        Ok((file, receipt))
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
        observation.configuration_failure = configuration_failure(&values, base, token);
        if let Some(url) = &self.mcp_url {
            let expected = serde_json::json!([{"name":"nanh-read-fixture","transport":"http","url":url,"toolPolicy":{"read_file":"allow"}}]);
            if values
                .get(2)
                .and_then(|value| value.get("managedMcpServers"))
                != Some(&expected)
            {
                observation.configuration_failure = Some("alternate-configuration");
            }
        }
        if observation.configuration_failure.is_some() {
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
        self.bridge_receipt.is_some()
            && self.configuration.len() == 3
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

fn owned_root_custody(directories: &[File], root_directory_index: usize) -> bool {
    directories.iter().all(retained_directory_regular)
        && directories.get(root_directory_index).is_some_and(|f| {
            nan_harness_private_fs::classify_owned_windows_dacl(
                f,
                nan_harness_private_fs::PrivatePathKind::Directory,
            ) == nan_harness_private_fs::OwnedWindowsDacl::Protected
        })
}

fn owned_directory_custody(
    directories: &[File],
    root_directory_index: usize,
    library_directory_index: Option<usize>,
) -> bool {
    use nan_harness_private_fs::{OwnedWindowsDacl as Dacl, PrivatePathKind};
    owned_root_custody(directories, root_directory_index)
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
    fn bridge_receipt_rejects_external_endpoints_credentials_and_unknown_fields() {
        let valid = serde_json::json!({"schemaVersion":1,"processId":40,
            "baseUrl":"http://127.0.0.1:43210","token":"a".repeat(64)});
        let receipt: BridgeReceipt = serde_json::from_value(valid.clone()).unwrap();
        assert_eq!(receipt.validated_port(), Ok(43210));
        for base in [
            "http://localhost:43210",
            "https://127.0.0.1:43210",
            "http://127.0.0.1:43210/v1",
            "http://u@127.0.0.1:43210",
            "http://127.0.0.1:43210?x=1",
            "http://127.0.0.1:43210#x",
        ] {
            let mut value = valid.clone();
            value["baseUrl"] = serde_json::json!(base);
            assert_eq!(
                serde_json::from_value::<BridgeReceipt>(value)
                    .unwrap()
                    .validated_port()
                    .ok(),
                None
            );
        }
        for token in ["A".repeat(64), "a".repeat(63), "g".repeat(64)] {
            let mut value = valid.clone();
            value["token"] = serde_json::json!(token);
            assert_eq!(
                serde_json::from_value::<BridgeReceipt>(value)
                    .unwrap()
                    .validated_port()
                    .ok(),
                None
            );
        }
        let mut value = valid.clone();
        value["schemaVersion"] = serde_json::json!(2);
        assert_eq!(
            serde_json::from_value::<BridgeReceipt>(value)
                .unwrap()
                .validated_port()
                .ok(),
            None
        );
        let mut value = valid;
        value["unknown"] = serde_json::json!(true);
        assert!(serde_json::from_value::<BridgeReceipt>(value).is_err());
    }

    #[test]
    fn bridge_values_and_diagnostics_distinguish_failures_without_private_values() {
        let valid = serde_json::json!({"schemaVersion":1,"processId":40,"baseUrl":"http://127.0.0.1:43210","token":"a".repeat(64)});
        for (key, value, expected) in [
            (
                "schemaVersion",
                serde_json::json!(2),
                BridgeFailure::SchemaVersion,
            ),
            (
                "processId",
                serde_json::json!(1),
                BridgeFailure::ProcessIdentity,
            ),
            (
                "token",
                serde_json::json!("private-invalid-token"),
                BridgeFailure::TokenFormat,
            ),
            (
                "baseUrl",
                serde_json::json!("private-invalid-url"),
                BridgeFailure::UrlParse,
            ),
            (
                "baseUrl",
                serde_json::json!("http://remote.invalid:43210"),
                BridgeFailure::UrlPolicy,
            ),
            (
                "baseUrl",
                serde_json::json!("http://127.0.0.1"),
                BridgeFailure::UrlPort,
            ),
        ] {
            let mut value_set = valid.clone();
            value_set[key] = value;
            assert_eq!(
                serde_json::from_value::<BridgeReceipt>(value_set)
                    .unwrap()
                    .validated_port(),
                Err(expected)
            );
            let diagnostic = serde_json::to_string(&expected).unwrap();
            assert!(!diagnostic.contains("127.0.0.1"));
            assert!(!diagnostic.contains("private-invalid"));
        }
        assert_eq!(endpoint_reason(b"true"), Some("owned"));
        assert_eq!(
            endpoint_reason(b"listener-owner-mismatch"),
            Some("listener-owner-mismatch")
        );
        for unknown in [
            b"true\n".as_slice(),
            b"unknown private native message",
            b"http://127.0.0.1:43210",
        ] {
            assert_eq!(endpoint_reason(unknown), None);
        }
        let receipt: BridgeReceipt = serde_json::from_value(valid).unwrap();
        assert_eq!(receipt.validated_port(), Ok(43210));
    }

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
        // Bridge admission precedes the library loan. Its original-root proof
        // must pass while the later complete configuration proof still fails.
        assert!(owned_root_custody(&directories[..1], 0));
        assert!(!owned_directory_custody(&directories[..1], 0, None));
        assert!(!owned_root_custody(&directories[..1], 1));
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
    fn changed_configuration_reports_only_closed_failure_labels() {
        let base = "http://127.0.0.1:1";
        let token = "private-synthetic-sentinel";
        assert_eq!(configuration_failure(&valid(), base, token), None);
        for (index, key, reason) in [
            (0, "deploymentMode", "deployment-mode"),
            (1, "appliedId", "applied-profile"),
            (2, "inferenceProvider", "provider"),
            (2, "inferenceGatewayBaseUrl", "base-url"),
            (2, "inferenceGatewayApiKey", "authentication-key"),
            (2, "inferenceGatewayAuthScheme", "authentication-scheme"),
            (1, "entries", "profile-entries"),
            (2, "disableDeploymentModeChooser", "deployment-chooser"),
            (2, "coworkTabEnabled", "chat-only"),
        ] {
            let mut values = valid();
            values[index][key] = serde_json::json!("private-invalid-value");
            assert_eq!(configuration_failure(&values, base, token), Some(reason));
        }
        let mut values = valid();
        values[1]["hybridPointer"] = serde_json::Value::Null;
        assert_eq!(
            configuration_failure(&values, base, token),
            Some("hybrid-pointer")
        );
        values = valid();
        values[2]["bootstrap"] = serde_json::json!({"private": "value"});
        assert_eq!(
            configuration_failure(&values, base, token),
            Some("alternate-configuration")
        );
        assert_eq!(
            configuration_failure(&[], base, token),
            Some("document-count")
        );
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
