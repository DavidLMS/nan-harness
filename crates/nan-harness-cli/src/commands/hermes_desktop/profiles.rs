#[allow(clippy::wildcard_imports)]
use super::*;

mod profile_configuration;
pub(super) use profile_configuration::*;

pub(super) fn ensure_managed_profile(
    paths: &DesktopPaths,
) -> Result<OwnershipReceipt, HermesDesktopError> {
    if let Some((ownership, location)) = locate_managed_profile(paths)? {
        if location == ManagedProfileLocation::Parked {
            ensure_profile_guard(paths, &ownership)?;
        }
        return Ok(ownership);
    }
    create_managed_profile(paths)
}

pub(super) fn locate_managed_profile(
    paths: &DesktopPaths,
) -> Result<Option<(OwnershipReceipt, ManagedProfileLocation)>, HermesDesktopError> {
    let active = profile_path_kind(&paths.managed_profile)?;
    let parked = profile_path_kind(&paths.parked_profile)?;
    validate_profile_shapes(active, parked)?;
    let ownership = read_optional_json::<OwnershipReceipt>(&paths.ownership_receipt)?;
    let selected = select_managed_profile(paths, active, parked);
    let Some((profile, location)) = selected else {
        return missing_managed_profile(active, ownership.as_ref());
    };
    let marker = read_profile_owner_marker(profile, location)?;
    let ownership = resolve_profile_ownership(paths, ownership, marker, location)?;
    validate_active_profile_guard(paths, active, &ownership)?;
    Ok(Some((ownership, location)))
}

pub(super) fn validate_profile_shapes(
    active: ProfilePathKind,
    parked: ProfilePathKind,
) -> Result<(), HermesDesktopError> {
    if parked != ProfilePathKind::Missing && parked != ProfilePathKind::Directory {
        return Err(HermesDesktopError::ParkedProfileOwnershipMismatch);
    }
    if active == ProfilePathKind::Directory && parked == ProfilePathKind::Directory {
        return Err(HermesDesktopError::ManagedProfileConflict);
    }
    if active == ProfilePathKind::Other {
        return Err(HermesDesktopError::UnmanagedNanProfile);
    }
    Ok(())
}

pub(super) fn select_managed_profile(
    paths: &DesktopPaths,
    active: ProfilePathKind,
    parked: ProfilePathKind,
) -> Option<(&Path, ManagedProfileLocation)> {
    match (active, parked) {
        (ProfilePathKind::Directory, ProfilePathKind::Missing) => {
            Some((&paths.managed_profile, ManagedProfileLocation::Active))
        }
        (ProfilePathKind::Missing | ProfilePathKind::RegularFile, ProfilePathKind::Directory) => {
            Some((&paths.parked_profile, ManagedProfileLocation::Parked))
        }
        _ => None,
    }
}

pub(super) fn missing_managed_profile(
    active: ProfilePathKind,
    ownership: Option<&OwnershipReceipt>,
) -> Result<Option<(OwnershipReceipt, ManagedProfileLocation)>, HermesDesktopError> {
    if ownership.is_some() {
        Err(HermesDesktopError::ManagedProfileMissing)
    } else if active == ProfilePathKind::RegularFile {
        Err(HermesDesktopError::UnmanagedNanProfile)
    } else {
        Ok(None)
    }
}

pub(super) fn read_profile_owner_marker(
    profile: &Path,
    location: ManagedProfileLocation,
) -> Result<OwnerMarker, HermesDesktopError> {
    let marker = read_optional_json::<OwnerMarker>(&profile.join(OWNER_MARKER_FILE))?;
    marker.ok_or_else(|| match location {
        ManagedProfileLocation::Active => HermesDesktopError::UnmanagedNanProfile,
        ManagedProfileLocation::Parked => HermesDesktopError::ParkedProfileOwnershipMismatch,
    })
}

pub(super) fn resolve_profile_ownership(
    paths: &DesktopPaths,
    ownership: Option<OwnershipReceipt>,
    marker: OwnerMarker,
    location: ManagedProfileLocation,
) -> Result<OwnershipReceipt, HermesDesktopError> {
    match ownership {
        Some(ownership) => {
            validate_ownership(&ownership, &marker)?;
            Ok(ownership)
        }
        None => recover_ownership(paths, marker, location),
    }
}

pub(super) fn validate_active_profile_guard(
    paths: &DesktopPaths,
    active: ProfilePathKind,
    ownership: &OwnershipReceipt,
) -> Result<(), HermesDesktopError> {
    if active != ProfilePathKind::RegularFile {
        return Ok(());
    }
    let guard =
        read_profile_guard(paths)?.ok_or(HermesDesktopError::ProfileGuardOwnershipMismatch)?;
    if guard.schema_version != OWNERSHIP_SCHEMA_VERSION || guard.owner_id != ownership.owner_id {
        return Err(HermesDesktopError::ProfileGuardOwnershipMismatch);
    }
    Ok(())
}

pub(super) fn profile_path_kind(path: &Path) -> Result<ProfilePathKind, HermesDesktopError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => Ok(ProfilePathKind::Directory),
        Ok(metadata) if metadata.file_type().is_file() => Ok(ProfilePathKind::RegularFile),
        Ok(_) => Ok(ProfilePathKind::Other),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(ProfilePathKind::Missing),
        Err(error) => Err(HermesDesktopError::ReadFile(error)),
    }
}

pub(super) fn read_profile_guard(
    paths: &DesktopPaths,
) -> Result<Option<OwnerMarker>, HermesDesktopError> {
    let Some(contents) = read_optional(&paths.managed_profile)? else {
        return Ok(None);
    };
    serde_json::from_slice(&contents)
        .map(Some)
        .map_err(|_| HermesDesktopError::ProfileGuardOwnershipMismatch)
}

pub(super) fn activate_managed_profile(
    paths: &DesktopPaths,
    expected: &OwnershipReceipt,
) -> Result<(), HermesDesktopError> {
    let Some((ownership, location)) = locate_managed_profile(paths)? else {
        return Err(HermesDesktopError::ManagedProfileMissing);
    };
    if &ownership != expected {
        return Err(HermesDesktopError::OwnershipMismatch);
    }
    if location == ManagedProfileLocation::Active {
        return Ok(());
    }
    fs::create_dir_all(&paths.profiles_root).map_err(HermesDesktopError::CreateProfile)?;
    remove_profile_guard(paths)?;
    if let Err(error) = fs::rename(&paths.parked_profile, &paths.managed_profile) {
        let _ = ensure_profile_guard(paths, &ownership);
        return Err(HermesDesktopError::ActivateProfile(error));
    }
    Ok(())
}

pub(super) fn park_managed_profile_if_owned(
    paths: &DesktopPaths,
) -> Result<(), HermesDesktopError> {
    match locate_managed_profile(paths)? {
        Some((_, ManagedProfileLocation::Active)) => park_managed_profile(paths),
        Some((ownership, ManagedProfileLocation::Parked)) => {
            ensure_profile_guard(paths, &ownership)
        }
        None => Ok(()),
    }
}

pub(super) fn park_managed_profile(paths: &DesktopPaths) -> Result<(), HermesDesktopError> {
    let Some((ownership, location)) = locate_managed_profile(paths)? else {
        return Err(HermesDesktopError::ManagedProfileMissing);
    };
    if location == ManagedProfileLocation::Parked {
        return ensure_profile_guard(paths, &ownership);
    }
    reset_managed_active_profile(paths)?;
    fs::create_dir_all(&paths.parked_profiles_root)
        .map_err(HermesDesktopError::CreateParkingDirectory)?;
    restrict_path(&paths.parked_profiles_root, PrivatePathKind::Directory)
        .map_err(HermesDesktopError::ProtectParkingDirectory)?;
    fs::rename(&paths.managed_profile, &paths.parked_profile)
        .map_err(HermesDesktopError::ParkProfile)?;
    if let Err(error) = ensure_profile_guard(paths, &ownership) {
        let _ = fs::rename(&paths.parked_profile, &paths.managed_profile);
        return Err(error);
    }
    Ok(())
}

pub(super) fn ensure_profile_guard(
    paths: &DesktopPaths,
    ownership: &OwnershipReceipt,
) -> Result<(), HermesDesktopError> {
    match profile_path_kind(&paths.managed_profile)? {
        ProfilePathKind::RegularFile => {
            let guard = read_profile_guard(paths)?
                .ok_or(HermesDesktopError::ProfileGuardOwnershipMismatch)?;
            if guard.schema_version == OWNERSHIP_SCHEMA_VERSION
                && guard.owner_id == ownership.owner_id
            {
                return Ok(());
            }
            return Err(HermesDesktopError::ProfileGuardOwnershipMismatch);
        }
        ProfilePathKind::Missing => {}
        ProfilePathKind::Directory => return Err(HermesDesktopError::ManagedProfileConflict),
        ProfilePathKind::Other => {
            return Err(HermesDesktopError::ProfileGuardOwnershipMismatch);
        }
    }
    let marker = OwnerMarker {
        schema_version: OWNERSHIP_SCHEMA_VERSION,
        owner_id: ownership.owner_id.clone(),
    };
    let payload = serde_json::to_vec_pretty(&marker).map_err(HermesDesktopError::Serialize)?;
    let mut file =
        open_private_new(&paths.managed_profile).map_err(HermesDesktopError::CreateProfileGuard)?;
    if let Err(error) = std::io::Write::write_all(&mut file, &payload) {
        drop(file);
        let _ = fs::remove_file(&paths.managed_profile);
        return Err(HermesDesktopError::WriteProfileGuard(error));
    }
    Ok(())
}

pub(super) fn remove_profile_guard(paths: &DesktopPaths) -> Result<(), HermesDesktopError> {
    match profile_path_kind(&paths.managed_profile)? {
        ProfilePathKind::Missing => Ok(()),
        ProfilePathKind::RegularFile => {
            fs::remove_file(&paths.managed_profile).map_err(HermesDesktopError::RemoveProfileGuard)
        }
        ProfilePathKind::Directory => Err(HermesDesktopError::ManagedProfileConflict),
        ProfilePathKind::Other => Err(HermesDesktopError::ProfileGuardOwnershipMismatch),
    }
}

pub(super) fn quarantine_recreated_profile_for_restore(
    paths: &DesktopPaths,
) -> Result<(), HermesDesktopError> {
    if profile_path_kind(&paths.managed_profile)? != ProfilePathKind::Directory
        || profile_path_kind(&paths.parked_profile)? != ProfilePathKind::Directory
        || paths.managed_profile.join("config.yaml").exists()
        || read_optional_json::<OwnerMarker>(&paths.managed_profile.join(OWNER_MARKER_FILE))?
            .is_some()
    {
        return Ok(());
    }
    let Some(ownership) = read_optional_json::<OwnershipReceipt>(&paths.ownership_receipt)? else {
        return Ok(());
    };
    let Some(marker) =
        read_optional_json::<OwnerMarker>(&paths.parked_profile.join(OWNER_MARKER_FILE))?
    else {
        return Ok(());
    };
    validate_ownership(&ownership, &marker)?;
    fs::create_dir_all(&paths.recovered_profiles_root)
        .map_err(HermesDesktopError::CreateRecoveryDirectory)?;
    restrict_path(&paths.recovered_profiles_root, PrivatePathKind::Directory)
        .map_err(HermesDesktopError::ProtectRecoveryDirectory)?;
    let recovered = paths
        .recovered_profiles_root
        .join(format!("{PROFILE_NAME}-{}", random_id()?));
    fs::rename(&paths.managed_profile, &recovered)
        .map_err(HermesDesktopError::QuarantineRecreatedProfile)?;
    if let Err(error) = ensure_profile_guard(paths, &ownership) {
        let _ = fs::rename(&recovered, &paths.managed_profile);
        return Err(error);
    }
    eprintln!(
        "warning: Hermes Desktop recreated an empty 'nan' profile from cached UI state; NaN preserved it in the private recovery area and restored the visibility guard."
    );
    Ok(())
}

pub(super) fn reset_managed_active_profile(paths: &DesktopPaths) -> Result<(), HermesDesktopError> {
    let Some(contents) = read_optional(&paths.active_profile)? else {
        return Ok(());
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&contents) else {
        return Ok(());
    };
    if value.get("profile").and_then(serde_json::Value::as_str) == Some(PROFILE_NAME) {
        let default = serde_json::to_vec_pretty(&json!({"profile": "default"}))
            .map_err(HermesDesktopError::Serialize)?;
        write_private(&paths.active_profile, &default)?;
    }
    Ok(())
}

pub(super) fn recover_ownership(
    paths: &DesktopPaths,
    marker: OwnerMarker,
    location: ManagedProfileLocation,
) -> Result<OwnershipReceipt, HermesDesktopError> {
    if marker.schema_version != OWNERSHIP_SCHEMA_VERSION || marker.owner_id == "diagnostic" {
        return Err(match location {
            ManagedProfileLocation::Active => HermesDesktopError::UnmanagedNanProfile,
            ManagedProfileLocation::Parked => HermesDesktopError::ParkedProfileOwnershipMismatch,
        });
    }
    let ownership = OwnershipReceipt {
        schema_version: OWNERSHIP_SCHEMA_VERSION,
        owner_id: marker.owner_id,
        profile_name: PROFILE_NAME.to_owned(),
        gateway_port: None,
    };
    write_json_private(&paths.ownership_receipt, &ownership)?;
    Ok(ownership)
}

pub(super) fn create_managed_profile(
    paths: &DesktopPaths,
) -> Result<OwnershipReceipt, HermesDesktopError> {
    fs::create_dir_all(&paths.parked_profiles_root)
        .map_err(HermesDesktopError::CreateParkingDirectory)?;
    restrict_path(&paths.parked_profiles_root, PrivatePathKind::Directory)
        .map_err(HermesDesktopError::ProtectParkingDirectory)?;
    fs::create_dir(&paths.parked_profile).map_err(HermesDesktopError::CreateProfile)?;
    restrict_path(&paths.parked_profile, PrivatePathKind::Directory)
        .map_err(HermesDesktopError::ProtectProfile)?;
    let owner_id = random_id()?;
    let marker = OwnerMarker {
        schema_version: OWNERSHIP_SCHEMA_VERSION,
        owner_id: owner_id.clone(),
    };
    let ownership = OwnershipReceipt {
        schema_version: OWNERSHIP_SCHEMA_VERSION,
        owner_id,
        profile_name: PROFILE_NAME.to_owned(),
        gateway_port: None,
    };
    let result = (|| {
        write_json_private(&paths.parked_profile.join(OWNER_MARKER_FILE), &marker)?;
        write_json_private(&paths.ownership_receipt, &ownership)?;
        ensure_profile_guard(paths, &ownership)?;
        Ok::<(), HermesDesktopError>(())
    })();
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&paths.parked_profile);
        if read_profile_guard(paths)
            .ok()
            .flatten()
            .is_some_and(|guard| guard.owner_id == ownership.owner_id)
        {
            let _ = fs::remove_file(&paths.managed_profile);
        }
        if read_optional_json::<OwnershipReceipt>(&paths.ownership_receipt)
            .ok()
            .flatten()
            .is_some_and(|receipt| receipt.owner_id == ownership.owner_id)
        {
            let _ = fs::remove_file(&paths.ownership_receipt);
        }
        return Err(error);
    }
    Ok(ownership)
}

pub(super) fn validate_ownership(
    ownership: &OwnershipReceipt,
    marker: &OwnerMarker,
) -> Result<(), HermesDesktopError> {
    if ownership.schema_version != OWNERSHIP_SCHEMA_VERSION
        || marker.schema_version != OWNERSHIP_SCHEMA_VERSION
    {
        return Err(HermesDesktopError::UnsupportedOwnershipSchema);
    }
    if ownership.profile_name != PROFILE_NAME || ownership.owner_id != marker.owner_id {
        return Err(HermesDesktopError::OwnershipMismatch);
    }
    Ok(())
}

pub(super) fn remove_legacy_profile_display_name(profile: &Path) -> Result<(), HermesDesktopError> {
    let path = profile.join("profile.yaml");
    let contents = match fs::read(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(HermesDesktopError::ReadFile(error)),
    };
    if matches!(
        contents.as_slice(),
        b"display_name: NaN" | b"display_name: NaN\n"
    ) {
        remove_if_exists(&path).map_err(HermesDesktopError::RemoveProfileMetadata)?;
    }
    Ok(())
}

pub(super) fn create_diagnostic_profile(
    paths: &DesktopPaths,
) -> Result<PathBuf, HermesDesktopError> {
    fs::create_dir_all(&paths.profiles_root).map_err(HermesDesktopError::CreateProfile)?;
    let name = format!("{DIAGNOSTIC_PROFILE_PREFIX}{}", random_id()?.to_lowercase());
    let profile = paths.profiles_root.join(name);
    fs::create_dir(&profile).map_err(HermesDesktopError::CreateProfile)?;
    restrict_path(&profile, PrivatePathKind::Directory)
        .map_err(HermesDesktopError::ProtectProfile)?;
    write_json_private(
        &profile.join(OWNER_MARKER_FILE),
        &OwnerMarker {
            schema_version: OWNERSHIP_SCHEMA_VERSION,
            owner_id: "diagnostic".to_owned(),
        },
    )?;
    Ok(profile)
}

pub(super) fn cleanup_stale_diagnostic_profiles(
    paths: &DesktopPaths,
) -> Result<(), HermesDesktopError> {
    let entries = match fs::read_dir(&paths.profiles_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(HermesDesktopError::ReadProfiles(error)),
    };
    for entry in entries {
        let entry = entry.map_err(HermesDesktopError::ReadProfiles)?;
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with(DIAGNOSTIC_PROFILE_PREFIX)
        {
            continue;
        }
        let marker = read_optional_json::<OwnerMarker>(&entry.path().join(OWNER_MARKER_FILE))?;
        if marker.as_ref().is_some_and(|marker| {
            marker.schema_version == OWNERSHIP_SCHEMA_VERSION && marker.owner_id == "diagnostic"
        }) {
            fs::remove_dir_all(entry.path()).map_err(HermesDesktopError::RemoveProfile)?;
        }
    }
    Ok(())
}
