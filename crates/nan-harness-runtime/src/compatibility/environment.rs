use std::env;

pub const COMPATIBILITY_MANIFEST_ENVIRONMENT_VARIABLE: &str = "NAN_COMPATIBILITY_MANIFEST_URL";
pub const DISABLE_COMPATIBILITY_REFRESH_ENVIRONMENT_VARIABLE: &str = "NAN_NO_COMPATIBILITY_CHECK";
const BUILD_COMPATIBILITY_MANIFEST_URL: Option<&str> =
    option_env!("NAN_COMPATIBILITY_MANIFEST_URL");

#[must_use]
pub fn automatic_refresh_enabled() -> bool {
    !environment_flag(DISABLE_COMPATIBILITY_REFRESH_ENVIRONMENT_VARIABLE)
        && env::var_os("CI").is_none()
}

#[must_use]
pub fn compatibility_manifest_url() -> Option<String> {
    env::var(COMPATIBILITY_MANIFEST_ENVIRONMENT_VARIABLE)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| BUILD_COMPATIBILITY_MANIFEST_URL.map(ToOwned::to_owned))
}

fn environment_flag(name: &str) -> bool {
    env::var(name).is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}
