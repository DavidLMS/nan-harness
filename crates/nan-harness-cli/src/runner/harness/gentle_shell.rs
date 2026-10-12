use super::{
    BridgeDiagnostic, CliError, HarnessKind, HarnessRunArgs, HarnessRunMode, LaunchSession,
    PreparedTerminalLaunch, RunError, discovery_options, inspect_harness, prepare_terminal_launch,
    run_discovered_harness, validate_limit_request,
};
use crate::app::DirectHarnessRunArgs;
use crate::commands::credentials::CredentialError;
use nan_harness_adapters::GentleShellAdapter;
use nan_harness_core::SecretValue;
use nan_harness_runtime::{ConfigOverrides, ConfigResolver, ProcessEnvironment};
use std::path::Path;
mod configuration;
use configuration::native_configuration;

pub(super) async fn run(
    input: &DirectHarnessRunArgs,
    interactive: bool,
    cwd: &Path,
    diagnostics: &mut Vec<BridgeDiagnostic>,
) -> Result<i32, RunError> {
    let mut args = input.run.clone();
    validate_session_arguments(&args.arguments)?;
    let executable = match &args.executable {
        Some(path) => path.clone(),
        None => nan_harness_runtime::locate_launcher("gentle-shell").map_err(CliError::from)?,
    };
    let discovery = inspect_harness(HarnessKind::Pi, &executable, discovery_options(&args))
        .map_err(CliError::from)?;
    let environment =
        nan_harness_runtime::inspect_gentle_shell(&executable, &home_selectors(&args.arguments))
            .map_err(CliError::from)?;
    let home = environment.agent_home;
    let home = if home.is_absolute() {
        home
    } else {
        cwd.join(home)
    };
    if !version_supported(&discovery.harness.detected_version)
        || !launcher_supported(&environment.launcher_version)
    {
        return Err(configuration::invalid(
            "gentle-shell",
            nan_harness_i18n::messages::detail_gentle_shell_contract,
        )
        .into());
    }
    let native_model = configuration::default_model(&home, cwd)?;
    validate_limit_request(
        HarnessKind::Pi,
        args.session_max_tokens,
        args.context,
        input.no_chat_gateway,
    )?;
    let native = native_configuration(&home, input.no_chat_gateway, args.dry_run)?;
    if args.provider_base_url.is_none() {
        args.provider_base_url = native.base_url;
    }
    let native_key = native.key;
    let adapter = GentleShellAdapter {
        agent_home: home.to_string_lossy().into_owned(),
        preserve_provider: input.no_chat_gateway
            && native_key.is_some()
            && input.run.provider_base_url.is_none(),
    };
    let launch = if args.dry_run {
        None
    } else {
        Some(prepare(&args, native_key, interactive).await?)
    };
    if args.model.is_none() {
        args.model = Some(select_default(native_model, launch.as_ref()));
    }
    for warning in discovery.terminal_warnings(nan_harness_i18n::locale()) {
        eprintln!(
            "{}",
            nan_harness_i18n::messages::harness_warning(nan_harness_i18n::locale(), &warning)
        );
    }
    let mode = HarnessRunMode {
        disable_direct_chat_gateway: input.no_chat_gateway,
        interactive,
    };
    run_discovered_harness(
        &args,
        &adapter,
        mode,
        launch.as_ref(),
        cwd,
        diagnostics,
        &discovery,
    )
    .await
    .map_err(|error| RunError::after_discovery(error, discovery.harness))
}

fn select_default(native: Option<String>, launch: Option<&PreparedTerminalLaunch>) -> String {
    native
        .filter(|model| {
            launch.is_none_or(|launch| launch.model_catalog.iter().any(|entry| entry.id == *model))
        })
        .unwrap_or_else(|| crate::runner::DEFAULT_MODEL_ID.to_owned())
}

async fn prepare(
    args: &HarnessRunArgs,
    native_key: Option<SecretValue>,
    interactive: bool,
) -> Result<PreparedTerminalLaunch, CliError> {
    if let Some(key) = native_key {
        let config = ConfigResolver::resolve(
            &ProcessEnvironment,
            ConfigOverrides {
                provider_base_url: args.provider_base_url.clone(),
                nan_api_key: Some(key),
            },
        )
        .map_err(CredentialError::from)?;
        let model_catalog = LaunchSession::new(&config).model_catalog().await?.to_vec();
        return Ok(PreparedTerminalLaunch {
            config,
            model_catalog,
        });
    }
    prepare_terminal_launch(args.provider_base_url.clone(), interactive).await
}

fn validate_session_arguments(arguments: &[String]) -> Result<(), CliError> {
    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        if matches!(argument.as_str(), "--home" | "--package-root") {
            arguments.next();
            continue;
        }
        if matches!(argument.as_str(), "--link" | "--isolated")
            || argument.starts_with("--home=")
            || argument.starts_with("--package-root=")
        {
            continue;
        }
        if matches!(
            argument.as_str(),
            "home"
                | "setup"
                | "upgrade"
                | "self-uninstall"
                | "install"
                | "remove"
                | "list"
                | "update"
                | "config"
                | "auth"
        ) {
            return Err(configuration::invalid(
                "gentle-shell",
                nan_harness_i18n::messages::detail_gentle_shell_arguments,
            ));
        }
        break;
    }
    Ok(())
}

fn home_selectors(arguments: &[String]) -> Vec<String> {
    let mut result = Vec::new();
    let mut args = arguments.iter();
    while let Some(arg) = args.next() {
        if arg == "--" {
            break;
        }
        if arg == "--home" {
            result.push(arg.clone());
            if let Some(value) = args.next() {
                result.push(value.clone());
            }
        } else if arg == "--link" || arg == "--isolated" || arg.starts_with("--home=") {
            result.push(arg.clone());
        }
    }
    result
}

fn version_supported(version: &str) -> bool {
    version
        .strip_prefix("pi ")
        .and_then(|value| semver::Version::parse(value).ok())
        .is_some_and(|version| version >= semver::Version::new(1, 1, 0))
}

fn launcher_supported(version: &str) -> bool {
    version
        .strip_prefix("gentle-shell ")
        .and_then(|value| semver::Version::parse(value).ok())
        .is_some_and(|version| version >= semver::Version::new(4, 1, 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_probe_only_receives_launcher_selectors() {
        let args = [
            "--home",
            "/home/with spaces",
            "-p",
            "private prompt",
            "--",
            "--link",
        ]
        .map(str::to_owned);
        assert_eq!(home_selectors(&args), ["--home", "/home/with spaces"]);
    }

    #[test]
    fn native_provider_api_requires_qualified_pi_version() {
        assert!(version_supported("pi 1.1.0"));
        assert!(!version_supported("pi 0.85.1"));
        assert!(!version_supported("gentle-shell 4.1.0"));
    }
}
