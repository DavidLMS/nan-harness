mod documents;
mod error;
mod paths;
mod persistent;
mod process;
mod session;

pub(crate) use documents::PenDocumentKind;
pub(crate) use error::PenDesktopError;

use crate::app::PenDesktopArgs;
use crate::commands::credentials;
use crate::commands::desktop::DesktopSessionLock;
use crate::commands::persistence::{PersistenceManager, discover_models};
use crate::error::CliError;
use nan_harness_core::{
    CodingModelProfile, DesktopHarnessKind, DesktopLaunchPlan, DesktopTransport, WebSearchPolicy,
};
use nan_harness_runtime::{
    BridgeDiagnostic, DesktopCompatibilityStatus, ExecutionOutcome, ResolvedConfig,
    classify_desktop_version, desktop_compatibility, start_chat_completions_gateway,
};
use semver::Version;
use tokio::net::TcpListener;

use paths::PenPaths;
use process::SystemPenProcess;

#[cfg(test)]
use crate::commands::desktop::{create_private_directory, write_private_atomic};
#[cfg(test)]
use documents::{
    hash_value, patched_auth_document, patched_models_document, provider_entry, read_json_object,
    serialize_document, write_json_private,
};
#[cfg(test)]
use nan_harness_core::ReasoningPolicy;
#[cfg(test)]
use paths::{PERSISTENT_SCHEMA_VERSION, PersistentReceipt};
#[cfg(test)]
use persistent::{
    backup_persistent_entry, persistent_configuration_active_at, persistent_model_count_at,
    remove_persistent_configuration_at, restore_persistent_entry,
};
#[cfg(test)]
use serde_json::{Map, Value, json};
#[cfg(test)]
use session::begin_session;
use session::{ensure_no_pending_session, restore_session};
#[cfg(test)]
use std::fs;
#[cfg(test)]
use std::path::{Path, PathBuf};

const DEFAULT_MODEL_ID: &str = "qwen3.6";

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
    let result = session::run_managed_session(&paths, &process, &gateway, &models).await;
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

fn extract_semver(output: &str) -> Option<Version> {
    output.split_whitespace().find_map(|candidate| {
        let candidate = candidate.trim_matches(|character: char| {
            !character.is_ascii_digit() && character != '.' && character != '-' && character != '+'
        });
        Version::parse(candidate).ok()
    })
}

pub(crate) fn persistent_configuration_exists() -> Result<bool, PenDesktopError> {
    persistent::persistent_configuration_exists()
}

pub(crate) fn persistent_configuration_active() -> Result<bool, PenDesktopError> {
    persistent::persistent_configuration_active()
}

pub(crate) fn persistent_credential_is_current(
    saved_fingerprint: Option<&str>,
) -> Result<Option<bool>, PenDesktopError> {
    persistent::persistent_credential_is_current(saved_fingerprint)
}

pub(crate) async fn configure_persistent(
    refresh: bool,
    confirmed: bool,
    interactive: bool,
) -> Result<usize, PenDesktopError> {
    persistent::configure_persistent(refresh, confirmed, interactive).await
}

pub(crate) fn refresh_persistent_with_config(
    config: &ResolvedConfig,
    models: &[CodingModelProfile],
) -> Result<bool, PenDesktopError> {
    persistent::refresh_persistent_with_config(config, models)
}

pub(crate) fn remove_persistent_configuration() -> Result<bool, PenDesktopError> {
    persistent::remove_persistent_configuration()
}

pub(crate) fn persistent_model_count() -> Result<Option<usize>, PenDesktopError> {
    persistent::persistent_model_count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nan_harness_core::CodingModelProfile;

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
        let provider = &value["providers"]["nan"];
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
        assert!(restored["providers"].get("nan").is_none());
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
