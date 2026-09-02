mod discovery;
mod json;
mod models;
mod report;
mod text;

use crate::app::{DoctorArgs, DoctorTarget};
use nan_harness_runtime::DiscoveryError;

pub(crate) async fn run(arguments: &DoctorArgs) -> Result<i32, DiscoveryError> {
    match arguments.harness {
        Some(DoctorTarget::Stable(harness)) if arguments.json => {
            Ok(json::print_harness_report(harness, arguments))
        }
        Some(DoctorTarget::Stable(harness)) => {
            text::print_harness_report(harness, arguments)?;
            Ok(0)
        }
        Some(DoctorTarget::Experimental(kind)) => {
            if arguments.json {
                Ok(json::print_experimental_report(kind))
            } else {
                Ok(text::print_experimental_report(kind))
            }
        }
        None if arguments.json => Ok(json::print_system_report(&report::system_json_report(
            discovery::system().await,
        ))),
        None => {
            let report = report::system_text_report(discovery::system().await);
            print!("{}", text::render_system_report(report));
            Ok(0)
        }
    }
}
