//! Payload-free native Chat action receipts and private, bounded request framing.
use super::Window;
use std::fmt::Write as _;
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ChatTurnStage {
    Request,
    Window,
    Tree,
    Mode,
    Composer,
    Focus,
    InputMismatch,
    Control,
    Scope,
    Deadline,
    ActionUncertain,
    ResponseMismatch,
    Sent,
    Copied,
    RetryReady,
    Retried,
    Completed,
}
impl ChatTurnStage {
    pub(super) fn parse(output: &str) -> Option<Self> {
        match output {
            "turn request\n" => Some(Self::Request),
            "turn window\n" => Some(Self::Window),
            "turn tree\n" => Some(Self::Tree),
            "turn mode\n" => Some(Self::Mode),
            "turn composer\n" => Some(Self::Composer),
            "turn focus\n" => Some(Self::Focus),
            "turn input-mismatch\n" => Some(Self::InputMismatch),
            "turn control\n" => Some(Self::Control),
            "turn scope\n" => Some(Self::Scope),
            "turn deadline\n" => Some(Self::Deadline),
            "turn action-uncertain\n" => Some(Self::ActionUncertain),
            "turn response-mismatch\n" => Some(Self::ResponseMismatch),
            "turn sent\n" => Some(Self::Sent),
            "turn copied\n" => Some(Self::Copied),
            "turn retry-ready\n" => Some(Self::RetryReady),
            "turn retried\n" => Some(Self::Retried),
            _ => None,
        }
    }
}
fn hex(value: &str) -> Zeroizing<String> {
    if value.is_empty() {
        return Zeroizing::new("-".into());
    }
    let mut output = Zeroizing::new(String::with_capacity(value.len() * 2));
    for byte in value.bytes() {
        write!(output, "{byte:02x}").expect("writing to a string cannot fail");
    }
    output
}
pub(super) fn request(
    window: &Window,
    mode: &str,
    values: [&str; 3],
    millis: u32,
    cutoff: u64,
    owner: u32,
) -> Option<Zeroizing<String>> {
    if !matches!(mode, "input" | "copy" | "retry-ready" | "retry")
        || values
            .iter()
            .any(|value| value.len() > 1024 || value.contains('\0'))
        || cutoff == 0
        || owner < 2
        || millis == 0
        || millis > 5000
    {
        return None;
    }
    let bounds = window.bounds;
    let [prompt, marker, sentinel] = values.map(hex);
    Some(Zeroizing::new(format!(
        "{mode} {} {} {} {} {} {} {millis} {cutoff} {owner} {} {} {}",
        window.id,
        window.pid,
        bounds.x,
        bounds.y,
        bounds.width,
        bounds.height,
        prompt.as_str(),
        marker.as_str(),
        sentinel.as_str()
    )))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn receipts_cannot_certify_output_or_actions_from_partial_or_payload_text() {
        assert_eq!(
            ChatTurnStage::parse("turn sent\n"),
            Some(ChatTurnStage::Sent)
        );
        assert_eq!(
            ChatTurnStage::parse("turn action-uncertain\n"),
            Some(ChatTurnStage::ActionUncertain)
        );
        for invalid in [
            "turn sent",
            "turn copied\nPRIVATE",
            "turn arbitrary\n",
            "PRIVATE",
        ] {
            assert_eq!(ChatTurnStage::parse(invalid), None);
        }
    }
    #[test]
    fn action_request_is_one_private_frame_bound_to_owner_and_cutoff() {
        let window = Window {
            id: 7,
            pid: 8,
            bounds: xa11y::Rect {
                x: 0,
                y: 0,
                width: 800,
                height: 600,
            },
            name: String::new(),
            layer: 0,
        };
        let frame = request(
            &window,
            "input",
            ["fresh\nprompt", "", "sentinel"],
            500,
            1000,
            9,
        )
        .unwrap();
        assert!(!frame.contains('\n'));
        assert!(frame.contains(" 500 1000 9 "));
        assert!(!frame.contains("fresh"));
        assert!(request(&window, "input", ["prompt", "", "sentinel"], 500, 0, 9).is_none());
        assert!(request(&window, "input", ["prompt", "", "sentinel"], 500, 1000, 1).is_none());
        assert!(request(&window, "unknown", ["prompt", "", "sentinel"], 500, 1000, 9).is_none());
    }
    #[test]
    fn private_text_is_hex_framed_without_newlines_or_command_arguments() {
        assert_eq!(hex("fresh\nprompt").as_str(), "66726573680a70726f6d7074");
        assert_eq!(hex("").as_str(), "-");
    }
}
