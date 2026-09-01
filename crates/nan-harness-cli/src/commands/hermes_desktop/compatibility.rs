use super::*;

pub(super) fn validate_arguments(arguments: &HermesDesktopArgs) -> Result<(), HermesDesktopError> {
    if arguments.restore
        && (arguments.run.model.is_some()
            || arguments.run.executable.is_some()
            || arguments.run.provider_base_url.is_some()
            || arguments.run.allow_unsupported
            || arguments.run.allow_untested
            || arguments.run.dry_run
            || arguments.no_chat_gateway
            || !arguments.run.arguments.is_empty())
    {
        return Err(HermesDesktopError::RestoreWithLaunchOptions);
    }
    if let Some(unsupported) = unsupported_desktop_argument(&arguments.run.arguments) {
        return Err(HermesDesktopError::UnsupportedDesktopArgument(unsupported));
    }
    Ok(())
}

pub(super) fn unsupported_desktop_argument(arguments: &[String]) -> Option<&'static str> {
    ["--build-only", "--setup-tcc-identity"]
        .into_iter()
        .find(|unsupported| arguments.iter().any(|argument| argument == unsupported))
}

pub(super) fn validate_desktop_compatibility(
    executable: &str,
    detected_version: &str,
    allow_unsupported: bool,
    allow_untested: bool,
) -> Result<(), HermesDesktopError> {
    validate_desktop_version(detected_version, allow_unsupported, allow_untested)?;
    let output = Command::new(executable)
        .args(["desktop", "--help"])
        .output()
        .map_err(HermesDesktopError::CapabilityProbe)?;
    if !output.status.success() {
        return Err(HermesDesktopError::CapabilityProbeFailed(
            output.status.code(),
        ));
    }
    let help = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let missing = missing_desktop_capabilities(&help);
    if !missing.is_empty() {
        return Err(HermesDesktopError::MissingDesktopCapabilities(
            missing.join(", "),
        ));
    }
    Ok(())
}

pub(super) fn validate_desktop_version(
    detected_version: &str,
    allow_unsupported: bool,
    allow_untested: bool,
) -> Result<(), HermesDesktopError> {
    let entry = desktop_compatibility(DesktopHarnessKind::Hermes)?;
    let version = extract_semver(detected_version);
    match classify_desktop_version(&entry, version.as_ref()) {
        DesktopCompatibilityStatus::ContractOnly => eprintln!(
            "warning: Hermes Desktop compatibility on this platform is based on deterministic contracts, not a live verification"
        ),
        DesktopCompatibilityStatus::OlderUnsupported if !allow_unsupported => {
            let (Some(detected), Some(minimum)) =
                (version.as_ref(), entry.minimum_app_version.as_ref())
            else {
                return Err(HermesDesktopError::InvalidCompatibilityEvidence);
            };
            return Err(HermesDesktopError::DesktopVersionUnsupported {
                detected: detected.clone(),
                minimum: minimum.clone(),
            });
        }
        DesktopCompatibilityStatus::OlderUnsupported => {
            eprintln!("warning: running an older unsupported Hermes Desktop version");
        }
        DesktopCompatibilityStatus::NewerUntested if !allow_untested => {
            let (Some(detected), Some(last)) =
                (version.as_ref(), entry.last_compatible_app_version.as_ref())
            else {
                return Err(HermesDesktopError::InvalidCompatibilityEvidence);
            };
            return Err(HermesDesktopError::DesktopVersionUntested {
                detected: detected.clone(),
                last: last.clone(),
            });
        }
        DesktopCompatibilityStatus::NewerUntested => {
            eprintln!(
                "warning: this Hermes Desktop version is newer than the local compatibility evidence"
            );
        }
        DesktopCompatibilityStatus::Unavailable => {
            return Err(HermesDesktopError::DesktopUnavailable);
        }
        DesktopCompatibilityStatus::Tested => {}
    }
    debug_assert_ne!(entry.evidence, DesktopCompatibilityEvidence::Unavailable);
    Ok(())
}

pub(super) fn missing_desktop_capabilities(help: &str) -> Vec<&'static str> {
    ["--source", "--skip-build", "--cwd"]
        .into_iter()
        .filter(|flag| !help.contains(flag))
        .collect()
}

pub(super) fn extract_semver(output: &str) -> Option<Version> {
    output.split_whitespace().find_map(|candidate| {
        let candidate = candidate.trim_matches(|character: char| {
            !character.is_ascii_digit() && character != '.' && character != '-' && character != '+'
        });
        Version::parse(candidate).ok()
    })
}

pub(super) fn select_model<'a>(
    models: &'a [CodingModelProfile],
    requested: Option<&str>,
) -> Result<&'a str, HermesDesktopError> {
    let selected = requested.unwrap_or(DEFAULT_MODEL_ID);
    if let Some(model) = models.iter().find(|model| model.id == selected) {
        return Ok(&model.id);
    }
    if requested.is_some() {
        return Err(HermesDesktopError::ModelUnavailable {
            model: selected.to_owned(),
            available: models.iter().map(|model| model.id.clone()).collect(),
        });
    }
    models
        .first()
        .map(|model| model.id.as_str())
        .ok_or(HermesDesktopError::EmptyModelCatalog)
}

pub(super) fn desktop_arguments(paths: &DesktopPaths, user_arguments: &[String]) -> Vec<String> {
    let mut arguments = vec!["desktop".to_owned()];
    if packaged_desktop_exists(paths)
        && !has_build_selection(user_arguments)
        && !has_alternate_hermes_root(user_arguments)
    {
        arguments.push("--skip-build".to_owned());
    }
    arguments.extend(user_arguments.iter().cloned());
    arguments
}

pub(super) fn has_alternate_hermes_root(arguments: &[String]) -> bool {
    arguments
        .iter()
        .any(|argument| argument == "--hermes-root" || argument.starts_with("--hermes-root="))
}

pub(super) fn has_build_selection(arguments: &[String]) -> bool {
    arguments.iter().any(|argument| {
        matches!(
            argument.as_str(),
            "--source" | "--skip-build" | "--force-build" | "--build-only"
        )
    })
}

pub(super) fn packaged_desktop_exists(paths: &DesktopPaths) -> bool {
    packaged_desktop_candidates(&paths.install_root)
        .iter()
        .any(|candidate| candidate.is_file())
}

pub(super) async fn bind_stable_gateway(
    paths: &DesktopPaths,
    ownership: &mut OwnershipReceipt,
) -> Result<TcpListener, HermesDesktopError> {
    let listener = match ownership.gateway_port {
        Some(port) => TcpListener::bind(("127.0.0.1", port))
            .await
            .map_err(|source| HermesDesktopError::StablePortUnavailable { port, source })?,
        None => TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(HermesDesktopError::BindGateway)?,
    };
    if ownership.gateway_port.is_none() {
        ownership.gateway_port = Some(
            listener
                .local_addr()
                .map_err(HermesDesktopError::BindGateway)?
                .port(),
        );
        write_json_private(&paths.ownership_receipt, ownership)?;
    }
    Ok(listener)
}
