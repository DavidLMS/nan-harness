use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TimeoutPhase {
    InitialResponse,
    Inactivity,
    CoordinatorQueue,
}

impl TimeoutPhase {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InitialResponse => "initial-response",
            Self::Inactivity => "inactivity",
            Self::CoordinatorQueue => "coordinator-queue",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RecoveryOutcome {
    Retrying,
    Delegated,
    Exhausted,
}

impl RecoveryOutcome {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Retrying => "retrying",
            Self::Delegated => "delegated",
            Self::Exhausted => "exhausted",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AttemptBucket {
    First,
    Second,
    Later,
}

impl AttemptBucket {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::First => "first",
            Self::Second => "second",
            Self::Later => "later",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RequestPriority {
    Foreground,
    Background,
}

impl RequestPriority {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Foreground => "foreground",
            Self::Background => "background",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum BridgeEndpoint {
    Models,
    Messages,
    CountTokens,
    Responses,
    Search,
    FxGateway,
}

impl BridgeEndpoint {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Models => "models",
            Self::Messages => "messages",
            Self::CountTokens => "count-tokens",
            Self::Responses => "responses",
            Self::Search => "search",
            Self::FxGateway => "fx-gateway",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ReasoningRequest {
    Auto,
    None,
    Low,
    Medium,
    High,
    Xhigh,
    Other,
}

impl ReasoningRequest {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::None => "none",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ModelPolicy {
    Unsupported,
    Toggle,
    Effort,
    AlwaysOn,
    Unknown,
}

impl ModelPolicy {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unsupported => "unsupported",
            Self::Toggle => "toggle",
            Self::Effort => "effort",
            Self::AlwaysOn => "always-on",
            Self::Unknown => "unknown",
        }
    }
}
