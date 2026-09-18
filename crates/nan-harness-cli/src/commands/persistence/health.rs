use serde::Serialize;
use std::{fs, io::ErrorKind, path::Path};

/// Ordered so a failed inspection cannot be hidden by a missing or edited document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ConfigurationHealth {
    Active,
    Missing,
    Changed,
    Invalid,
    Unreadable,
}

impl ConfigurationHealth {
    pub(crate) const fn from_matches(matches: bool) -> Self {
        if matches { Self::Active } else { Self::Changed }
    }

    pub(crate) const fn is_active(self) -> bool {
        matches!(self, Self::Active)
    }

    pub(crate) const fn error_code(self) -> Option<&'static str> {
        match self {
            Self::Invalid => Some("NH-CONFIG-006"),
            Self::Unreadable => Some("NH-CONFIG-007"),
            Self::Active | Self::Missing | Self::Changed => None,
        }
    }

    pub(crate) fn terminal_label(self, locale: nan_harness_i18n::Locale) -> &'static str {
        use nan_harness_i18n::messages;
        match self {
            Self::Active => messages::configuration_active_text(locale),
            Self::Missing => messages::configuration_missing_text(locale),
            Self::Changed => messages::configuration_changed_text(locale),
            Self::Invalid => messages::configuration_invalid_text(locale),
            Self::Unreadable => messages::configuration_unreadable_text(locale),
        }
    }

    pub(crate) fn recovery_hint(self) -> Option<&'static str> {
        match self {
            Self::Invalid => Some(nan_harness_i18n::messages::configuration_syntax_hint_text(
                nan_harness_i18n::locale(),
            )),
            Self::Unreadable => Some(nan_harness_i18n::messages::configuration_access_hint_text(
                nan_harness_i18n::locale(),
            )),
            Self::Active | Self::Missing | Self::Changed => None,
        }
    }
}

pub(crate) fn read_managed_document(path: &Path) -> Result<Vec<u8>, ConfigurationHealth> {
    let metadata = fs::metadata(path).map_err(|error| read_health(error.kind()))?;
    if !metadata.is_file() {
        return Err(ConfigurationHealth::Unreadable);
    }
    fs::read(path).map_err(|error| read_health(error.kind()))
}

pub(super) fn read_managed_jsonc(
    path: &Path,
) -> Result<jsonc_parser::cst::CstObject, ConfigurationHealth> {
    let contents = read_managed_document(path)?;
    let source = std::str::from_utf8(&contents).map_err(|_| ConfigurationHealth::Invalid)?;
    jsonc_parser::cst::CstRootNode::parse(source, &jsonc_parser::ParseOptions::default())
        .map_err(|_| ConfigurationHealth::Invalid)?
        .object_value()
        .ok_or(ConfigurationHealth::Invalid)
}

fn read_health(kind: ErrorKind) -> ConfigurationHealth {
    match kind {
        ErrorKind::NotFound => ConfigurationHealth::Missing,
        _ => ConfigurationHealth::Unreadable,
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn managed_document_read_failure_is_unreadable_instead_of_missing() {
        let root = tempfile::tempdir().expect("temporary directory");
        let path = root.path().join("loop");
        std::os::unix::fs::symlink("loop", &path).expect("unreadable symlink loop");
        assert_eq!(
            read_managed_document(&path),
            Err(ConfigurationHealth::Unreadable)
        );
    }
}
