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
    Some(
        run_searxng_host(request)
            .await
            .map_or(ExitCode::FAILURE, |()| ExitCode::SUCCESS),
    )
}
