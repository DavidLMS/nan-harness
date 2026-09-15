use nan_harness_runtime::run_searxng_host;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::ExitCode;

const SUBCOMMAND: &str = "__searxng-host";

pub(crate) async fn run_if_requested() -> Option<ExitCode> {
    let mut arguments = std::env::args_os();
    let _executable = arguments.next();
    if arguments.next().as_deref() != Some(OsStr::new(SUBCOMMAND)) {
        return None;
    }
    let Some(request) = arguments.next().map(PathBuf::from) else {
        return Some(ExitCode::FAILURE);
    };
    if arguments.next().is_some() {
        return Some(ExitCode::FAILURE);
    }
    // The host outlives its launcher by design, so it must not keep the launcher's standard
    // handles open. Release them before anything can write and be cached.
    let _ = nan_harness_detach::release_inherited_standard_handles();
    Some(
        run_searxng_host(request)
            .await
            .map_or(ExitCode::FAILURE, |()| ExitCode::SUCCESS),
    )
}
