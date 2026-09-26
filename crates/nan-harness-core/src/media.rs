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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_model: Option<ImageModel>,
}

impl MediaSelection {
    #[must_use]
    pub const fn none() -> Self {
        Self {
            stt: false,
            tts: false,
            image: false,
            image_model: None,
        }
    }

    #[must_use]
    pub const fn all() -> Self {
        Self {
            stt: true,
            tts: true,
            image: true,
            image_model: None,
        }
    }

    #[must_use]
    pub const fn any(self) -> bool {
        self.stt || self.tts || self.image
    }
}

/// Image models supported by the native NaN media providers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImageModel {
    #[default]
    #[serde(rename = "flux-2-klein")]
    Flux2Klein,
    #[serde(rename = "qwen-image-2.1")]
    QwenImage21,
}

impl ImageModel {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Flux2Klein => "flux-2-klein",
            Self::QwenImage21 => "qwen-image-2.1",
        }
    }

    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "flux-2-klein" => Some(Self::Flux2Klein),
            "qwen-image-2.1" => Some(Self::QwenImage21),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ImageModel, MediaSelection};

    #[test]
    fn previous_media_receipts_keep_the_flux_default() {
        let media: MediaSelection =
            serde_json::from_str(r#"{"stt":false,"tts":false,"image":true}"#)
                .expect("legacy receipt");
        assert_eq!(
            media.image_model.unwrap_or_default(),
            ImageModel::Flux2Klein
        );
        let selected = MediaSelection {
            image_model: Some(ImageModel::QwenImage21),
            ..media
        };
        let encoded = serde_json::to_value(selected).expect("receipt");
        assert_eq!(encoded["imageModel"], "qwen-image-2.1");
        assert_eq!(
            serde_json::from_value::<MediaSelection>(encoded).expect("read receipt"),
            selected
        );
    }
}
