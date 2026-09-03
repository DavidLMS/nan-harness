mod arguments;
mod error;
mod json_rpc;
mod response_limits;
mod transport;

use arguments::Arguments;
use error::fail;
use json_rpc::SearchMcp;
use std::process::ExitCode;

const SUBCOMMAND: &str = "__search-mcp";

pub(crate) async fn run_if_requested() -> Option<ExitCode> {
    let mut values = std::env::args_os();
    let _executable = values.next();
    if values.next().as_deref() != Some(std::ffi::OsStr::new(SUBCOMMAND)) {
        return None;
    }
    Some(match Arguments::parse(values) {
        Ok(arguments) => match SearchMcp::new(arguments) {
            Ok(server) => server.run().await,
            Err(error) => fail(&error),
        },
        Err(error) => fail(&error),
    })
}
