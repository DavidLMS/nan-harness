use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    nan_harness_cli::main_entry().await
}
