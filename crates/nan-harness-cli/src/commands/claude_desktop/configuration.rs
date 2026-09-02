#[allow(clippy::wildcard_imports)]
use super::*;

pub(super) fn apply_gateway(
    paths: &DesktopPaths,
    base_url: &str,
    token: &str,
) -> Result<(), ClaudeDesktopError> {
    let mut documents = paths
        .documents()
        .into_iter()
        .map(read_json_object)
        .collect::<Result<Vec<_>, _>>()?;
    documents[0].insert("deploymentMode".to_owned(), json!("3p"));
    documents[1].insert("deploymentMode".to_owned(), json!("3p"));

    documents[2].insert("appliedId".to_owned(), json!(PROFILE_ID));
    let entries = documents[2]
        .remove("entries")
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    let mut entries = entries
        .into_iter()
        .filter(|entry| entry.get("id").and_then(Value::as_str) != Some(PROFILE_ID))
        .collect::<Vec<_>>();
    entries.push(json!({"id": PROFILE_ID, "name": PROFILE_NAME}));
    documents[2].insert("entries".to_owned(), Value::Array(entries));

    let profile = &mut documents[3];
    profile.insert("inferenceProvider".to_owned(), json!("gateway"));
    profile.insert("inferenceGatewayBaseUrl".to_owned(), json!(base_url));
    profile.insert("inferenceGatewayApiKey".to_owned(), json!(token));
    profile.insert("inferenceGatewayAuthScheme".to_owned(), json!("bearer"));
    profile.insert("deploymentDisplayName".to_owned(), json!(PROFILE_NAME));
    profile.insert("modelDiscoveryEnabled".to_owned(), json!(true));
    profile.insert("chatTabEnabled".to_owned(), json!(true));
    profile.insert("autoModeEnabled".to_owned(), json!(true));
    profile.insert("disableDeploymentModeChooser".to_owned(), json!(true));
    profile.insert("coworkEgressAllowedHosts".to_owned(), json!(["*"]));
    profile.remove("inferenceModels");

    for (document, path) in documents.into_iter().zip(paths.documents()) {
        let mut payload =
            serde_json::to_vec_pretty(&document).map_err(ClaudeDesktopError::SerializeConfig)?;
        payload.push(b'\n');
        let permissions = existing_permissions(path)?;
        atomic_write(path, &payload, permissions.as_ref(), false)?;
    }
    Ok(())
}

pub(super) fn read_json_object(path: &Path) -> Result<Map<String, Value>, ClaudeDesktopError> {
    reject_symlink(path)?;
    match fs::read(path) {
        Ok(contents) => serde_json::from_slice::<Value>(&contents)
            .map_err(ClaudeDesktopError::ParseConfig)?
            .as_object()
            .cloned()
            .ok_or(ClaudeDesktopError::ConfigRoot),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(Map::new()),
        Err(error) => Err(ClaudeDesktopError::ReadConfig(error)),
    }
}

pub(super) fn existing_permissions(path: &Path) -> Result<Option<Permissions>, ClaudeDesktopError> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(Some(metadata.permissions())),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ClaudeDesktopError::ReadConfig(error)),
    }
}
