use crate::WebSearchPolicy;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;
use thiserror::Error;

/// Experimental GUI surfaces managed separately from the stable harness set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DesktopHarnessKind {
    #[serde(rename = "chatgpt-desktop")]
    ChatGpt,
    #[serde(rename = "claude-desktop")]
    Claude,
    #[serde(rename = "hermes-desktop")]
    Hermes,
    #[serde(rename = "pen-desktop")]
    Pen,
    #[serde(rename = "zed-desktop")]
    Zed,
}

impl DesktopHarnessKind {
    pub const ALL: [Self; 5] = [
        Self::ChatGpt,
        Self::Claude,
        Self::Hermes,
        Self::Pen,
        Self::Zed,
    ];

    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::ChatGpt => "ChatGPT Desktop",
            Self::Claude => "Claude Desktop",
            Self::Hermes => "Hermes Desktop",
            Self::Pen => "Pen Desktop",
            Self::Zed => "Zed",
        }
    }
}

impl fmt::Display for DesktopHarnessKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ChatGpt => "chatgpt-desktop",
            Self::Claude => "claude-desktop",
            Self::Hermes => "hermes-desktop",
            Self::Pen => "pen-desktop",
            Self::Zed => "zed-desktop",
        })
    }
}

impl FromStr for DesktopHarnessKind {
    type Err = ParseDesktopHarnessKindError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "chatgpt-desktop" | "codex-desktop" => Ok(Self::ChatGpt),
            "claude-desktop" => Ok(Self::Claude),
            "hermes-desktop" => Ok(Self::Hermes),
            "pen" | "pen-desktop" => Ok(Self::Pen),
            "zed" | "zed-desktop" => Ok(Self::Zed),
            _ => Err(ParseDesktopHarnessKindError(value.to_owned())),
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
#[error("unknown experimental desktop harness '{0}'")]
pub struct ParseDesktopHarnessKindError(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DesktopTransport {
    ResponsesBridge,
    AnthropicBridge,
    ChatCompletionsGateway,
    DirectChatCompletions,
}

impl fmt::Display for DesktopTransport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ResponsesBridge => "responses-bridge",
            Self::AnthropicBridge => "anthropic-bridge",
            Self::ChatCompletionsGateway => "chat-completions-gateway",
            Self::DirectChatCompletions => "direct-chat-completions",
        })
    }
}

/// A serializable, credential-free description of a prospective Desktop launch.
///
/// Constructing or serializing this value is inert: it does not discover models,
/// read credentials, write a profile, bind a socket, or start a process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
// These independent capabilities are serialized as booleans so dry-run output
// remains straightforward for both humans and automation.
#[allow(clippy::struct_excessive_bools)]
pub struct DesktopLaunchPlan {
    pub schema_version: u8,
    pub harness: DesktopHarnessKind,
    pub experimental: bool,
    pub platform: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auxiliary_model: Option<String>,
    pub transport: DesktopTransport,
    pub web_search_policy: WebSearchPolicy,
    pub persistent_profile: bool,
    pub private_diagnostics: bool,
    pub restore_only: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub native_arguments: Vec<String>,
}

impl DesktopLaunchPlan {
    pub const SCHEMA_VERSION: u8 = 1;

    #[must_use]
    pub fn new(harness: DesktopHarnessKind, transport: DesktopTransport) -> Self {
        Self {
            schema_version: Self::SCHEMA_VERSION,
            harness,
            experimental: true,
            platform: std::env::consts::OS.to_owned(),
            executable: None,
            selected_model: None,
            auxiliary_model: None,
            transport,
            web_search_policy: WebSearchPolicy::Auto,
            persistent_profile: false,
            private_diagnostics: false,
            restore_only: false,
            native_arguments: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DesktopHarnessKind, DesktopLaunchPlan, DesktopTransport};
    use crate::HarnessKind;
    use std::str::FromStr as _;

    #[test]
    fn desktop_registry_is_separate_and_aliases_are_typed() {
        assert_eq!(DesktopHarnessKind::ALL.len(), 5);
        assert_eq!(
            DesktopHarnessKind::from_str("codex-desktop"),
            Ok(DesktopHarnessKind::ChatGpt)
        );
        assert_eq!(
            DesktopHarnessKind::from_str("pen"),
            Ok(DesktopHarnessKind::Pen)
        );
        assert_eq!(
            DesktopHarnessKind::from_str("zed"),
            Ok(DesktopHarnessKind::Zed)
        );
        assert_eq!(HarnessKind::ALL.len(), 15);
        assert!(HarnessKind::from_str("zed").is_err());
    }

    #[test]
    fn desktop_launch_plan_is_safe_and_serializable() {
        let plan = DesktopLaunchPlan::new(
            DesktopHarnessKind::Claude,
            DesktopTransport::AnthropicBridge,
        );
        let json = serde_json::to_value(plan).expect("Desktop plan should serialize");
        assert_eq!(json["schemaVersion"], 1);
        assert_eq!(json["experimental"], true);
        assert!(json.get("credential").is_none());
    }
}
