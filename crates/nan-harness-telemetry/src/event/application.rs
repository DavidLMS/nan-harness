use serde::{Deserialize, Serialize};

const APPLICATION_NAME: &str = "nan-harness";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Application {
    name: String,
    version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    build_commit: Option<String>,
}

impl Application {
    pub(super) fn current() -> Self {
        Self {
            name: APPLICATION_NAME.to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            build_commit: option_env!("NAN_BUILD_COMMIT")
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned),
        }
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    #[must_use]
    pub fn build_commit(&self) -> Option<&str> {
        self.build_commit.as_deref()
    }
}
