//! Closed native receipt for one guarded source-backed Chat press.

#[derive(Clone, Copy, Debug, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ChatPressStage {
    Request,
    InitialProof,
    WindowBounds,
    Tree,
    Mode,
    Chat,
    ControlRecheck,
    HitTest,
    Deadline,
    PressUncertain,
    Completed,
}
impl ChatPressStage {
    #[cfg(any(target_os = "macos", test))]
    pub(super) fn parse(output: &str) -> Option<Self> {
        match output {
            "chat request\n" => Some(Self::Request),
            "chat initial-proof\n" => Some(Self::InitialProof),
            "chat window-bounds\n" => Some(Self::WindowBounds),
            "chat tree\n" => Some(Self::Tree),
            "chat mode\n" => Some(Self::Mode),
            "chat chat\n" => Some(Self::Chat),
            "chat control-recheck\n" => Some(Self::ControlRecheck),
            "chat hit-test\n" => Some(Self::HitTest),
            "chat deadline\n" => Some(Self::Deadline),
            "chat press-uncertain\n" => Some(Self::PressUncertain),
            "chat completed\n" => Some(Self::Completed),
            _ => None,
        }
    }
    pub(crate) fn attempted(self) -> bool {
        matches!(self, Self::Completed | Self::PressUncertain)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_completed_or_uncertain_native_receipts_indicate_an_action() {
        for stage in [
            ChatPressStage::Request,
            ChatPressStage::Tree,
            ChatPressStage::ControlRecheck,
            ChatPressStage::Deadline,
        ] {
            assert!(!stage.attempted());
        }
        assert!(ChatPressStage::Completed.attempted());
        assert!(ChatPressStage::PressUncertain.attempted());
        assert_eq!(
            ChatPressStage::parse("chat completed\n"),
            Some(ChatPressStage::Completed)
        );
        for invalid in [
            "",
            "chat completed",
            "chat completed\nPRIVATE",
            "chat PRIVATE\n",
        ] {
            assert!(ChatPressStage::parse(invalid).is_none());
        }
    }
}
