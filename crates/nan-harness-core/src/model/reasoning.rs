use serde::{Deserialize, Serialize};

/// Reasoning effort values accepted by models with an effort-based policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    Low,
    Medium,
    High,
    Max,
}

/// Small, copyable capability set with an array-shaped persistence format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupportedReasoningEfforts {
    values: [ReasoningEffort; 4],
    len: usize,
}

impl SupportedReasoningEfforts {
    pub const STANDARD: Self = Self {
        values: [
            ReasoningEffort::Low,
            ReasoningEffort::Medium,
            ReasoningEffort::High,
            ReasoningEffort::High,
        ],
        len: 3,
    };
    pub const ALL: Self = Self {
        values: [
            ReasoningEffort::Low,
            ReasoningEffort::Medium,
            ReasoningEffort::High,
            ReasoningEffort::Max,
        ],
        len: 4,
    };

    #[must_use]
    pub fn contains(&self, effort: &ReasoningEffort) -> bool {
        self.values[..self.len].contains(effort)
    }

    pub fn iter(self) -> impl Iterator<Item = ReasoningEffort> {
        self.into_iter()
    }
}

impl IntoIterator for SupportedReasoningEfforts {
    type Item = ReasoningEffort;
    type IntoIter = std::iter::Take<std::array::IntoIter<ReasoningEffort, 4>>;

    fn into_iter(self) -> Self::IntoIter {
        self.values.into_iter().take(self.len)
    }
}

impl Serialize for SupportedReasoningEfforts {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.values[..self.len].serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SupportedReasoningEfforts {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let values = Vec::<ReasoningEffort>::deserialize(deserializer)?;
        if values.is_empty() || values.len() > 4 {
            return Err(serde::de::Error::custom(
                "expected one to four supported reasoning efforts",
            ));
        }
        let mut result = Self::STANDARD;
        for (index, effort) in values.iter().enumerate() {
            if values[..index].contains(effort) {
                return Err(serde::de::Error::custom(
                    "duplicate supported reasoning effort",
                ));
            }
            result.values[index] = *effort;
        }
        result.len = values.len();
        Ok(result)
    }
}

/// Harness-provided reasoning preference before model capabilities are applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasoningHint {
    Disabled,
    Low,
    Medium,
    High,
    ExtraHigh,
}

/// A model's declared reasoning control contract.
///
/// `Unknown` is deliberately different from `Unsupported`: the former means
/// that NaN has no profile for the model, while the latter is an explicit
/// statement in bundled metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum ReasoningPolicy {
    Toggle {
        default_enabled: bool,
    },
    Effort {
        supported: SupportedReasoningEfforts,
        default: ReasoningEffort,
        #[serde(default)]
        supports_disabled: bool,
    },
    AlwaysOn,
    Unsupported,
    Unknown,
}

impl ReasoningPolicy {
    #[must_use]
    pub const fn is_unknown(self) -> bool {
        matches!(self, Self::Unknown)
    }

    /// Returns the model default without turning it into an explicit request.
    #[must_use]
    pub const fn default_selection(self) -> ReasoningSelection {
        match self {
            Self::Toggle { default_enabled } => ReasoningSelection::Toggle(default_enabled),
            Self::Effort { default, .. } => ReasoningSelection::Effort(default),
            Self::AlwaysOn => ReasoningSelection::Toggle(true),
            Self::Unsupported | Self::Unknown => ReasoningSelection::Auto,
        }
    }

    /// Validates an explicit selection against this model's declared policy.
    #[must_use]
    pub fn accepts(self, selection: ReasoningSelection) -> bool {
        if selection == ReasoningSelection::Auto {
            return true;
        }
        match (self, selection) {
            (Self::Effort { supported, .. }, ReasoningSelection::Effort(effort)) => {
                supported.contains(&effort)
            }
            (
                Self::Effort {
                    supports_disabled: true,
                    ..
                },
                ReasoningSelection::Toggle(false),
            )
            | (Self::Toggle { .. }, ReasoningSelection::Toggle(_))
            | (Self::AlwaysOn, ReasoningSelection::Toggle(true)) => true,
            _ => false,
        }
    }

    /// Resolves a harness preference into a control supported by this model.
    #[must_use]
    pub fn resolve_hint(self, hint: ReasoningHint) -> Option<ReasoningSelection> {
        let selection = match self {
            Self::Toggle { .. } => match hint {
                ReasoningHint::Disabled => ReasoningSelection::Toggle(false),
                ReasoningHint::Low
                | ReasoningHint::Medium
                | ReasoningHint::High
                | ReasoningHint::ExtraHigh => ReasoningSelection::Toggle(true),
            },
            Self::Effort { supported, .. } => match hint {
                ReasoningHint::Disabled => ReasoningSelection::Toggle(false),
                ReasoningHint::Low => ReasoningSelection::Effort(ReasoningEffort::Low),
                ReasoningHint::Medium => ReasoningSelection::Effort(ReasoningEffort::Medium),
                ReasoningHint::High => ReasoningSelection::Effort(ReasoningEffort::High),
                ReasoningHint::ExtraHigh => {
                    ReasoningSelection::Effort(if supported.contains(&ReasoningEffort::Max) {
                        ReasoningEffort::Max
                    } else {
                        ReasoningEffort::High
                    })
                }
            },
            Self::AlwaysOn => match hint {
                ReasoningHint::Disabled => return None,
                ReasoningHint::Low
                | ReasoningHint::Medium
                | ReasoningHint::High
                | ReasoningHint::ExtraHigh => ReasoningSelection::Toggle(true),
            },
            Self::Unsupported | Self::Unknown => ReasoningSelection::Auto,
        };
        self.accepts(selection).then_some(selection)
    }
}

/// User-facing reasoning choice. `Auto` means no explicit upstream parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "kebab-case")]
pub enum ReasoningSelection {
    Auto,
    Toggle(bool),
    Effort(ReasoningEffort),
}

impl ReasoningSelection {
    /// Returns `None` only for `Auto`, preserving omission independently of a
    /// model's default value.
    #[must_use]
    pub const fn explicit_parameter(self) -> Option<ReasoningParameter> {
        match self {
            Self::Auto => None,
            Self::Toggle(enabled) => Some(ReasoningParameter::Toggle(enabled)),
            Self::Effort(effort) => Some(ReasoningParameter::Effort(effort)),
        }
    }
}

/// Concrete reasoning value suitable for bridge and catalog serialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "kebab-case")]
pub enum ReasoningParameter {
    Toggle(bool),
    Effort(ReasoningEffort),
}
