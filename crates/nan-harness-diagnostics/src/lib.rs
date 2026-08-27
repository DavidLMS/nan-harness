#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum MessageLevel {
    Warning,
    SetupRequired,
    Error,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ReportPolicy {
    Never,
    ConsentAware,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecoveryAction {
    pub title: String,
    #[serde(default)]
    pub commands: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl RecoveryAction {
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            commands: Vec::new(),
            detail: None,
        }
    }

    #[must_use]
    pub fn with_commands(mut self, commands: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.commands = commands.into_iter().map(Into::into).collect();
        self
    }

    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UserMessage {
    pub level: MessageLevel,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub summary: String,
    #[serde(default)]
    pub actions: Vec<RecoveryAction>,
    pub report_policy: ReportPolicy,
}

impl UserMessage {
    #[must_use]
    pub fn warning(summary: impl Into<String>) -> Self {
        Self::new(MessageLevel::Warning, None, summary, ReportPolicy::Never)
    }

    #[must_use]
    pub fn reportable_warning(summary: impl Into<String>) -> Self {
        Self::new(
            MessageLevel::Warning,
            None,
            summary,
            ReportPolicy::ConsentAware,
        )
    }

    #[must_use]
    pub fn setup_required(summary: impl Into<String>) -> Self {
        Self::new(
            MessageLevel::SetupRequired,
            None,
            summary,
            ReportPolicy::Never,
        )
    }

    #[must_use]
    pub fn error(code: impl Into<String>, summary: impl Into<String>) -> Self {
        Self::new(
            MessageLevel::Error,
            Some(code.into()),
            summary,
            ReportPolicy::ConsentAware,
        )
    }

    fn new(
        level: MessageLevel,
        code: Option<String>,
        summary: impl Into<String>,
        report_policy: ReportPolicy,
    ) -> Self {
        Self {
            level,
            code,
            summary: summary.into(),
            actions: Vec::new(),
            report_policy,
        }
    }

    #[must_use]
    pub fn with_action(mut self, action: RecoveryAction) -> Self {
        self.actions.push(action);
        self
    }

    #[must_use]
    pub fn is_reportable(&self) -> bool {
        self.report_policy == ReportPolicy::ConsentAware
    }

    #[must_use]
    pub fn render_terminal(&self) -> String {
        let mut rendered = String::new();
        match (self.level, self.code.as_deref()) {
            (MessageLevel::Warning, _) => rendered.push_str("warning: "),
            (MessageLevel::SetupRequired, _) => rendered.push_str("setup required: "),
            (MessageLevel::Error, Some(code)) => {
                let _ = write!(rendered, "error [{code}]: ");
            }
            (MessageLevel::Error, None) => rendered.push_str("error: "),
        }
        rendered.push_str(&self.summary);

        for action in &self.actions {
            rendered.push_str("\n\n");
            rendered.push_str(&action.title);
            for command in &action.commands {
                rendered.push_str("\n  ");
                rendered.push_str(command);
            }
            if let Some(detail) = &action.detail {
                rendered.push_str("\n\n");
                rendered.push_str(detail);
            }
        }
        rendered
    }
}

#[cfg(test)]
mod tests {
    use super::{RecoveryAction, ReportPolicy, UserMessage};

    #[test]
    fn setup_guidance_has_no_error_code_or_report_policy() {
        let message = UserMessage::setup_required(
            "DeepSeek Harness requires Node.js >= 22.19.0, but detected Node.js v20.19.4.",
        )
        .with_action(
            RecoveryAction::new("Recommended fix with nvm:").with_commands([
                "nvm install 22",
                "nvm use 22",
                "nan dsh",
            ]),
        );

        assert_eq!(message.report_policy, ReportPolicy::Never);
        assert!(!message.is_reportable());
        assert_eq!(
            message.render_terminal(),
            concat!(
                "setup required: DeepSeek Harness requires Node.js >= 22.19.0, ",
                "but detected Node.js v20.19.4.\n\n",
                "Recommended fix with nvm:\n",
                "  nvm install 22\n",
                "  nvm use 22\n",
                "  nan dsh"
            )
        );
    }

    #[test]
    fn nan_harness_errors_keep_their_code_and_are_reportable() {
        let message = UserMessage::error("NH-BRIDGE-102", "the bridge rejected a request");

        assert!(message.is_reportable());
        assert_eq!(
            message.render_terminal(),
            "error [NH-BRIDGE-102]: the bridge rejected a request"
        );
    }

    #[test]
    fn reportable_warnings_hide_error_codes_but_keep_telemetry_enabled() {
        let message =
            UserMessage::reportable_warning("The terminal session needs to be restarted.");

        assert!(message.is_reportable());
        assert_eq!(
            message.render_terminal(),
            "warning: The terminal session needs to be restarted."
        );
    }
}
