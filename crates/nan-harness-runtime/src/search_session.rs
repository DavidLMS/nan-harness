use crate::search_docker::DockerSearchPaths;
use crate::search_supervisor::{SearchInterest, SearchSupervisor};
use nan_harness_search::{SearxngConfig, SearxngMode};
use std::path::{Path, PathBuf};
use tokio::task::JoinHandle;

/// Keeps optional search owned for the lifetime of a desktop bridge or a Docker-backed launch.
/// Startup remains advisory; dropping the bridge also cancels an unfinished acquisition.
pub(crate) struct ManagedSearchSession {
    local: Option<JoinHandle<()>>,
    _docker: Option<SearchInterest>,
}

impl ManagedSearchSession {
    pub(crate) fn start(config: Option<&SearxngConfig>) -> Option<Self> {
        Self::for_home(config?, search_home()?.as_path())
    }

    fn for_home(config: &SearxngConfig, home: &Path) -> Option<Self> {
        match config.mode() {
            SearxngMode::Remote => None,
            SearxngMode::Docker => {
                let paths = DockerSearchPaths::for_user_home(home);
                if !paths.is_owned_root().ok()? {
                    return None;
                }
                Some(Self {
                    local: None,
                    _docker: Some(SearchInterest::acquire(paths.root()).ok()?),
                })
            }
            SearxngMode::Local => {
                let supervisor =
                    SearchSupervisor::from_standalone_install(config, Some(home)).ok()??;
                Some(Self {
                    local: Some(tokio::spawn(async move {
                        let Ok(Some(_lease)) = supervisor.acquire().await else {
                            return;
                        };
                        std::future::pending::<()>().await;
                    })),
                    _docker: None,
                })
            }
        }
    }
}

impl Drop for ManagedSearchSession {
    fn drop(&mut self) {
        if let Some(task) = &self.local {
            task.abort();
        }
    }
}

pub(crate) fn search_home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search_supervisor::active_search_interests;

    #[test]
    fn docker_session_holds_and_releases_interest_without_starting_docker() {
        let home = tempfile::tempdir().expect("isolated home");
        let paths = DockerSearchPaths::for_user_home(home.path());
        std::fs::create_dir_all(paths.root()).expect("owned Docker root");
        std::fs::write(
            paths.root().join(".nanh-owned"),
            b"nanh-search-docker-root-v1\n",
        )
        .expect("ownership marker");
        let config = SearxngConfig::docker("http://127.0.0.1:8080").expect("endpoint");
        let session = ManagedSearchSession::for_home(&config, home.path()).expect("session");
        assert_eq!(
            active_search_interests(paths.root()).expect("active interests"),
            1
        );
        drop(session);
        assert_eq!(
            active_search_interests(paths.root()).expect("released interests"),
            0
        );
    }

    #[test]
    fn missing_installations_and_remote_endpoints_create_no_backend_state() {
        let home = tempfile::tempdir().expect("isolated home");
        for config in [
            SearxngConfig::local("http://127.0.0.1:8888").expect("local"),
            SearxngConfig::docker("http://127.0.0.1:8080").expect("Docker"),
            SearxngConfig::remote("https://search.example.test").expect("remote"),
        ] {
            assert!(ManagedSearchSession::for_home(&config, home.path()).is_none());
        }
        assert_eq!(std::fs::read_dir(home.path()).expect("home").count(), 0);
    }
}
