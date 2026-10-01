use nan_harness_i18n::{locale, messages};
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
pub(super) enum SearchState {
    #[serde(rename = "disabled")]
    Disabled,
    #[serde(rename = "missing")]
    Missing,
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "stopped")]
    Stopped,
    #[serde(rename = "unknown")]
    Unknown,
    #[serde(rename = "reachable")]
    Reachable,
    #[serde(rename = "unavailable")]
    Unavailable,
}

impl SearchState {
    pub(super) fn terminal_label(self) -> String {
        match self {
            Self::Disabled => messages::search_state_disabled(locale()),
            Self::Missing => messages::search_state_missing(locale()),
            Self::Running => messages::search_state_running(locale()),
            Self::Stopped => messages::search_state_stopped(locale()),
            Self::Unknown => messages::search_state_unknown(locale()),
            Self::Reachable => messages::search_state_reachable(locale()),
            Self::Unavailable => messages::search_state_unavailable(locale()),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub(super) enum SearchProblem {
    #[serde(rename = "endpoint verification failed")]
    EndpointVerification,
    #[serde(rename = "managed local installation is missing")]
    LocalMissing,
    #[serde(rename = "managed Docker container is absent")]
    DockerMissing,
    #[serde(rename = "managed Docker state could not be inspected")]
    DockerInspection,
}

impl SearchProblem {
    pub(super) fn terminal_label(self) -> String {
        match self {
            Self::EndpointVerification => messages::search_problem_endpointverification(locale()),
            Self::LocalMissing => messages::search_problem_localmissing(locale()),
            Self::DockerMissing => messages::search_problem_dockermissing(locale()),
            Self::DockerInspection => messages::search_problem_dockerinspection(locale()),
        }
    }
}
