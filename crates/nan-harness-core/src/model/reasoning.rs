use serde::{Deserialize, Serialize};

/// Reasoning effort values accepted by models with an effort-based policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    Low,
    Medium,
    High,
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
        supported: [ReasoningEffort; 3],
        default: ReasoningEffort,
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
            (Self::Toggle { .. }, ReasoningSelection::Toggle(_))
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
            Self::Effort { .. } => match hint {
                ReasoningHint::Disabled => return None,
                ReasoningHint::Low => ReasoningSelection::Effort(ReasoningEffort::Low),
                ReasoningHint::Medium => ReasoningSelection::Effort(ReasoningEffort::Medium),
                ReasoningHint::High | ReasoningHint::ExtraHigh => {
                    ReasoningSelection::Effort(ReasoningEffort::High)
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
