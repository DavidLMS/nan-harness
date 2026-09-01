use crate::app::PenDesktopArgs;
use crate::commands::credentials;
use crate::commands::desktop::{
    DesktopSessionLock, DesktopStateError, create_private_directory, reject_symlink,
    remove_file_if_present, write_private_atomic,
};
use crate::commands::persistence::{PersistenceManager, config_directory, discover_models};
use crate::error::CliError;
use nan_harness_core::{
    CodingModelProfile, DesktopHarnessKind, DesktopLaunchPlan, DesktopTransport, ReasoningPolicy,
    SecretError, WebSearchPolicy,
};
use nan_harness_runtime::{
    BridgeDiagnostic, ChatGatewayError, DesktopCompatibilityStatus, ExecutionOutcome,
    ResolvedConfig, RunningChatCompletionsGateway, classify_desktop_version, desktop_compatibility,
    start_chat_completions_gateway,
};
use nan_harness_telemetry::diagnostic::{Diagnostic, DiagnosticReason};
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest as _, Sha256};
use std::env;
use std::fs;
use std::future::Future;
use std::io::{BufRead as _, ErrorKind, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
use thiserror::Error;
use tokio::net::TcpListener;

const PROVIDER_ID: &str = "nan";
const PROVIDER_NAME: &str = "NaN";
const DEFAULT_MODEL_ID: &str = "qwen3.6";
const SESSION_SCHEMA_VERSION: u8 = 1;
const PERSISTENT_SCHEMA_VERSION: u8 = 1;

#[derive(Debug, Clone)]
struct PenPaths {
    models: PathBuf,
    auth: PathBuf,
    state_directory: PathBuf,
    session_receipt: PathBuf,
    session_backup_directory: PathBuf,
    persistent_receipt: PathBuf,
    persistent_backup_directory: PathBuf,
}

impl PenPaths {
    fn from_environment() -> Result<Self, PenDesktopError> {
        let home = user_home().ok_or(PenDesktopError::MissingHomeDirectory)?;
        let state = config_directory()
            .ok_or(PenDesktopError::MissingStateDirectory)?
            .join("pen-desktop");
        Self::new(&home, &state)
    }

    fn new(home: &Path, state_directory: &Path) -> Result<Self, PenDesktopError> {
        if !home.is_absolute() || !state_directory.is_absolute() {
            return Err(PenDesktopError::InvalidPath);
        }
        let pencil_directory = home.join(".pencil");
        Ok(Self {
            models: pencil_directory.join("models.json"),
            auth: pencil_directory.join("agent-auth"),
            session_receipt: state_directory.join("session.json"),
            session_backup_directory: state_directory.join("session-backups"),
            persistent_receipt: state_directory.join("configuration.json"),
            persistent_backup_directory: state_directory.join("configuration-backups"),
            state_directory: state_directory.to_path_buf(),
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PenDocumentKind {
    Models,
    Auth,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FileSnapshot {
    existed: bool,
    original_sha256: Option<String>,
    backup_file: String,
    applied_file_sha256: String,
    applied_entry_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SessionReceipt {
    schema_version: u8,
    models: FileSnapshot,
    auth: FileSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PersistentReceipt {
    schema_version: u8,
    models_file_existed: bool,
    auth_file_existed: bool,
    models_backup: PersistentEntryBackup,
    auth_backup: PersistentEntryBackup,
    credential_fingerprint: String,
    applied_models_sha256: String,
    applied_auth_sha256: String,
    model_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PersistentEntryBackup {
    existed: bool,
    sha256: Option<String>,
    backup_file: String,
}

pub(crate) async fn run(
    arguments: &PenDesktopArgs,
    interactive: bool,
    bridge_diagnostics: &mut Vec<BridgeDiagnostic>,
) -> Result<i32, CliError> {
    if arguments.dry_run {
        return print_dry_run(arguments);
    }
    let paths = PenPaths::from_environment()?;
    let process = SystemPenProcess::new(arguments.executable.clone())?;
    if arguments.restore {
        let _lock =
            DesktopSessionLock::acquire(&paths.state_directory).map_err(PenDesktopError::from)?;
        if process.is_running()? {
            return Err(PenDesktopError::AlreadyRunning.into());
        }
        if restore_session(&paths)? {
            eprintln!("Pen Desktop configuration restored.");
        } else {
            eprintln!("No Pen Desktop session needs recovery.");
        }
        return Ok(0);
    }

    process.ensure_available()?;
    validate_compatibility(
        process.installed_version().as_ref(),
        arguments.allow_unsupported,
        arguments.allow_untested,
    )?;
    let _lock =
        DesktopSessionLock::acquire(&paths.state_directory).map_err(PenDesktopError::from)?;
    ensure_no_pending_session(&paths)?;
    if process.is_running()? {
        return Err(PenDesktopError::AlreadyRunning.into());
    }

    let mut launch_config =
        credentials::resolve_or_onboard(arguments.provider_base_url.clone(), interactive).await?;
    let models = match launch_config.model_catalog.take() {
        Some(models) => models,
        None => discover_models(&launch_config.config).await?,
    };
    let manager = PersistenceManager::from_environment()?;
    let remembered = if arguments.model.is_none() {
        manager
            .last_desktop_selection(DesktopHarnessKind::Pen)?
            .map(|selection| selection.model)
    } else {
        None
    };
    let selected_model = select_model(
        &models,
        arguments.model.as_deref().or(remembered.as_deref()),
    )?
    .to_owned();
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(PenDesktopError::BindGateway)?;
    let gateway =
        start_chat_completions_gateway(&launch_config.config, listener, &selected_model, false)
            .map_err(PenDesktopError::from)?;
    let result = run_managed_session(&paths, &process, &gateway, &models).await;
    let shutdown = gateway.shutdown_with_usage().await;

    match (result, shutdown) {
        (Err(error), _) => Err(error.into()),
        (Ok(code), Ok((diagnostics, usage))) => {
            for diagnostic in diagnostics {
                if !bridge_diagnostics.contains(&diagnostic) {
                    bridge_diagnostics.push(diagnostic);
                }
            }
            if let Err(error) =
                manager.save_last_desktop_selection(DesktopHarnessKind::Pen, &selected_model)
            {
                eprintln!("warning: could not save the last Pen model: {error}");
            }
            let outcome = if code == 0 {
                ExecutionOutcome::Succeeded
            } else {
                ExecutionOutcome::Failed
            };
            if let Some(summary) = crate::usage_summary::render_snapshot(&usage, outcome) {
                eprintln!("{summary}");
            }
            Ok(code)
        }
        (Ok(_), Err(error)) => Err(PenDesktopError::Gateway(error).into()),
    }
}

fn print_dry_run(arguments: &PenDesktopArgs) -> Result<i32, CliError> {
    let mut plan = DesktopLaunchPlan::new(
        DesktopHarnessKind::Pen,
        DesktopTransport::ChatCompletionsGateway,
    );
    plan.executable.clone_from(&arguments.executable);
    plan.selected_model.clone_from(&arguments.model);
    plan.web_search_policy = WebSearchPolicy::Disabled;
    plan.restore_only = arguments.restore;
    println!(
        "{}",
        serde_json::to_string_pretty(&plan).map_err(PenDesktopError::Serialize)?
    );
    Ok(0)
}

async fn run_managed_session(
    paths: &PenPaths,
    process: &SystemPenProcess,
    gateway: &RunningChatCompletionsGateway,
    models: &[CodingModelProfile],
) -> Result<i32, PenDesktopError> {
    let models_document = patched_models_document(
        read_json_object(&paths.models)?,
        &gateway.client_base_url(),
        models,
    )?;
    let auth_document = gateway
        .with_session_token(|token| patched_auth_document(read_json_object(&paths.auth)?, token))?;
    begin_session(paths, &models_document, &auth_document)?;
    match process.is_running() {
        Ok(false) => {}
        Ok(true) => return restore_after(paths, Err(PenDesktopError::AlreadyRunning)),
        Err(error) => return restore_after(paths, Err(error)),
    }
    if let Err(error) = process.launch() {
        return restore_after(paths, Err(error));
    }
    eprintln!(
        "Pen Desktop launched through NaN with {} available text models. Quit Pen to restore its previous configuration.",
        models.len()
    );
    let completion = wait_for_exit_or_signal(process).await;
    match completion {
        Ok(WaitOutcome::Exited) => restore_after(paths, Ok(0)),
        Ok(WaitOutcome::Signaled(code)) => {
            terminate_and_wait(process).await?;
            restore_after(paths, Ok(code))
        }
        Err(error) => {
            terminate_and_wait(process).await?;
            restore_after(paths, Err(error))
        }
    }
}

fn validate_compatibility(
    installed: Option<&Version>,
    allow_unsupported: bool,
    allow_untested: bool,
) -> Result<(), PenDesktopError> {
    let entry = desktop_compatibility(DesktopHarnessKind::Pen)?;
    match classify_desktop_version(&entry, installed) {
        DesktopCompatibilityStatus::Tested => Ok(()),
        DesktopCompatibilityStatus::ContractOnly => {
            eprintln!(
                "warning: Pen Desktop compatibility on this platform is contract-tested, not live-verified"
            );
            Ok(())
        }
        DesktopCompatibilityStatus::NewerUntested if allow_untested => {
            eprintln!("warning: this Pen Desktop version is newer than the live-verified version");
            Ok(())
        }
        DesktopCompatibilityStatus::NewerUntested => Err(PenDesktopError::NewerUntested),
        DesktopCompatibilityStatus::OlderUnsupported if allow_unsupported => {
            eprintln!("warning: this Pen Desktop version is older than the supported version");
            Ok(())
        }
        DesktopCompatibilityStatus::OlderUnsupported => Err(PenDesktopError::OlderUnsupported),
        DesktopCompatibilityStatus::Unavailable => Err(PenDesktopError::UnsupportedPlatform),
    }
}

fn select_model<'a>(
    models: &'a [CodingModelProfile],
    requested: Option<&str>,
) -> Result<&'a str, PenDesktopError> {
    let selected = requested.unwrap_or(DEFAULT_MODEL_ID);
    if let Some(model) = models.iter().find(|model| model.id == selected) {
        return Ok(&model.id);
    }
    if requested.is_some() {
        return Err(PenDesktopError::ModelUnavailable {
            model: selected.to_owned(),
            available: models.iter().map(|model| model.id.clone()).collect(),
        });
    }
    models
        .first()
        .map(|model| model.id.as_str())
        .ok_or(PenDesktopError::EmptyModelCatalog)
}

fn patched_models_document(
    mut root: Map<String, Value>,
    base_url: &str,
    models: &[CodingModelProfile],
) -> Result<Vec<u8>, PenDesktopError> {
    let providers = object_field_mut(&mut root, "providers", PenDocumentKind::Models)?;
    providers.insert(
        PROVIDER_ID.to_owned(),
        json!({
            "name": PROVIDER_NAME,
            "baseUrl": base_url,
            "api": "openai-completions",
            "models": models.iter().map(pen_model).collect::<Vec<_>>()
        }),
    );
    serialize_document(&root)
}

fn patched_auth_document(
    mut root: Map<String, Value>,
    api_key: &str,
) -> Result<Vec<u8>, PenDesktopError> {
    root.insert(
        PROVIDER_ID.to_owned(),
        json!({"type": "api_key", "key": api_key}),
    );
    serialize_document(&root)
}

fn pen_model(model: &CodingModelProfile) -> Value {
    let mut input = vec!["text"];
    if model.image_input {
        input.push("image");
    }
    json!({
        "id": model.id,
        "name": model.display_name,
        "reasoning": !matches!(model.reasoning, ReasoningPolicy::Unsupported | ReasoningPolicy::Unknown),
        "input": input,
        "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0},
        "contextWindow": model.context_window,
        "maxTokens": model.max_output_tokens
    })
}

fn object_field_mut<'a>(
    root: &'a mut Map<String, Value>,
    field: &'static str,
    document: PenDocumentKind,
) -> Result<&'a mut Map<String, Value>, PenDesktopError> {
    let value = root
        .entry(field.to_owned())
        .or_insert_with(|| Value::Object(Map::new()));
    value
        .as_object_mut()
        .ok_or(PenDesktopError::FieldNotObject { document, field })
}

fn serialize_document(root: &Map<String, Value>) -> Result<Vec<u8>, PenDesktopError> {
    let mut bytes = serde_json::to_vec_pretty(root).map_err(PenDesktopError::Serialize)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn read_json_object(path: &Path) -> Result<Map<String, Value>, PenDesktopError> {
    let Some(contents) = read_optional(path)? else {
        return Ok(Map::new());
    };
    serde_json::from_slice::<Value>(&contents)
        .map_err(|source| PenDesktopError::ParseDocument {
            path: path.to_path_buf(),
            source,
        })?
        .as_object()
        .cloned()
        .ok_or_else(|| PenDesktopError::DocumentRootNotObject(path.to_path_buf()))
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, PenDesktopError> {
    reject_symlink(path)?;
    match fs::read(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(source) => Err(PenDesktopError::ReadDocument {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn begin_session(
    paths: &PenPaths,
    models_document: &[u8],
    auth_document: &[u8],
) -> Result<(), PenDesktopError> {
    ensure_no_pending_session(paths)?;
    let models_original = read_optional(&paths.models)?;
    let auth_original = read_optional(&paths.auth)?;
    let models_entry = provider_entry(models_document, PenDocumentKind::Models)?;
    let auth_entry = provider_entry(auth_document, PenDocumentKind::Auth)?;
    create_private_directory(&paths.session_backup_directory)?;
    let captured = (|| {
        Ok::<_, PenDesktopError>(SessionReceipt {
            schema_version: SESSION_SCHEMA_VERSION,
            models: snapshot(
                &paths.session_backup_directory,
                "models.backup",
                models_original.as_deref(),
                models_document,
                &models_entry,
            )?,
            auth: snapshot(
                &paths.session_backup_directory,
                "auth.backup",
                auth_original.as_deref(),
                auth_document,
                &auth_entry,
            )?,
        })
    })();
    let receipt = match captured {
        Ok(receipt) => receipt,
        Err(error) => {
            cleanup_uncommitted_backups(paths);
            return Err(error);
        }
    };
    if let Err(error) = write_json_private(&paths.session_receipt, &receipt) {
        cleanup_uncommitted_backups(paths);
        return Err(error);
    }
    if let Err(error) = write_private_atomic(&paths.models, models_document)
        .and_then(|()| write_private_atomic(&paths.auth, auth_document))
    {
        let error = PenDesktopError::State(error);
        let _ = restore_session(paths);
        return Err(error);
    }
    Ok(())
}

fn cleanup_uncommitted_backups(paths: &PenPaths) {
    let _ = remove_file_if_present(&paths.session_backup_directory.join("models.backup"));
    let _ = remove_file_if_present(&paths.session_backup_directory.join("auth.backup"));
    let _ = fs::remove_dir(&paths.session_backup_directory);
}

fn snapshot(
    backup_directory: &Path,
    backup_file: &str,
    original: Option<&[u8]>,
    applied: &[u8],
    applied_entry: &Value,
) -> Result<FileSnapshot, PenDesktopError> {
    if let Some(original) = original {
        write_private_atomic(&backup_directory.join(backup_file), original)?;
    }
    Ok(FileSnapshot {
        existed: original.is_some(),
        original_sha256: original.map(sha256),
        backup_file: backup_file.to_owned(),
        applied_file_sha256: sha256(applied),
        applied_entry_sha256: hash_value(applied_entry)?,
    })
}

fn ensure_no_pending_session(paths: &PenPaths) -> Result<(), PenDesktopError> {
    if paths.session_receipt.exists() || paths.session_backup_directory.exists() {
        return Err(PenDesktopError::PendingRecovery);
    }
    Ok(())
}

fn restore_after(
    paths: &PenPaths,
    result: Result<i32, PenDesktopError>,
) -> Result<i32, PenDesktopError> {
    match (result, restore_session(paths)) {
        (Ok(code), Ok(_)) => Ok(code),
        (Err(error), Ok(_)) | (_, Err(error)) => Err(error),
    }
}

fn restore_session(paths: &PenPaths) -> Result<bool, PenDesktopError> {
    let Some(contents) = read_optional(&paths.session_receipt)? else {
        if paths.session_backup_directory.exists() {
            return Err(PenDesktopError::OrphanBackup);
        }
        return Ok(false);
    };
    let receipt: SessionReceipt =
        serde_json::from_slice(&contents).map_err(PenDesktopError::ParseReceipt)?;
    if receipt.schema_version != SESSION_SCHEMA_VERSION
        || receipt.models.backup_file != "models.backup"
        || receipt.auth.backup_file != "auth.backup"
    {
        return Err(PenDesktopError::InvalidReceipt);
    }
    restore_document(paths, PenDocumentKind::Models, &receipt.models)?;
    restore_document(paths, PenDocumentKind::Auth, &receipt.auth)?;
    remove_file_if_present(&paths.session_backup_directory.join("models.backup"))?;
    remove_file_if_present(&paths.session_backup_directory.join("auth.backup"))?;
    match fs::remove_dir(&paths.session_backup_directory) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(PenDesktopError::RemoveBackup(error)),
    }
    remove_file_if_present(&paths.session_receipt)?;
    Ok(true)
}

fn restore_document(
    paths: &PenPaths,
    kind: PenDocumentKind,
    snapshot: &FileSnapshot,
) -> Result<(), PenDesktopError> {
    let target = match kind {
        PenDocumentKind::Models => &paths.models,
        PenDocumentKind::Auth => &paths.auth,
    };
    let current = read_optional(target)?;
    if file_matches_original(current.as_deref(), snapshot) {
        return Ok(());
    }
    if current
        .as_deref()
        .is_some_and(|contents| sha256(contents) == snapshot.applied_file_sha256)
    {
        return restore_exact(paths, target, snapshot);
    }
    let Some(current) = current else {
        return Err(PenDesktopError::ManagedConfigurationChanged(target.clone()));
    };
    let current_entry = provider_entry(&current, kind)?;
    if hash_value(&current_entry)? != snapshot.applied_entry_sha256 {
        return Err(PenDesktopError::ManagedConfigurationChanged(target.clone()));
    }
    let original = read_snapshot(paths, snapshot)?;
    let replacement = merge_original_entry(&current, original.as_deref(), kind)?;
    write_private_atomic(target, &replacement)?;
    Ok(())
}

fn restore_exact(
    paths: &PenPaths,
    target: &Path,
    snapshot: &FileSnapshot,
) -> Result<(), PenDesktopError> {
    if let Some(original) = read_snapshot(paths, snapshot)? {
        write_private_atomic(target, &original)?;
    } else {
        remove_file_if_present(target)?;
    }
    Ok(())
}

fn read_snapshot(
    paths: &PenPaths,
    snapshot: &FileSnapshot,
) -> Result<Option<Vec<u8>>, PenDesktopError> {
    if !snapshot.existed {
        return Ok(None);
    }
    let contents = fs::read(paths.session_backup_directory.join(&snapshot.backup_file))
        .map_err(PenDesktopError::ReadBackup)?;
    if Some(sha256(&contents)) != snapshot.original_sha256 {
        return Err(PenDesktopError::BackupHashMismatch);
    }
    Ok(Some(contents))
}

fn file_matches_original(current: Option<&[u8]>, snapshot: &FileSnapshot) -> bool {
    match (
        current,
        snapshot.existed,
        snapshot.original_sha256.as_deref(),
    ) {
        (None, false, _) => true,
        (Some(current), true, Some(hash)) => sha256(current) == hash,
        _ => false,
    }
}

fn provider_entry(contents: &[u8], kind: PenDocumentKind) -> Result<Value, PenDesktopError> {
    let root: Value = serde_json::from_slice(contents).map_err(|source| {
        PenDesktopError::ParseManagedDocument {
            document: kind,
            source,
        }
    })?;
    match kind {
        PenDocumentKind::Models => root
            .get("providers")
            .and_then(|providers| providers.get(PROVIDER_ID)),
        PenDocumentKind::Auth => root.get(PROVIDER_ID),
    }
    .cloned()
    .ok_or(PenDesktopError::ManagedEntryMissing(kind))
}

fn merge_original_entry(
    current: &[u8],
    original: Option<&[u8]>,
    kind: PenDocumentKind,
) -> Result<Vec<u8>, PenDesktopError> {
    let mut current: Map<String, Value> = serde_json::from_slice::<Value>(current)
        .map_err(|source| PenDesktopError::ParseManagedDocument {
            document: kind,
            source,
        })?
        .as_object()
        .cloned()
        .ok_or(PenDesktopError::ManagedRootNotObject(kind))?;
    let previous = original
        .map(|contents| {
            serde_json::from_slice::<Value>(contents)
                .map_err(|source| PenDesktopError::ParseManagedDocument {
                    document: kind,
                    source,
                })
                .and_then(|value| {
                    value
                        .as_object()
                        .cloned()
                        .ok_or(PenDesktopError::ManagedRootNotObject(kind))
                })
        })
        .transpose()?;
    match kind {
        PenDocumentKind::Models => {
            let providers = object_field_mut(&mut current, "providers", kind)?;
            match previous
                .as_ref()
                .and_then(|root| root.get("providers"))
                .and_then(|providers| providers.get(PROVIDER_ID))
            {
                Some(value) => {
                    providers.insert(PROVIDER_ID.to_owned(), value.clone());
                }
                None => {
                    providers.remove(PROVIDER_ID);
                }
            }
        }
        PenDocumentKind::Auth => match previous.as_ref().and_then(|root| root.get(PROVIDER_ID)) {
            Some(value) => {
                current.insert(PROVIDER_ID.to_owned(), value.clone());
            }
            None => {
                current.remove(PROVIDER_ID);
            }
        },
    }
    serialize_document(&current)
}

fn hash_value(value: &Value) -> Result<String, PenDesktopError> {
    serde_json::to_vec(value)
        .map(|contents| sha256(&contents))
        .map_err(PenDesktopError::Serialize)
}

fn sha256(contents: &[u8]) -> String {
    let digest = Sha256::digest(contents);
    let mut result = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut result, "{byte:02x}");
    }
    result
}

fn write_json_private(path: &Path, value: &impl Serialize) -> Result<(), PenDesktopError> {
    let mut payload = serde_json::to_vec_pretty(value).map_err(PenDesktopError::Serialize)?;
    payload.push(b'\n');
    write_private_atomic(path, &payload)?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WaitOutcome {
    Exited,
    Signaled(i32),
}

async fn wait_for_exit_or_signal(
    process: &SystemPenProcess,
) -> Result<WaitOutcome, PenDesktopError> {
    let mut observed_running = false;
    let mut startup_polls = 0_u8;
    let signal = termination_signal();
    tokio::pin!(signal);
    loop {
        if process.is_running()? {
            observed_running = true;
        } else if observed_running {
            return Ok(WaitOutcome::Exited);
        } else {
            startup_polls = startup_polls.saturating_add(1);
            if startup_polls >= 40 {
                return Err(PenDesktopError::DidNotStart);
            }
        }
        if let Some(code) = wait_for_poll_or_signal(signal.as_mut()).await {
            return Ok(WaitOutcome::Signaled(code));
        }
    }
}

async fn wait_for_poll_or_signal<F>(signal: std::pin::Pin<&mut F>) -> Option<i32>
where
    F: Future<Output = i32>,
{
    tokio::select! {
        () = tokio::time::sleep(Duration::from_millis(125)) => None,
        code = signal => Some(code),
    }
}

async fn terminate_and_wait(process: &SystemPenProcess) -> Result<(), PenDesktopError> {
    let _ = process.terminate(false);
    for _ in 0..120 {
        if !process.is_running()? {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(125)).await;
    }
    process.terminate(true)?;
    for _ in 0..40 {
        if !process.is_running()? {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(125)).await;
    }
    Err(PenDesktopError::DidNotTerminate)
}

#[cfg(unix)]
async fn termination_signal() -> i32 {
    let Ok(mut terminate) =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
    else {
        let _ = tokio::signal::ctrl_c().await;
        return 130;
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => 130,
        _ = terminate.recv() => 143,
    }
}

#[cfg(not(unix))]
async fn termination_signal() -> i32 {
    let _ = tokio::signal::ctrl_c().await;
    130
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PenPlatform {
    Macos,
    Windows,
    Linux,
}

impl PenPlatform {
    fn current() -> Result<Self, PenDesktopError> {
        if cfg!(target_os = "macos") {
            Ok(Self::Macos)
        } else if cfg!(windows) {
            Ok(Self::Windows)
        } else if cfg!(target_os = "linux") {
            Ok(Self::Linux)
        } else {
            Err(PenDesktopError::UnsupportedPlatform)
        }
    }
}

struct SystemPenProcess {
    platform: PenPlatform,
    executable: Option<PathBuf>,
}

impl SystemPenProcess {
    fn new(executable: Option<PathBuf>) -> Result<Self, PenDesktopError> {
        Ok(Self {
            platform: PenPlatform::current()?,
            executable,
        })
    }

    fn ensure_available(&self) -> Result<(), PenDesktopError> {
        if self.resolve_executable().is_some() {
            Ok(())
        } else {
            Err(PenDesktopError::AppNotFound)
        }
    }

    fn resolve_executable(&self) -> Option<PathBuf> {
        if let Some(explicit) = &self.executable {
            if self.platform == PenPlatform::Macos && explicit.is_dir() {
                let executable = explicit.join("Contents/MacOS/Pen");
                return executable.is_file().then_some(executable);
            }
            return explicit.is_file().then(|| explicit.clone());
        }
        match self.platform {
            PenPlatform::Macos => find_macos_app().map(|app| app.join("Contents/MacOS/Pen")),
            PenPlatform::Windows => find_windows_app(),
            PenPlatform::Linux => find_on_path("pen").or_else(|| find_on_path("Pen")),
        }
    }

    fn installed_version(&self) -> Option<Version> {
        if self.platform != PenPlatform::Macos {
            return None;
        }
        let executable = self.resolve_executable()?;
        let app = executable.parent()?.parent()?.parent()?;
        let output = Command::new("/usr/bin/plutil")
            .args(["-extract", "CFBundleShortVersionString", "raw", "-o", "-"])
            .arg(app.join("Contents/Info.plist"))
            .output()
            .ok()?;
        output
            .status
            .success()
            .then(|| extract_semver(&String::from_utf8_lossy(&output.stdout)))
            .flatten()
    }

    fn launch(&self) -> Result<(), PenDesktopError> {
        let executable = self
            .resolve_executable()
            .ok_or(PenDesktopError::AppNotFound)?;
        let mut command = if self.platform == PenPlatform::Macos {
            let app = executable
                .parent()
                .and_then(Path::parent)
                .and_then(Path::parent)
                .ok_or(PenDesktopError::InvalidInstallation)?;
            let mut command = Command::new("/usr/bin/open");
            command.arg(app);
            command
        } else {
            Command::new(executable)
        };
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(PenDesktopError::Launch)
    }

    fn is_running(&self) -> Result<bool, PenDesktopError> {
        match self.platform {
            PenPlatform::Macos => {
                process_matches("/usr/bin/pgrep", &["-f", "Pen.app/Contents/MacOS/Pen"])
            }
            PenPlatform::Linux => Ok(process_matches("pgrep", &["-x", "Pen"])?
                || process_matches("pgrep", &["-x", "pen"])?),
            PenPlatform::Windows => {
                let output = Command::new("tasklist.exe")
                    .args(["/FI", "IMAGENAME eq Pen.exe", "/FO", "CSV", "/NH"])
                    .output()
                    .map_err(PenDesktopError::ProcessCheck)?;
                if !output.status.success() {
                    return Err(PenDesktopError::ProcessCheckFailed(output.status.code()));
                }
                Ok(String::from_utf8_lossy(&output.stdout)
                    .to_ascii_lowercase()
                    .contains("\"pen.exe\""))
            }
        }
    }

    fn terminate(&self, force: bool) -> Result<(), PenDesktopError> {
        if self.platform == PenPlatform::Linux {
            let signal = if force { "-KILL" } else { "-TERM" };
            for process_name in ["Pen", "pen"] {
                let status = Command::new("pkill")
                    .args([signal, "-x", process_name])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .map_err(PenDesktopError::Terminate)?;
                if !matches!(status.code(), Some(0 | 1)) {
                    return Err(PenDesktopError::TerminateFailed(status.code()));
                }
            }
            return Ok(());
        }
        let (command, arguments): (&str, Vec<&str>) = match self.platform {
            PenPlatform::Macos => (
                "/usr/bin/pkill",
                if force {
                    vec!["-KILL", "-f", "Pen.app/Contents/MacOS/Pen"]
                } else {
                    vec!["-TERM", "-f", "Pen.app/Contents/MacOS/Pen"]
                },
            ),
            PenPlatform::Linux => unreachable!("Linux termination returns above"),
            PenPlatform::Windows => (
                "taskkill.exe",
                if force {
                    vec!["/F", "/IM", "Pen.exe", "/T"]
                } else {
                    vec!["/IM", "Pen.exe", "/T"]
                },
            ),
        };
        let status = Command::new(command)
            .args(arguments)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(PenDesktopError::Terminate)?;
        if matches!(status.code(), Some(0 | 1)) {
            Ok(())
        } else {
            Err(PenDesktopError::TerminateFailed(status.code()))
        }
    }
}

fn process_matches(command: &str, arguments: &[&str]) -> Result<bool, PenDesktopError> {
    let status = Command::new(command)
        .args(arguments)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(PenDesktopError::ProcessCheck)?;
    match status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        code => Err(PenDesktopError::ProcessCheckFailed(code)),
    }
}

fn find_macos_app() -> Option<PathBuf> {
    let home = user_home()?;
    [
        PathBuf::from("/Applications/Pen.app"),
        home.join("Applications/Pen.app"),
    ]
    .into_iter()
    .find(|path| path.join("Contents/MacOS/Pen").is_file())
}

fn find_windows_app() -> Option<PathBuf> {
    let local = env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
    [
        local.join("Programs/Pen/Pen.exe"),
        local.join("Pen/Pen.exe"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    env::var_os("PATH")?
        .to_string_lossy()
        .split(if cfg!(windows) { ';' } else { ':' })
        .map(Path::new)
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
}

fn user_home() -> Option<PathBuf> {
    if cfg!(windows) {
        env::var_os("USERPROFILE").map(PathBuf::from)
    } else {
        env::var_os("HOME").map(PathBuf::from)
    }
}

fn extract_semver(output: &str) -> Option<Version> {
    output.split_whitespace().find_map(|candidate| {
        let candidate = candidate.trim_matches(|character: char| {
            !character.is_ascii_digit() && character != '.' && character != '-' && character != '+'
        });
        Version::parse(candidate).ok()
    })
}

pub(crate) fn persistent_configuration_exists() -> Result<bool, PenDesktopError> {
    let paths = PenPaths::from_environment()?;
    Ok(read_persistent_receipt(&paths)?.is_some())
}

pub(crate) fn persistent_configuration_active() -> Result<bool, PenDesktopError> {
    let paths = PenPaths::from_environment()?;
    persistent_configuration_active_at(&paths)
}

pub(crate) fn persistent_credential_is_current(
    saved_fingerprint: Option<&str>,
) -> Result<Option<bool>, PenDesktopError> {
    Ok(
        read_persistent_receipt(&PenPaths::from_environment()?)?.map(|receipt| {
            saved_fingerprint
                .is_some_and(|fingerprint| fingerprint == receipt.credential_fingerprint)
        }),
    )
}

fn persistent_configuration_active_at(paths: &PenPaths) -> Result<bool, PenDesktopError> {
    let Some(receipt) = read_persistent_receipt(paths)? else {
        return Ok(false);
    };
    let models = read_json_object(&paths.models)?;
    let auth = read_json_object(&paths.auth)?;
    Ok(models
        .get("providers")
        .and_then(|providers| providers.get(PROVIDER_ID))
        .map(hash_value)
        .transpose()?
        .as_deref()
        == Some(&receipt.applied_models_sha256)
        && auth
            .get(PROVIDER_ID)
            .map(hash_value)
            .transpose()?
            .as_deref()
            == Some(&receipt.applied_auth_sha256))
}

pub(crate) async fn configure_persistent(
    refresh: bool,
    confirmed: bool,
    interactive: bool,
) -> Result<usize, PenDesktopError> {
    let paths = PenPaths::from_environment()?;
    let _lock = DesktopSessionLock::acquire(&paths.state_directory)?;
    ensure_no_pending_session(&paths)?;
    let previous_receipt = read_persistent_receipt(&paths)?;
    if refresh && previous_receipt.is_none() {
        return Err(PenDesktopError::PersistentNotConfigured);
    }
    if previous_receipt.is_some() && !persistent_configuration_active_at(&paths)? {
        return Err(PenDesktopError::PersistentConfigurationChanged);
    }
    if previous_receipt.is_none() && !confirmed && !confirm_persistent(interactive, &paths)? {
        return Err(PenDesktopError::ConfigurationCancelled);
    }
    let (config, models) = credentials::resolve_saved_or_onboard(None, interactive).await?;
    apply_persistent_configuration(&paths, &config, &models, previous_receipt.as_ref())?;
    Ok(models.len())
}

pub(crate) fn refresh_persistent_with_config(
    config: &ResolvedConfig,
    models: &[CodingModelProfile],
) -> Result<bool, PenDesktopError> {
    let paths = PenPaths::from_environment()?;
    let _lock = DesktopSessionLock::acquire(&paths.state_directory)?;
    ensure_no_pending_session(&paths)?;
    let Some(previous_receipt) = read_persistent_receipt(&paths)? else {
        return Ok(false);
    };
    if !persistent_configuration_active_at(&paths)? {
        return Err(PenDesktopError::PersistentConfigurationChanged);
    }
    apply_persistent_configuration(&paths, config, models, Some(&previous_receipt))?;
    Ok(true)
}

fn apply_persistent_configuration(
    paths: &PenPaths,
    config: &ResolvedConfig,
    models: &[CodingModelProfile],
    previous_receipt: Option<&PersistentReceipt>,
) -> Result<(), PenDesktopError> {
    let models_root = read_json_object(&paths.models)?;
    let auth_root = read_json_object(&paths.auth)?;
    let models_file_existed =
        previous_receipt.map_or(paths.models.exists(), |receipt| receipt.models_file_existed);
    let auth_file_existed =
        previous_receipt.map_or(paths.auth.exists(), |receipt| receipt.auth_file_existed);
    let first_configuration = previous_receipt.is_none();
    let (models_backup, auth_backup) = if let Some(receipt) = previous_receipt {
        (receipt.models_backup.clone(), receipt.auth_backup.clone())
    } else {
        create_private_directory(&paths.persistent_backup_directory)?;
        let result = (|| {
            Ok::<_, PenDesktopError>((
                backup_persistent_entry(
                    paths,
                    "models-provider.json",
                    models_root
                        .get("providers")
                        .and_then(|providers| providers.get(PROVIDER_ID)),
                )?,
                backup_persistent_entry(paths, "auth-entry.json", auth_root.get(PROVIDER_ID))?,
            ))
        })();
        match result {
            Ok(backups) => backups,
            Err(error) => {
                cleanup_uncommitted_persistent_backups(paths);
                return Err(error);
            }
        }
    };
    let models_document = patched_models_document(models_root, &config.provider_base_url, models)?;
    let auth_document = config
        .secrets
        .with_secret(&config.provider_credential_ref, |api_key| {
            patched_auth_document(auth_root, api_key)
        })
        .map_err(PenDesktopError::Secret)??;
    let models_entry = provider_entry(&models_document, PenDocumentKind::Models)?;
    let auth_entry = provider_entry(&auth_document, PenDocumentKind::Auth)?;
    let receipt = PersistentReceipt {
        schema_version: PERSISTENT_SCHEMA_VERSION,
        models_file_existed,
        auth_file_existed,
        models_backup,
        auth_backup,
        credential_fingerprint: credentials::credential_fingerprint(config)?,
        applied_models_sha256: hash_value(&models_entry)?,
        applied_auth_sha256: hash_value(&auth_entry)?,
        model_ids: models.iter().map(|model| model.id.clone()).collect(),
    };
    let old_models = read_optional(&paths.models)?;
    let old_auth = read_optional(&paths.auth)?;
    let old_receipt = read_optional(&paths.persistent_receipt)?;
    if let Err(error) = write_json_private(&paths.persistent_receipt, &receipt) {
        if first_configuration {
            cleanup_uncommitted_persistent_backups(paths);
        }
        return Err(error);
    }
    if let Err(error) = write_private_atomic(&paths.models, &models_document)
        .and_then(|()| write_private_atomic(&paths.auth, &auth_document))
    {
        let _ = restore_optional_file(&paths.models, old_models.as_deref());
        let _ = restore_optional_file(&paths.auth, old_auth.as_deref());
        let _ = restore_optional_file(&paths.persistent_receipt, old_receipt.as_deref());
        if first_configuration {
            cleanup_uncommitted_persistent_backups(paths);
        }
        return Err(PenDesktopError::State(error));
    }
    Ok(())
}

fn backup_persistent_entry(
    paths: &PenPaths,
    backup_file: &str,
    value: Option<&Value>,
) -> Result<PersistentEntryBackup, PenDesktopError> {
    let contents = value
        .map(serde_json::to_vec)
        .transpose()
        .map_err(PenDesktopError::Serialize)?;
    if let Some(contents) = contents.as_deref() {
        write_private_atomic(
            &paths.persistent_backup_directory.join(backup_file),
            contents,
        )?;
    }
    Ok(PersistentEntryBackup {
        existed: contents.is_some(),
        sha256: contents.as_deref().map(sha256),
        backup_file: backup_file.to_owned(),
    })
}

fn read_persistent_backup(
    paths: &PenPaths,
    backup: &PersistentEntryBackup,
) -> Result<Option<Value>, PenDesktopError> {
    if !backup.existed {
        return Ok(None);
    }
    let contents = fs::read(paths.persistent_backup_directory.join(&backup.backup_file))
        .map_err(PenDesktopError::ReadBackup)?;
    if Some(sha256(&contents)) != backup.sha256 {
        return Err(PenDesktopError::BackupHashMismatch);
    }
    serde_json::from_slice(&contents)
        .map(Some)
        .map_err(PenDesktopError::ParseReceipt)
}

fn remove_persistent_backups(paths: &PenPaths) -> Result<(), PenDesktopError> {
    remove_file_if_present(
        &paths
            .persistent_backup_directory
            .join("models-provider.json"),
    )?;
    remove_file_if_present(&paths.persistent_backup_directory.join("auth-entry.json"))?;
    match fs::remove_dir(&paths.persistent_backup_directory) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(PenDesktopError::RemoveBackup(error)),
    }
}

fn cleanup_uncommitted_persistent_backups(paths: &PenPaths) {
    let _ = remove_persistent_backups(paths);
}

pub(crate) fn remove_persistent_configuration() -> Result<bool, PenDesktopError> {
    let paths = PenPaths::from_environment()?;
    let _lock = DesktopSessionLock::acquire(&paths.state_directory)?;
    remove_persistent_configuration_at(&paths)
}

fn remove_persistent_configuration_at(paths: &PenPaths) -> Result<bool, PenDesktopError> {
    ensure_no_pending_session(paths)?;
    let Some(receipt) = read_persistent_receipt(paths)? else {
        return Ok(false);
    };
    let previous_models_provider = read_persistent_backup(paths, &receipt.models_backup)?;
    let previous_auth = read_persistent_backup(paths, &receipt.auth_backup)?;
    let models_state = persistent_entry_state(
        &paths.models,
        PenDocumentKind::Models,
        &receipt.applied_models_sha256,
        previous_models_provider.as_ref(),
    )?;
    let auth_state = persistent_entry_state(
        &paths.auth,
        PenDocumentKind::Auth,
        &receipt.applied_auth_sha256,
        previous_auth.as_ref(),
    )?;
    if models_state == PersistentEntryState::Changed || auth_state == PersistentEntryState::Changed
    {
        return Err(PenDesktopError::PersistentConfigurationChanged);
    }
    if models_state == PersistentEntryState::Applied {
        restore_persistent_entry(
            &paths.models,
            PenDocumentKind::Models,
            previous_models_provider.as_ref(),
            receipt.models_file_existed,
        )?;
    }
    if auth_state == PersistentEntryState::Applied {
        restore_persistent_entry(
            &paths.auth,
            PenDocumentKind::Auth,
            previous_auth.as_ref(),
            receipt.auth_file_existed,
        )?;
    }
    remove_persistent_backups(paths)?;
    remove_file_if_present(&paths.persistent_receipt)?;
    Ok(true)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PersistentEntryState {
    Applied,
    Previous,
    Changed,
}

fn persistent_entry_state(
    path: &Path,
    kind: PenDocumentKind,
    applied_sha256: &str,
    previous: Option<&Value>,
) -> Result<PersistentEntryState, PenDesktopError> {
    let root = read_json_object(path)?;
    let current = match kind {
        PenDocumentKind::Models => root
            .get("providers")
            .and_then(|providers| providers.get(PROVIDER_ID)),
        PenDocumentKind::Auth => root.get(PROVIDER_ID),
    };
    if current
        .map(hash_value)
        .transpose()?
        .is_some_and(|hash| hash == applied_sha256)
    {
        return Ok(PersistentEntryState::Applied);
    }
    let previous_matches = match (current, previous) {
        (None, None) => true,
        (Some(current), Some(previous)) => hash_value(current)? == hash_value(previous)?,
        _ => false,
    };
    Ok(if previous_matches {
        PersistentEntryState::Previous
    } else {
        PersistentEntryState::Changed
    })
}

pub(crate) fn persistent_model_count() -> Result<Option<usize>, PenDesktopError> {
    persistent_model_count_at(&PenPaths::from_environment()?)
}

fn persistent_model_count_at(paths: &PenPaths) -> Result<Option<usize>, PenDesktopError> {
    Ok(read_persistent_receipt(paths)?.map(|receipt| receipt.model_ids.len()))
}

fn read_persistent_receipt(paths: &PenPaths) -> Result<Option<PersistentReceipt>, PenDesktopError> {
    let Some(contents) = read_optional(&paths.persistent_receipt)? else {
        if paths.persistent_backup_directory.exists() {
            return Err(PenDesktopError::OrphanPersistentBackup);
        }
        return Ok(None);
    };
    let receipt: PersistentReceipt =
        serde_json::from_slice(&contents).map_err(PenDesktopError::ParseReceipt)?;
    if receipt.schema_version != PERSISTENT_SCHEMA_VERSION
        || receipt.models_backup.backup_file != "models-provider.json"
        || receipt.auth_backup.backup_file != "auth-entry.json"
    {
        return Err(PenDesktopError::InvalidReceipt);
    }
    let _ = read_persistent_backup(paths, &receipt.models_backup)?;
    let _ = read_persistent_backup(paths, &receipt.auth_backup)?;
    Ok(Some(receipt))
}

fn restore_persistent_entry(
    path: &Path,
    kind: PenDocumentKind,
    previous: Option<&Value>,
    original_file_existed: bool,
) -> Result<(), PenDesktopError> {
    let mut root = read_json_object(path)?;
    match kind {
        PenDocumentKind::Models => {
            let providers = object_field_mut(&mut root, "providers", kind)?;
            match previous {
                Some(value) => {
                    providers.insert(PROVIDER_ID.to_owned(), value.clone());
                }
                None => {
                    providers.remove(PROVIDER_ID);
                }
            }
            if providers.is_empty() {
                root.remove("providers");
            }
        }
        PenDocumentKind::Auth => match previous {
            Some(value) => {
                root.insert(PROVIDER_ID.to_owned(), value.clone());
            }
            None => {
                root.remove(PROVIDER_ID);
            }
        },
    }
    if !original_file_existed && root.is_empty() {
        remove_file_if_present(path)?;
    } else {
        write_private_atomic(path, &serialize_document(&root)?)?;
    }
    Ok(())
}

fn restore_optional_file(path: &Path, contents: Option<&[u8]>) -> Result<(), PenDesktopError> {
    match contents {
        Some(contents) => write_private_atomic(path, contents)?,
        None => remove_file_if_present(path)?,
    }
    Ok(())
}

fn confirm_persistent(interactive: bool, paths: &PenPaths) -> Result<bool, PenDesktopError> {
    if !interactive {
        return Err(PenDesktopError::ConfirmationRequired);
    }
    eprintln!("nan-harness will add a persistent NaN provider to Pen Desktop.");
    eprintln!("The saved NaN API key will be copied into Pen's native credential file.");
    eprintln!("Managed files:");
    eprintln!("  - {}", paths.models.display());
    eprintln!("  - {}", paths.auth.display());
    let mut output = std::io::stderr().lock();
    write!(output, "Continue? [y/N] ").map_err(PenDesktopError::Prompt)?;
    output.flush().map_err(PenDesktopError::Prompt)?;
    let mut response = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut response)
        .map_err(PenDesktopError::Prompt)?;
    Ok(matches!(
        response.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

#[derive(Debug, Error)]
pub(crate) enum PenDesktopError {
    #[error("Pen Desktop integration is available only on macOS, Windows, and Linux")]
    UnsupportedPlatform,
    #[error(transparent)]
    Compatibility(#[from] nan_harness_runtime::DesktopCompatibilityError),
    #[error(
        "this Pen Desktop version is older than the supported version; retry with --allow-unsupported only if you accept the risk"
    )]
    OlderUnsupported,
    #[error(
        "this Pen Desktop version is newer than the live-verified version; retry with --allow-untested only if you accept the risk"
    )]
    NewerUntested,
    #[error("Pen Desktop was not found; install it from https://www.pen.dev or pass --executable")]
    AppNotFound,
    #[error("the Pen Desktop installation layout is invalid")]
    InvalidInstallation,
    #[error("Pen Desktop is already running; quit it completely before continuing")]
    AlreadyRunning,
    #[error("an interrupted Pen session needs recovery; quit Pen and run `nan pen --restore`")]
    PendingRecovery,
    #[error(
        "a Pen session backup exists without a valid receipt; inspect the private nan-harness state before continuing"
    )]
    OrphanBackup,
    #[error(
        "a persistent Pen backup exists without a valid receipt; inspect the private nan-harness state before continuing"
    )]
    OrphanPersistentBackup,
    #[error("Pen Desktop did not start; its previous configuration was restored")]
    DidNotStart,
    #[error("Pen Desktop did not terminate; quit it completely and run `nan pen --restore`")]
    DidNotTerminate,
    #[error("could not determine the current user's home directory")]
    MissingHomeDirectory,
    #[error("could not determine the nan-harness state directory")]
    MissingStateDirectory,
    #[error("a Pen Desktop path is not absolute")]
    InvalidPath,
    #[error("could not bind the private Pen gateway: {0}")]
    BindGateway(std::io::Error),
    #[error(transparent)]
    Gateway(#[from] ChatGatewayError),
    #[error(transparent)]
    State(#[from] DesktopStateError),
    #[error("model '{model}' is not available; available models: {}", available.join(", "))]
    ModelUnavailable {
        model: String,
        available: Vec<String>,
    },
    #[error("the NaN text-model catalog is empty")]
    EmptyModelCatalog,
    #[error("could not read Pen configuration '{}': {source}", path.display())]
    ReadDocument {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Pen configuration '{}' is not valid JSON: {source}", path.display())]
    ParseDocument {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("Pen configuration '{}' must contain a JSON object", .0.display())]
    DocumentRootNotObject(PathBuf),
    #[error("the {document:?} configuration field '{field}' must contain a JSON object")]
    FieldNotObject {
        document: PenDocumentKind,
        field: &'static str,
    },
    #[error("could not serialize Pen configuration: {0}")]
    Serialize(serde_json::Error),
    #[error("the managed {document:?} configuration is not valid JSON: {source}")]
    ParseManagedDocument {
        document: PenDocumentKind,
        source: serde_json::Error,
    },
    #[error("the managed {0:?} configuration must contain a JSON object")]
    ManagedRootNotObject(PenDocumentKind),
    #[error("the managed NaN entry is missing from the {0:?} configuration")]
    ManagedEntryMissing(PenDocumentKind),
    #[error("'{}' changed while Pen was open; refusing to overwrite those changes", .0.display())]
    ManagedConfigurationChanged(PathBuf),
    #[error("the private Pen receipt is invalid: {0}")]
    ParseReceipt(serde_json::Error),
    #[error("the private Pen receipt schema or targets are invalid")]
    InvalidReceipt,
    #[error("could not read a private Pen backup: {0}")]
    ReadBackup(std::io::Error),
    #[error("a private Pen backup does not match its receipt hash")]
    BackupHashMismatch,
    #[error("could not remove private Pen backups: {0}")]
    RemoveBackup(std::io::Error),
    #[error("could not launch Pen Desktop: {0}")]
    Launch(std::io::Error),
    #[error("could not inspect the Pen Desktop process: {0}")]
    ProcessCheck(std::io::Error),
    #[error("the Pen Desktop process check failed with exit code {0:?}")]
    ProcessCheckFailed(Option<i32>),
    #[error("could not terminate Pen Desktop: {0}")]
    Terminate(std::io::Error),
    #[error("Pen Desktop termination failed with exit code {0:?}")]
    TerminateFailed(Option<i32>),
    #[error("the saved credential could not be read: {0}")]
    Secret(SecretError),
    #[error("Pen Desktop is not configured by nan-harness")]
    PersistentNotConfigured,
    #[error("Pen Desktop's managed provider changed; refusing to overwrite user changes")]
    PersistentConfigurationChanged,
    #[error("persistent Pen configuration requires an interactive confirmation or --yes")]
    ConfirmationRequired,
    #[error("persistent Pen configuration was cancelled")]
    ConfigurationCancelled,
    #[error("could not read confirmation: {0}")]
    Prompt(std::io::Error),
    #[error(transparent)]
    Credential(#[from] credentials::CredentialError),
    #[error(transparent)]
    Persistence(#[from] crate::commands::persistence::PersistenceError),
}

impl PenDesktopError {
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::Gateway(error) => error.code(),
            Self::UnsupportedPlatform
            | Self::Compatibility(_)
            | Self::OlderUnsupported
            | Self::NewerUntested
            | Self::AppNotFound
            | Self::InvalidInstallation => "NH-PEN-001",
            Self::AlreadyRunning
            | Self::PendingRecovery
            | Self::OrphanBackup
            | Self::OrphanPersistentBackup
            | Self::ManagedConfigurationChanged(_)
            | Self::PersistentConfigurationChanged => "NH-PEN-002",
            Self::Credential(error) => error.code(),
            Self::Persistence(error) => error.code(),
            _ => "NH-PEN-003",
        }
    }

    pub(crate) const fn diagnostic(&self) -> Diagnostic {
        match self {
            Self::UnsupportedPlatform
            | Self::Compatibility(_)
            | Self::OlderUnsupported
            | Self::NewerUntested => Diagnostic::general(DiagnosticReason::UnsupportedVersion),
            Self::AppNotFound => Diagnostic::general(DiagnosticReason::MissingExecutable),
            Self::InvalidInstallation => Diagnostic::general(DiagnosticReason::InvalidExecutable),
            Self::AlreadyRunning
            | Self::PendingRecovery
            | Self::OrphanBackup
            | Self::OrphanPersistentBackup
            | Self::ManagedConfigurationChanged(_)
            | Self::PersistentConfigurationChanged
            | Self::PersistentNotConfigured => {
                Diagnostic::general(DiagnosticReason::ConfigurationConflict)
            }
            Self::DidNotStart | Self::Launch(_) => {
                Diagnostic::general(DiagnosticReason::ProcessStartFailed)
            }
            Self::DidNotTerminate | Self::Terminate(_) | Self::TerminateFailed(_) => {
                Diagnostic::general(DiagnosticReason::ProcessTerminationFailed)
            }
            Self::Gateway(_) => Diagnostic::general(DiagnosticReason::BridgeExited),
            Self::Serialize(_) => Diagnostic::general(DiagnosticReason::SerializationFailed),
            Self::ProcessCheck(_) | Self::ProcessCheckFailed(_) => {
                Diagnostic::general(DiagnosticReason::ProcessWaitFailed)
            }
            Self::ModelUnavailable { .. }
            | Self::EmptyModelCatalog
            | Self::ParseDocument { .. }
            | Self::DocumentRootNotObject(_)
            | Self::FieldNotObject { .. }
            | Self::ParseManagedDocument { .. }
            | Self::ManagedRootNotObject(_)
            | Self::ManagedEntryMissing(_)
            | Self::ParseReceipt(_)
            | Self::InvalidReceipt
            | Self::ConfigurationCancelled => {
                Diagnostic::general(DiagnosticReason::InvalidConfiguration)
            }
            Self::MissingHomeDirectory
            | Self::MissingStateDirectory
            | Self::InvalidPath
            | Self::BindGateway(_)
            | Self::State(_)
            | Self::ReadDocument { .. }
            | Self::ReadBackup(_)
            | Self::BackupHashMismatch
            | Self::RemoveBackup(_)
            | Self::Secret(_)
            | Self::ConfirmationRequired
            | Self::Prompt(_)
            | Self::Credential(_)
            | Self::Persistence(_) => {
                Diagnostic::general(DiagnosticReason::FilesystemOperationFailed)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nan_harness_core::{CodingModelProfile, ReasoningPolicy};

    fn model(id: &str, image_input: bool) -> CodingModelProfile {
        let mut profile = CodingModelProfile::generic(id);
        profile.image_input = image_input;
        profile.reasoning = ReasoningPolicy::AlwaysOn;
        profile
    }

    fn paths() -> (tempfile::TempDir, PenPaths) {
        let root = tempfile::tempdir().expect("temp root");
        let paths = PenPaths::new(&root.path().join("home"), &root.path().join("state"))
            .expect("test paths");
        (root, paths)
    }

    #[test]
    fn paths_are_home_relative_and_do_not_embed_a_username() {
        let paths = PenPaths::new(
            Path::new("/home/alice"),
            Path::new("/var/lib/alice/nan-harness/pen-desktop"),
        )
        .expect("paths");
        assert_eq!(
            paths.models,
            PathBuf::from("/home/alice/.pencil/models.json")
        );
        assert_eq!(paths.auth, PathBuf::from("/home/alice/.pencil/agent-auth"));
        assert!(!paths.models.to_string_lossy().contains("david"));
    }

    #[test]
    fn provider_contains_every_text_model_and_pen_metadata() {
        let models = vec![model("qwen3.6", true), model("glm5.3-flash", false)];
        let document = patched_models_document(Map::new(), "http://127.0.0.1:3210/v1", &models)
            .expect("models document");
        let value: Value = serde_json::from_slice(&document).expect("json");
        let provider = &value["providers"][PROVIDER_ID];
        assert_eq!(provider["api"], "openai-completions");
        assert_eq!(provider["models"].as_array().expect("models").len(), 2);
        assert_eq!(provider["models"][0]["input"], json!(["text", "image"]));
    }

    #[test]
    fn session_restore_is_exact_when_pen_does_not_modify_files() {
        let (_root, paths) = paths();
        fs::create_dir_all(paths.models.parent().expect("pencil directory"))
            .expect("pencil directory");
        let original_models = b"{\"providers\":{\"other\":{\"name\":\"Other\"}}}\n";
        let original_auth = b"{\"other\":{\"type\":\"api_key\",\"key\":\"private\"}}\n";
        fs::write(&paths.models, original_models).expect("models");
        fs::write(&paths.auth, original_auth).expect("auth");
        let models = patched_models_document(
            read_json_object(&paths.models).expect("read models"),
            "http://127.0.0.1:1234/v1",
            &[model("qwen3.6", true)],
        )
        .expect("patched models");
        let auth = patched_auth_document(
            read_json_object(&paths.auth).expect("read auth"),
            "session-only",
        )
        .expect("patched auth");
        begin_session(&paths, &models, &auth).expect("begin session");
        let receipt = fs::read_to_string(&paths.session_receipt).expect("receipt");
        assert!(!receipt.contains("session-only"));
        assert!(!receipt.contains("private"));
        assert!(restore_session(&paths).expect("restore"));
        assert_eq!(fs::read(&paths.models).expect("models"), original_models);
        assert_eq!(fs::read(&paths.auth).expect("auth"), original_auth);
    }

    #[test]
    fn restore_preserves_unrelated_changes_made_while_pen_was_open() {
        let (_root, paths) = paths();
        let models = patched_models_document(
            Map::new(),
            "http://127.0.0.1:1234/v1",
            &[model("qwen3.6", true)],
        )
        .expect("models");
        let auth = patched_auth_document(Map::new(), "session-only").expect("auth");
        begin_session(&paths, &models, &auth).expect("begin session");
        let mut changed = read_json_object(&paths.models).expect("read changed");
        changed.insert("unrelated".to_owned(), json!({"preserved": true}));
        fs::write(
            &paths.models,
            serialize_document(&changed).expect("serialize"),
        )
        .expect("write changed");
        restore_session(&paths).expect("restore");
        let restored = read_json_object(&paths.models).expect("restored models");
        assert_eq!(restored["unrelated"]["preserved"], true);
        assert!(restored["providers"].get(PROVIDER_ID).is_none());
    }

    #[test]
    fn persistent_configuration_is_owned_checked_and_removable() {
        let (_root, paths) = paths();
        let models = patched_models_document(
            Map::new(),
            "https://api.nan.build/v1",
            &[model("qwen3.6", true), model("glm5.3-flash", false)],
        )
        .expect("models");
        let auth = patched_auth_document(Map::new(), "copied-provider-secret").expect("auth");
        let models_entry = provider_entry(&models, PenDocumentKind::Models).expect("models entry");
        let auth_entry = provider_entry(&auth, PenDocumentKind::Auth).expect("auth entry");
        create_private_directory(&paths.persistent_backup_directory)
            .expect("persistent backup directory");
        let receipt = PersistentReceipt {
            schema_version: PERSISTENT_SCHEMA_VERSION,
            models_file_existed: false,
            auth_file_existed: false,
            models_backup: backup_persistent_entry(&paths, "models-provider.json", None)
                .expect("models backup"),
            auth_backup: backup_persistent_entry(&paths, "auth-entry.json", None)
                .expect("auth backup"),
            credential_fingerprint: "fingerprint".to_owned(),
            applied_models_sha256: hash_value(&models_entry).expect("models hash"),
            applied_auth_sha256: hash_value(&auth_entry).expect("auth hash"),
            model_ids: vec!["qwen3.6".to_owned(), "glm5.3-flash".to_owned()],
        };
        write_json_private(&paths.persistent_receipt, &receipt).expect("receipt");
        write_private_atomic(&paths.models, &models).expect("models document");
        write_private_atomic(&paths.auth, &auth).expect("auth document");

        assert!(persistent_configuration_active_at(&paths).expect("active"));
        assert_eq!(persistent_model_count_at(&paths).expect("count"), Some(2));
        let receipt_text = fs::read_to_string(&paths.persistent_receipt).expect("receipt text");
        assert!(!receipt_text.contains("copied-provider-secret"));
        restore_persistent_entry(&paths.models, PenDocumentKind::Models, None, false)
            .expect("simulate partially completed removal");
        assert!(remove_persistent_configuration_at(&paths).expect("remove"));
        assert!(!paths.models.exists());
        assert!(!paths.auth.exists());
        assert!(!paths.persistent_receipt.exists());
    }

    #[cfg(unix)]
    #[test]
    fn previous_pen_credentials_live_only_in_owner_private_backups() {
        use std::os::unix::fs::PermissionsExt as _;

        let (_root, paths) = paths();
        create_private_directory(&paths.persistent_backup_directory)
            .expect("persistent backup directory");
        let previous = json!({
            "type": "api_key",
            "key": "previous-provider-secret"
        });
        let backup = backup_persistent_entry(&paths, "auth-entry.json", Some(&previous))
            .expect("credential backup");
        let metadata = serde_json::to_string(&backup).expect("backup metadata");
        assert!(!metadata.contains("previous-provider-secret"));
        let path = paths.persistent_backup_directory.join("auth-entry.json");
        assert!(
            fs::read_to_string(&path)
                .expect("backup contents")
                .contains("previous-provider-secret")
        );
        assert_eq!(
            fs::metadata(path)
                .expect("backup metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    #[test]
    fn explicit_unknown_model_fails_but_default_falls_back_to_first() {
        let models = vec![model("glm5.3-flash", false)];
        assert!(matches!(
            select_model(&models, Some("missing")),
            Err(PenDesktopError::ModelUnavailable { .. })
        ));
        assert_eq!(
            select_model(&models, None).expect("fallback"),
            "glm5.3-flash"
        );
    }
}
