use super::*;

pub(super) fn begin_session(
    paths: &DesktopPaths,
    profile: &Path,
    mode: SessionMode,
    session_key: &str,
) -> Result<(), HermesDesktopError> {
    if paths.session_receipt.exists() {
        return Err(HermesDesktopError::PendingRecovery);
    }
    let environment_path = profile.join(".env");
    let active_original = read_optional(&paths.active_profile)?;
    let environment_original = read_optional(&environment_path)?;
    let environment_applied = add_env_block(environment_original.as_deref(), session_key)?;
    let profile_name = profile
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(HermesDesktopError::InvalidProfilePath)?;
    let active_applied = serde_json::to_vec_pretty(&json!({"profile": profile_name}))
        .map_err(HermesDesktopError::Serialize)?;

    fs::create_dir_all(&paths.backup_directory)
        .map_err(HermesDesktopError::CreateBackupDirectory)?;
    restrict_path(&paths.backup_directory, PrivatePathKind::Directory)
        .map_err(HermesDesktopError::ProtectBackupDirectory)?;
    let active_backup = backup_file(
        &paths.backup_directory,
        "active-profile.backup",
        active_original.as_deref(),
    )?;
    let environment_backup = backup_file(
        &paths.backup_directory,
        "profile-env.backup",
        environment_original.as_deref(),
    )?;
    let receipt = SessionReceipt {
        schema_version: SESSION_SCHEMA_VERSION,
        mode,
        profile: profile.to_path_buf(),
        active_profile: active_backup,
        environment: environment_backup,
        active_applied_sha256: sha256(&active_applied),
        environment_applied_sha256: sha256(&environment_applied),
    };
    write_json_private(&paths.session_receipt, &receipt)?;
    if let Err(error) = write_private(&environment_path, &environment_applied)
        .and_then(|()| write_private(&paths.active_profile, &active_applied))
    {
        let _ = restore_session(paths);
        return Err(error);
    }
    Ok(())
}

pub(super) fn add_env_block(
    original: Option<&[u8]>,
    session_key: &str,
) -> Result<Vec<u8>, HermesDesktopError> {
    let original = original.unwrap_or_default();
    let text = std::str::from_utf8(original).map_err(HermesDesktopError::ProfileEnvUtf8)?;
    if text.contains(ENV_BLOCK_BEGIN)
        || text.contains(ENV_BLOCK_END)
        || text.lines().any(defines_nan_api_key)
    {
        return Err(HermesDesktopError::ProfileCredentialConflict);
    }
    let mut output = text.trim_end_matches(['\r', '\n']).to_owned();
    if !output.is_empty() {
        output.push('\n');
    }
    output.push_str(ENV_BLOCK_BEGIN);
    output.push('\n');
    output.push_str("NAN_API_KEY=");
    output.push_str(&dotenv_quote(session_key));
    output.push('\n');
    output.push_str(ENV_BLOCK_END);
    output.push('\n');
    Ok(output.into_bytes())
}

pub(super) fn dotenv_quote(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r");
    format!("\"{escaped}\"")
}

pub(super) fn defines_nan_api_key(line: &str) -> bool {
    let line = line.trim_start();
    if line.starts_with('#') {
        return false;
    }
    line.strip_prefix("export ")
        .unwrap_or(line)
        .starts_with("NAN_API_KEY=")
}

pub(super) fn restore_session(paths: &DesktopPaths) -> Result<(), HermesDesktopError> {
    let Some(receipt) = read_optional_json::<SessionReceipt>(&paths.session_receipt)? else {
        return Ok(());
    };
    if receipt.schema_version != SESSION_SCHEMA_VERSION {
        return Err(HermesDesktopError::UnsupportedSessionSchema);
    }
    validate_session_receipt(paths, &receipt)?;
    restore_active_profile(paths, &receipt)?;
    restore_environment(paths, &receipt)?;
    if receipt.mode == SessionMode::Diagnostic {
        remove_owned_diagnostic_profile(&receipt.profile)?;
    }
    remove_if_exists(&paths.backup_directory.join("active-profile.backup"))
        .map_err(HermesDesktopError::RemoveBackup)?;
    remove_if_exists(&paths.backup_directory.join("profile-env.backup"))
        .map_err(HermesDesktopError::RemoveBackup)?;
    match fs::remove_dir(&paths.backup_directory) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(HermesDesktopError::RemoveBackup(error)),
    }
    remove_if_exists(&paths.session_receipt).map_err(HermesDesktopError::RemoveReceipt)?;
    Ok(())
}

pub(super) fn validate_session_receipt(
    paths: &DesktopPaths,
    receipt: &SessionReceipt,
) -> Result<(), HermesDesktopError> {
    let valid = match receipt.mode {
        SessionMode::Persistent => receipt.profile == paths.managed_profile,
        SessionMode::Diagnostic => {
            receipt.profile.parent() == Some(paths.profiles_root.as_path())
                && receipt.profile.file_name().is_some_and(|name| {
                    name.to_string_lossy()
                        .starts_with(DIAGNOSTIC_PROFILE_PREFIX)
                })
        }
    };
    if !valid
        || receipt.active_profile.backup_file != "active-profile.backup"
        || receipt.environment.backup_file != "profile-env.backup"
    {
        return Err(HermesDesktopError::InvalidRecoveryReceipt);
    }
    Ok(())
}

pub(super) fn restore_active_profile(
    paths: &DesktopPaths,
    receipt: &SessionReceipt,
) -> Result<(), HermesDesktopError> {
    let current = read_optional(&paths.active_profile)?;
    if file_is_original(current.as_deref(), &receipt.active_profile) {
        return Ok(());
    }
    if current
        .as_deref()
        .is_some_and(|value| sha256(value) == receipt.active_applied_sha256)
    {
        restore_backup(paths, &paths.active_profile, &receipt.active_profile)?;
    } else {
        eprintln!(
            "warning: Hermes Desktop's active profile changed during the NaN session; preserving the user's selection."
        );
    }
    Ok(())
}

pub(super) fn restore_environment(
    paths: &DesktopPaths,
    receipt: &SessionReceipt,
) -> Result<(), HermesDesktopError> {
    let path = receipt.profile.join(".env");
    let current = read_optional(&path)?;
    if file_is_original(current.as_deref(), &receipt.environment) {
        return Ok(());
    }
    if current
        .as_deref()
        .is_some_and(|value| sha256(value) == receipt.environment_applied_sha256)
    {
        return restore_backup(paths, &path, &receipt.environment);
    }
    let Some(current) = current else {
        return Ok(());
    };
    let cleaned = remove_env_block(&current)?;
    write_private(&path, &cleaned)
}

pub(super) fn remove_env_block(contents: &[u8]) -> Result<Vec<u8>, HermesDesktopError> {
    let text = std::str::from_utf8(contents).map_err(HermesDesktopError::ProfileEnvUtf8)?;
    let Some(begin) = text.find(ENV_BLOCK_BEGIN) else {
        return if text.lines().any(defines_nan_api_key) {
            Err(HermesDesktopError::ManagedCredentialChanged)
        } else {
            Ok(contents.to_vec())
        };
    };
    let end_start = text[begin..]
        .find(ENV_BLOCK_END)
        .map(|offset| begin + offset)
        .ok_or(HermesDesktopError::ManagedCredentialChanged)?;
    if text[end_start + ENV_BLOCK_END.len()..].contains(ENV_BLOCK_END)
        || text[..begin].contains(ENV_BLOCK_BEGIN)
    {
        return Err(HermesDesktopError::ManagedCredentialChanged);
    }
    let mut end = end_start + ENV_BLOCK_END.len();
    if text.as_bytes().get(end) == Some(&b'\r') {
        end += 1;
    }
    if text.as_bytes().get(end) == Some(&b'\n') {
        end += 1;
    }
    let mut output = String::with_capacity(text.len());
    output.push_str(&text[..begin]);
    output.push_str(&text[end..]);
    Ok(output.into_bytes())
}

pub(super) fn remove_owned_diagnostic_profile(profile: &Path) -> Result<(), HermesDesktopError> {
    if !profile.exists() {
        return Ok(());
    }
    let marker = read_optional_json::<OwnerMarker>(&profile.join(OWNER_MARKER_FILE))?;
    if !marker.as_ref().is_some_and(|marker| {
        marker.schema_version == OWNERSHIP_SCHEMA_VERSION && marker.owner_id == "diagnostic"
    }) {
        return Err(HermesDesktopError::DiagnosticOwnershipMismatch);
    }
    fs::remove_dir_all(profile).map_err(HermesDesktopError::RemoveProfile)
}

pub(super) fn ensure_recovery_is_safe(paths: &DesktopPaths) -> Result<(), HermesDesktopError> {
    if running_desktop()?.is_some() {
        return Err(HermesDesktopError::AlreadyRunning);
    }
    if live_update_owner(&paths.update_marker)?.is_some() {
        return Err(HermesDesktopError::UpdateStillRunning);
    }
    Ok(())
}

pub(super) fn backup_file(
    directory: &Path,
    name: &str,
    contents: Option<&[u8]>,
) -> Result<FileBackup, HermesDesktopError> {
    let path = directory.join(name);
    match contents {
        Some(contents) => write_private(&path, contents)?,
        None => remove_if_exists(&path).map_err(HermesDesktopError::RemoveBackup)?,
    }
    Ok(FileBackup {
        existed: contents.is_some(),
        original_sha256: contents.map(sha256),
        backup_file: name.to_owned(),
    })
}

pub(super) fn restore_backup(
    paths: &DesktopPaths,
    target: &Path,
    backup: &FileBackup,
) -> Result<(), HermesDesktopError> {
    if backup.existed {
        let backup_path = paths.backup_directory.join(&backup.backup_file);
        let contents = fs::read(&backup_path).map_err(HermesDesktopError::ReadBackup)?;
        if Some(sha256(&contents)) != backup.original_sha256 {
            return Err(HermesDesktopError::BackupHashMismatch);
        }
        write_private(target, &contents)
    } else {
        remove_if_exists(target).map_err(HermesDesktopError::Restore)
    }
}

pub(super) fn file_is_original(current: Option<&[u8]>, backup: &FileBackup) -> bool {
    match (current, backup.existed, backup.original_sha256.as_deref()) {
        (None, false, _) => true,
        (Some(current), true, Some(hash)) => sha256(current) == hash,
        _ => false,
    }
}

pub(super) fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, HermesDesktopError> {
    reject_profile_symlink(path)?;
    match fs::read(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(HermesDesktopError::ReadFile(error)),
    }
}

pub(super) fn read_optional_json<T: for<'de> Deserialize<'de>>(
    path: &Path,
) -> Result<Option<T>, HermesDesktopError> {
    let Some(contents) = read_optional(path)? else {
        return Ok(None);
    };
    serde_json::from_slice(&contents)
        .map(Some)
        .map_err(HermesDesktopError::ParseReceipt)
}

pub(super) fn write_json_private(
    path: &Path,
    value: &impl Serialize,
) -> Result<(), HermesDesktopError> {
    let payload = serde_json::to_vec_pretty(value).map_err(HermesDesktopError::Serialize)?;
    write_private(path, &payload)
}

pub(super) fn write_private(path: &Path, payload: &[u8]) -> Result<(), HermesDesktopError> {
    reject_profile_symlink(path)?;
    write_private_file(path, payload, None).map_err(HermesDesktopError::Persistence)
}

pub(super) fn remove_if_exists(path: &Path) -> Result<(), std::io::Error> {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(std::io::Error::other(
            "managed Desktop state contains an unsafe symbolic link",
        ));
    }
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

pub(super) fn sha256(contents: &[u8]) -> String {
    let digest = Sha256::digest(contents);
    digest
        .iter()
        .fold(String::with_capacity(64), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

pub(super) fn random_id() -> Result<String, HermesDesktopError> {
    let mut bytes = [0_u8; 12];
    getrandom::fill(&mut bytes).map_err(HermesDesktopError::Random)?;
    Ok(bytes
        .iter()
        .fold(String::with_capacity(24), |mut output, byte| {
            let _ = write!(output, "{byte:02x}");
            output
        }))
}
