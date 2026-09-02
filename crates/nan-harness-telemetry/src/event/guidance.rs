use serde::{Deserialize, Serialize};

pub const REOPEN_TERMINAL_GUIDANCE_TEXT: &str = "The current terminal session cannot access the project directory. Please close this terminal, open a new terminal in the project directory, and try again.";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum GuidanceClassification {
    Environmental,
}

impl GuidanceClassification {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Environmental => "environmental",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UserGuidance {
    classification: GuidanceClassification,
    id: String,
    shown: bool,
    locale: String,
    version: u8,
    text: String,
}

impl UserGuidance {
    #[must_use]
    pub fn reopen_terminal(shown: bool) -> Self {
        Self {
            classification: GuidanceClassification::Environmental,
            id: "reopen-terminal".to_owned(),
            shown,
            locale: "en".to_owned(),
            version: 1,
            text: REOPEN_TERMINAL_GUIDANCE_TEXT.to_owned(),
        }
    }

    #[must_use]
    pub const fn classification(&self) -> GuidanceClassification {
        self.classification
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub const fn shown(&self) -> bool {
        self.shown
    }

    #[must_use]
    pub fn locale(&self) -> &str {
        &self.locale
    }

    #[must_use]
    pub const fn version(&self) -> u8 {
        self.version
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    pub(crate) fn is_approved(&self) -> bool {
        self.classification == GuidanceClassification::Environmental
            && self.id == "reopen-terminal"
            && self.locale == "en"
            && self.version == 1
            && self.text == REOPEN_TERMINAL_GUIDANCE_TEXT
    }
}
