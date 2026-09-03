use nan_harness_core::{DesktopHarnessKind, HarnessKind};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DoctorTarget {
    Stable(HarnessKind),
    Experimental(DesktopHarnessKind),
}

impl FromStr for DoctorTarget {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if let Ok(kind) = DesktopHarnessKind::from_str(value) {
            return Ok(Self::Experimental(kind));
        }
        HarnessKind::from_str(value)
            .map(Self::Stable)
            .map_err(|error| error.to_string())
    }
}

impl fmt::Display for DoctorTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stable(kind) => kind.fmt(formatter),
            Self::Experimental(kind) => kind.fmt(formatter),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfigTarget {
    Stable(HarnessKind),
    Pen,
}

impl ConfigTarget {
    pub(crate) const fn stable(self) -> Option<HarnessKind> {
        match self {
            Self::Stable(kind) => Some(kind),
            Self::Pen => None,
        }
    }
}

impl fmt::Display for ConfigTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stable(kind) => kind.fmt(formatter),
            Self::Pen => formatter.write_str("pen"),
        }
    }
}

pub(super) fn parse_config_harness(value: &str) -> Result<ConfigTarget, String> {
    if matches!(value, "pen" | "pen-desktop") {
        return Ok(ConfigTarget::Pen);
    }
    if value == "hermes-desktop" {
        return Ok(ConfigTarget::Stable(HarnessKind::Hermes));
    }
    HarnessKind::from_str(value)
        .map(ConfigTarget::Stable)
        .map_err(|error| error.to_string())
}
