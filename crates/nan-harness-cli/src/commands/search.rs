use crate::app::{SearchCommand, SearchSetupArgs, SearchStatusArgs};
use crate::commands::persistence::config_directory;
use nan_harness_runtime::search_docker::{
    DEFAULT_HOST_PORT, DockerSearchManager, DockerSearchPaths, DockerSearchRequest,
    DockerSearchStatus, ProcessDockerExecutor,
};
use nan_harness_runtime::searxng::{
    ProcessSearxngCommandExecutor, SearxngInstallPaths, SearxngInstallRequest, SearxngPlatform,
    SearxngSetupPlan, SearxngSourceMetadata, cleanup_owned_searxng_install,
    execute_searxng_install_plan, plan_searxng_installation, read_searxng_install_metadata,
};
use nan_harness_runtime::{
    SearchConfigError, SearchConfigStoreError, SearxngConfig, SearxngMode, load_search_config,
    save_search_config,
};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;

const SEARCH_CONFIG_FILE: &str = "search.json";
const LOCAL_SEARCH_URL: &str = "http://127.0.0.1:8888";
const DOCKER_SEARCH_URL: &str = "http://127.0.0.1:8080";
const MAX_ARCHIVE_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Error)]
pub(crate) enum SearchCommandError {
    #[error("could not determine the nan-harness configuration directory")]
    MissingConfigDirectory,
    #[error("could not determine the current user's home directory")]
    MissingHomeDirectory,
    #[error(
        "NaN web search is not configured; run `nanh search setup --local`, `--docker`, or `--url URL` first"
    )]
    NotConfigured,
    #[error("could not read the saved SearXNG configuration: {0}")]
    LoadConfig(#[source] SearchConfigStoreError),
    #[error("could not save the SearXNG configuration: {0}")]
    SaveConfig(#[source] SearchConfigStoreError),
    #[error("invalid SearXNG endpoint: {0}")]
    InvalidEndpoint(#[source] SearchConfigError),
    #[error("SearXNG setup is not supported on this platform")]
    UnsupportedPlatform,
    #[error("could not prepare the local SearXNG installation: {0}")]
    LocalPlan(#[source] nan_harness_runtime::searxng::SearxngInstallError),
    #[error("could not download the pinned SearXNG source archive: {0}")]
    ArchiveDownload(#[source] reqwest::Error),
    #[error("SearXNG source archive returned HTTP status {0}")]
    ArchiveStatus(u16),
    #[error("SearXNG source archive is larger than the supported 256 MiB limit")]
    ArchiveTooLarge,
    #[error("could not install local SearXNG: {0}")]
    LocalInstall(#[source] nan_harness_runtime::searxng::SearxngInstallError),
    #[error("could not inspect or update managed Docker search: {0}")]
    Docker(#[source] nan_harness_runtime::search_docker::DockerSearchError),
    #[error("could not inspect or remove local SearXNG: {0}")]
    LocalState(#[source] nan_harness_runtime::searxng::SearxngInstallError),
    #[error("cannot change SearXNG while {0} search session(s) are active")]
    ActiveSessions(usize),
    #[error("could not remove the saved SearXNG configuration: {0}")]
    RemoveConfig(#[source] std::io::Error),
    #[error("could not render SearXNG status: {0}")]
    SerializeStatus(#[source] serde_json::Error),
    #[error("could not inspect SearXNG state: {0}")]
    InspectState(#[source] std::io::Error),
}

impl SearchCommandError {
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::InvalidEndpoint(_)
            | Self::UnsupportedPlatform
            | Self::ActiveSessions(_)
            | Self::NotConfigured => "NH-SEARCH-001",
            Self::ArchiveDownload(_) | Self::ArchiveStatus(_) | Self::ArchiveTooLarge => {
                "NH-SEARCH-002"
            }
            Self::Docker(_) | Self::LocalPlan(_) | Self::LocalInstall(_) | Self::LocalState(_) => {
                "NH-SEARCH-003"
            }
            Self::MissingConfigDirectory
            | Self::MissingHomeDirectory
            | Self::LoadConfig(_)
            | Self::SaveConfig(_)
            | Self::RemoveConfig(_)
            | Self::SerializeStatus(_)
            | Self::InspectState(_) => "NH-SEARCH-004",
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchStatus {
    configured: bool,
    enabled: bool,
    mode: Option<SearxngMode>,
    url: Option<String>,
    backend: Option<&'static str>,
    running: Option<bool>,
}

pub(crate) async fn run(command: &SearchCommand) -> Result<(), SearchCommandError> {
    match command {
        SearchCommand::Setup(arguments) => setup(arguments).await,
        SearchCommand::Status(arguments) => status(arguments),
        SearchCommand::Disable => disable(),
        SearchCommand::Update => update().await,
        SearchCommand::Remove => remove(),
    }
}

async fn setup(arguments: &SearchSetupArgs) -> Result<(), SearchCommandError> {
    let paths = state_paths()?;
    refuse_active_sessions(&paths, arguments.local || arguments.docker)?;
    let (config, backend) = if arguments.local {
        let home = home_directory()?;
        setup_local(&home).await?
    } else if arguments.docker {
        let home = home_directory()?;
        setup_docker(&home)?
    } else {
        let value = arguments
            .url
            .as_deref()
            .ok_or(SearchCommandError::InvalidEndpoint(
                SearchConfigError::InvalidUrl,
            ))?;
        (
            SearxngConfig::remote(value).map_err(SearchCommandError::InvalidEndpoint)?,
            "remote",
        )
    };

    save_search_config(&paths.config_file, &config).map_err(SearchCommandError::SaveConfig)?;
    println!(
        "NaN web search configured ({backend}) at {}. Credentials are not required.",
        config.base_url_string()
    );
    Ok(())
}

fn status(arguments: &SearchStatusArgs) -> Result<(), SearchCommandError> {
    let paths = state_paths()?;
    let config = load_search_config(&paths.config_file).map_err(SearchCommandError::LoadConfig)?;
    let status = match config {
        Some(config) => configured_status(&paths, config)?,
        None => SearchStatus {
            configured: false,
            enabled: false,
            mode: None,
            url: None,
            backend: None,
            running: None,
        },
    };

    if arguments.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&status).map_err(SearchCommandError::SerializeStatus)?
        );
    } else if !status.configured {
        println!(
            "NaN web search is disabled. Run `nanh search setup --local`, `--docker`, or `--url URL` to enable it."
        );
    } else {
        let backend = status.backend.unwrap_or("configured");
        let running = status.running.map_or(String::new(), |value| {
            if value { " (running)" } else { " (stopped)" }.to_owned()
        });
        println!(
            "NaN web search is enabled ({backend}) at {}{running}.",
            status.url.as_deref().unwrap_or("unknown endpoint")
        );
    }
    Ok(())
}

fn configured_status(
    paths: &SearchPaths,
    config: SearxngConfig,
) -> Result<SearchStatus, SearchCommandError> {
    let (backend, running) = match config.mode() {
        SearxngMode::Remote => ("remote", None),
        SearxngMode::Local => {
            let local_install = paths
                .local_install
                .as_ref()
                .ok_or(SearchCommandError::UnsupportedPlatform)?;
            let installed = read_searxng_install_metadata(local_install)
                .map_err(SearchCommandError::LocalState)?
                .is_some();
            ("local", Some(installed))
        }
        SearxngMode::Docker => {
            let manager = docker_manager(&paths.home)?;
            let running = match manager
                .status_without_starting()
                .map_err(SearchCommandError::Docker)?
            {
                DockerSearchStatus::Running(_) => true,
                DockerSearchStatus::Absent | DockerSearchStatus::Stopped(_) => false,
            };
            ("docker", Some(running))
        }
    };
    Ok(SearchStatus {
        configured: true,
        enabled: true,
        mode: Some(config.mode()),
        url: Some(config.base_url_string()),
        backend: Some(backend),
        running,
    })
}

fn disable() -> Result<(), SearchCommandError> {
    let paths = state_paths()?;
    let config = load_search_config(&paths.config_file).map_err(SearchCommandError::LoadConfig)?;
    if config.is_none() {
        println!("NaN web search is already disabled.");
        return Ok(());
    }
    refuse_active_sessions(&paths, true)?;
    remove_config(&paths.config_file)?;
    println!(
        "NaN web search disabled. The managed backend was retained; run `nanh search setup` to enable it again."
    );
    Ok(())
}

async fn update() -> Result<(), SearchCommandError> {
    let paths = state_paths()?;
    let Some(config) =
        load_search_config(&paths.config_file).map_err(SearchCommandError::LoadConfig)?
    else {
        return Err(SearchCommandError::NotConfigured);
    };
    refuse_active_sessions(&paths, true)?;
    match config.mode() {
        SearxngMode::Remote => {
            println!("Remote SearXNG endpoints are managed externally; no update was performed.");
        }
        SearxngMode::Local => {
            let home = home_directory()?;
            install_local(&home, true).await?;
            println!("Managed local SearXNG was updated.");
        }
        SearxngMode::Docker => {
            let manager = docker_manager(&paths.home)?;
            let plan = manager
                .plan(nan_harness_runtime::search_docker::DockerOperation::Update)
                .map_err(SearchCommandError::Docker)?;
            nan_harness_runtime::search_docker::execute_docker_search_update(
                &plan,
                active_sessions(&paths.docker.root())?,
                ProcessDockerExecutor,
            )
            .map_err(SearchCommandError::Docker)?;
            println!("Managed Docker SearXNG was updated.");
        }
    }
    Ok(())
}

fn remove() -> Result<(), SearchCommandError> {
    let paths = state_paths()?;
    let Some(config) =
        load_search_config(&paths.config_file).map_err(SearchCommandError::LoadConfig)?
    else {
        println!("NaN web search is already removed.");
        return Ok(());
    };
    refuse_active_sessions(&paths, true)?;
    match config.mode() {
        SearxngMode::Remote => {}
        SearxngMode::Local => {
            let local_install = paths
                .local_install
                .as_ref()
                .ok_or(SearchCommandError::UnsupportedPlatform)?;
            cleanup_owned_searxng_install(local_install).map_err(SearchCommandError::LocalState)?;
        }
        SearxngMode::Docker => {
            docker_manager(&paths.home)?
                .remove(active_sessions(&paths.docker.root())?)
                .map_err(SearchCommandError::Docker)?;
        }
    }
    remove_config(&paths.config_file)?;
    println!("NaN web search backend removed.");
    Ok(())
}

async fn setup_local(home: &Path) -> Result<(SearxngConfig, &'static str), SearchCommandError> {
    install_local(home, false).await?;
    Ok((
        SearxngConfig::local(LOCAL_SEARCH_URL).map_err(SearchCommandError::InvalidEndpoint)?,
        "local",
    ))
}

async fn install_local(home: &Path, update: bool) -> Result<(), SearchCommandError> {
    let platform = SearxngPlatform::current().ok_or(SearchCommandError::UnsupportedPlatform)?;
    let paths = SearxngInstallPaths::for_user_home(home, platform);
    let source = SearxngSourceMetadata::official();
    let request = SearxngInstallRequest::with_python_bootstrap();
    let plan = plan_searxng_installation(request, Some(platform), paths, source.clone())
        .map_err(SearchCommandError::LocalPlan)?;
    if update || matches!(plan, SearxngSetupPlan::Setup(_)) {
        let archive = download_archive(&source.archive_url).await?;
        execute_searxng_install_plan(&plan, &archive, &ProcessSearxngCommandExecutor)
            .map_err(SearchCommandError::LocalInstall)?;
    }
    Ok(())
}

fn setup_docker(home: &Path) -> Result<(SearxngConfig, &'static str), SearchCommandError> {
    let manager = docker_manager(home)?;
    let plan = manager.setup_plan().map_err(SearchCommandError::Docker)?;
    nan_harness_runtime::search_docker::execute_docker_search_setup(
        &plan,
        0,
        ProcessDockerExecutor,
    )
    .map_err(SearchCommandError::Docker)?;
    Ok((
        SearxngConfig::docker(DOCKER_SEARCH_URL).map_err(SearchCommandError::InvalidEndpoint)?,
        "docker",
    ))
}

fn docker_manager(
    home: &Path,
) -> Result<DockerSearchManager<ProcessDockerExecutor>, SearchCommandError> {
    DockerSearchManager::new(
        ProcessDockerExecutor,
        DockerSearchPaths::for_user_home(home),
        DockerSearchRequest::configured(DEFAULT_HOST_PORT),
    )
    .map_err(SearchCommandError::Docker)
}

#[derive(Debug)]
struct SearchPaths {
    home: PathBuf,
    config_file: PathBuf,
    local_install: Option<SearxngInstallPaths>,
    docker: DockerSearchPaths,
}

fn state_paths() -> Result<SearchPaths, SearchCommandError> {
    let home = home_directory()?;
    let directory = config_directory().ok_or(SearchCommandError::MissingConfigDirectory)?;
    Ok(SearchPaths {
        config_file: directory.join(SEARCH_CONFIG_FILE),
        local_install: SearxngPlatform::current()
            .map(|platform| SearxngInstallPaths::for_user_home(&home, platform)),
        docker: DockerSearchPaths::for_user_home(&home),
        home,
    })
}

fn home_directory() -> Result<PathBuf, SearchCommandError> {
    #[cfg(windows)]
    let value = std::env::var_os("USERPROFILE");
    #[cfg(not(windows))]
    let value = std::env::var_os("HOME");
    value
        .map(PathBuf::from)
        .ok_or(SearchCommandError::MissingHomeDirectory)
}

fn remove_config(path: &Path) -> Result<(), SearchCommandError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(SearchCommandError::RemoveConfig(error)),
    }
}

fn refuse_active_sessions(
    paths: &SearchPaths,
    destructive: bool,
) -> Result<(), SearchCommandError> {
    if !destructive {
        return Ok(());
    }
    let local = paths
        .local_install
        .as_ref()
        .map(|path| active_sessions(path.root()))
        .transpose()?
        .unwrap_or(0);
    let count = local.saturating_add(active_sessions(paths.docker.root())?);
    if count == 0 {
        Ok(())
    } else {
        Err(SearchCommandError::ActiveSessions(count))
    }
}

fn active_sessions(path: &Path) -> Result<usize, SearchCommandError> {
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(SearchCommandError::InspectState(error)),
    };
    Ok(entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".nan-harness-searxng-interest-")
        })
        .count())
}

async fn download_archive(url: &str) -> Result<Vec<u8>, SearchCommandError> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(SearchCommandError::ArchiveDownload)?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(SearchCommandError::ArchiveDownload)?;
    if !response.status().is_success() {
        return Err(SearchCommandError::ArchiveStatus(
            response.status().as_u16(),
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_ARCHIVE_BYTES)
    {
        return Err(SearchCommandError::ArchiveTooLarge);
    }
    let body = response
        .bytes()
        .await
        .map_err(SearchCommandError::ArchiveDownload)?;
    if body.len() as u64 > MAX_ARCHIVE_BYTES {
        return Err(SearchCommandError::ArchiveTooLarge);
    }
    Ok(body.to_vec())
}
