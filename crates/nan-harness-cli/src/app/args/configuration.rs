use super::launch::WebSearchArgs;
use crate::app::targets::{ConfigTarget, parse_config_harness};
use clap::Args;

#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct ConfigArgs {
    #[arg(
        value_name = "HARNESS",
        help = "Harness whose native user configuration should be managed",
        value_parser = parse_config_harness
    )]
    pub(crate) harness: Option<ConfigTarget>,
    #[command(flatten)]
    pub(crate) search: WebSearchArgs,
    #[arg(
        long,
        help = "Inspect one harness, or all harnesses when HARNESS is omitted",
        conflicts_with_all = ["refresh", "remove", "refresh_all", "remove_all"]
    )]
    pub(crate) status: bool,
    #[arg(
        long,
        help = "Refresh the copied key, model catalog, and managed defaults",
        requires = "harness",
        conflicts_with_all = ["status", "remove", "refresh_all", "remove_all"]
    )]
    pub(crate) refresh: bool,
    #[arg(
        long,
        help = "Remove this managed native configuration safely",
        requires = "harness",
        conflicts_with_all = ["status", "refresh", "refresh_all", "remove_all"]
    )]
    pub(crate) remove: bool,
    #[arg(
        long,
        help = "Refresh every native configuration managed by nan-harness",
        conflicts_with_all = ["harness", "status", "refresh", "remove", "remove_all"]
    )]
    pub(crate) refresh_all: bool,
    #[arg(
        long,
        help = "Remove every native configuration managed by nan-harness",
        conflicts_with_all = ["harness", "status", "refresh", "remove", "refresh_all"]
    )]
    pub(crate) remove_all: bool,
    #[arg(
        short = 'y',
        long,
        help = "Confirm first-time configuration or remove-all without prompting"
    )]
    pub(crate) yes: bool,
}
