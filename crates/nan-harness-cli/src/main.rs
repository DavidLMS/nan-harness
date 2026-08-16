use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "nan-harness",
    version,
    about = "Run AI coding harnesses through NaN"
)]
struct Cli;

fn main() {
    let _cli = Cli::parse();
}
