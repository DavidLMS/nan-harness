mod artifacts;
mod compatibility;
mod desktop;
mod desktop_checks;
mod hosted_checks;
pub(crate) use hosted_checks::merge_release_hosted_checks;
mod validation;
mod verification;
mod versioning;

pub(crate) use artifacts::generate_metadata;
pub(crate) use compatibility::{
    generate_compatibility_feed, generate_hosted_compatibility_feed,
    generate_unified_compatibility_feed, generate_versioned_compatibility_feed,
    merge_compatibility_feed, merge_hosted_compatibility_feed, merge_unified_compatibility_feed,
    merge_versioned_compatibility_feed, validate_compatibility_feed,
    validate_hosted_compatibility_feed, validate_unified_compatibility_feed,
    validate_versioned_compatibility_feed,
};
pub(crate) use desktop_checks::merge_release_checks;
pub(crate) use validation::{validate_changelog, validate_tag, write_changelog_notes};
pub(crate) use versioning::set_version;

#[cfg(test)]
mod tests;
