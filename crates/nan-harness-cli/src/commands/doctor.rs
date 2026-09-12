mod discovery;
mod json;
mod models;
mod report;
mod text;

use crate::app::{DoctorArgs, DoctorTarget};
use nan_harness_runtime::DiscoveryError;

pub(crate) async fn run(arguments: &DoctorArgs) -> Result<i32, DiscoveryError> {
    if arguments.offline && !arguments.json {
        println!(
            "{}", nan_harness_i18n::messages::doctor_info_offline_nan_harness_network_activity_and_credential_resolution_skipped(nan_harness_i18n::locale()));
        println!("{}", nan_harness_i18n::messages::doctor_info_compatibility_data_local_cached_or_embedded_evidence_not_refreshed(nan_harness_i18n::locale()));
    }
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
                Ok(json::print_experimental_report(kind, arguments.offline))
            } else {
                Ok(text::print_experimental_report(kind))
            }
        }
        None if arguments.json => Ok(json::print_system_report(&report::system_json_report(
            discovery::system(arguments.offline).await,
        ))),
        None => {
            let report = report::system_text_report(discovery::system(arguments.offline).await);
            let exit_code = i32::from(report.managed_configurations.has_errors());
            print!("{}", text::render_system_report(report));
            Ok(exit_code)
        }
    }
}
