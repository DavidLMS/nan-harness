mod artifacts;
mod compatibility;
mod validation;
mod verification;
mod versioning;

pub(crate) use artifacts::generate_metadata;
pub(crate) use compatibility::{
    generate_compatibility_feed, merge_compatibility_feed, validate_compatibility_feed,
};
pub(crate) use validation::{validate_changelog, validate_tag, write_changelog_notes};
pub(crate) use versioning::set_version;

#[cfg(test)]
mod tests;
