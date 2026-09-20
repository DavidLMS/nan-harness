use serde::{Deserialize, Serialize};

/// Native media capabilities selected for one harness launch or configuration.
///
/// Each capability is independent so an existing provider can be preserved while
/// NaN is enabled for another capability.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaSelection {
    pub stt: bool,
    pub tts: bool,
    pub image: bool,
}

impl MediaSelection {
    #[must_use]
    pub const fn none() -> Self {
        Self {
            stt: false,
            tts: false,
            image: false,
        }
    }

    #[must_use]
    pub const fn all() -> Self {
        Self {
            stt: true,
            tts: true,
            image: true,
        }
    }

    #[must_use]
    pub const fn any(self) -> bool {
        self.stt || self.tts || self.image
    }
}
