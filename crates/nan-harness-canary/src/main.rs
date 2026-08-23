#![forbid(unsafe_code)]

mod aggregate;
mod app;
mod cell;
mod conformance;
mod credentials;
mod record;
mod report;
mod setup;
mod ux;

use app::{Cli, Command};
use clap::Parser as _;
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    match run(Cli::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("canary error: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Command::Setup(arguments) => setup::run(&arguments).await?,
        Command::Ux(arguments) => ux::run(&arguments)?,
        Command::Aggregate(arguments) => aggregate::run(&arguments)?,
        Command::Cell(arguments) => cell::run(&arguments).await?,
        Command::Reproduce(arguments) => cell::reproduce(&arguments).await?,
        Command::ValidateReport(arguments) => {
            let report = report::CanaryReport::read(&arguments.report)?;
            report.validate()?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Command::Record(arguments) => record::run(&arguments)?,
        Command::Conformance(arguments) => conformance::run(&arguments).await?,
    }
    Ok(())
}
