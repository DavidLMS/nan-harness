//! Payload-free native Chat action receipts and private, bounded request framing.
use super::Window;
use std::fmt::Write as _;
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ChatActionPhase {
    BeforeGuard,
    AfterGuard,
    Transport,
    PostGuard,
    Completed,
}

pub(crate) const fn failure_label(category: super::FailureCategory) -> &'static str {
    use super::FailureCategory as F;
    match category {
        F::InvalidInput => "invalid-input",
        F::Spawn => "spawn",
        F::Pipe => "pipe",
        F::Output => "output",
        F::Timeout => "timeout",
        F::NonzeroExit => "nonzero-exit",
        F::WindowChanged => "window-changed",
        F::WindowQueryRejected => "window-query-rejected",
        F::SessionUnavailable => "session-unavailable",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ChatTurnStage {
    Request,
    Window,
    Tree,
    TreeQuery,
    TreeDuplicate,
    TreeType,
    TreeLimit,
    TreePid,
    TreeFocus,
    TreeWindow,
    Mode,
    Composer,
    Focus,
    InputMismatch,
    InputInitialUnavailable,
    InputInitialNonempty,
    InputClipboardMismatch,
    InputValueMismatch,
    Control,
    Scope,
    ScopeAnchorAbsent,
    ScopeAnchorAmbiguous,
    ScopeControlAbsent,
    ScopeControlAmbiguous,
    ScopeHeadingAmbiguous,
    ScopePromptMismatch,
    Deadline,
    ActionUncertain,
    ResponseMismatch,
    Sent,
    Copied,
    RetryReady,
    Retried,
    #[cfg(target_os = "macos")]
    Completed,
}
impl ChatTurnStage {
    pub(crate) fn passive_pending(self) -> bool {
        matches!(
            self,
            Self::Scope
                | Self::ScopeAnchorAbsent
                | Self::ScopeAnchorAmbiguous
                | Self::ScopeControlAbsent
                | Self::ScopeControlAmbiguous
                | Self::ScopeHeadingAmbiguous
                | Self::ScopePromptMismatch
                | Self::Tree
                | Self::TreeQuery
        )
    }
    pub(super) fn parse(output: &str) -> Option<Self> {
        match output {
            "turn request\n" => Some(Self::Request),
            "turn window\n" => Some(Self::Window),
            "turn tree\n" => Some(Self::Tree),
            "turn tree-query\n" => Some(Self::TreeQuery),
            "turn tree-duplicate\n" => Some(Self::TreeDuplicate),
            "turn tree-type\n" => Some(Self::TreeType),
            "turn tree-limit\n" => Some(Self::TreeLimit),
            "turn tree-pid\n" => Some(Self::TreePid),
            "turn tree-focus\n" => Some(Self::TreeFocus),
            "turn tree-window\n" => Some(Self::TreeWindow),
            "turn mode\n" => Some(Self::Mode),
            "turn composer\n" => Some(Self::Composer),
            "turn focus\n" => Some(Self::Focus),
            "turn input-mismatch\n" => Some(Self::InputMismatch),
            "turn input-initial-unavailable\n" => Some(Self::InputInitialUnavailable),
            "turn input-initial-nonempty\n" => Some(Self::InputInitialNonempty),
            "turn input-clipboard-mismatch\n" => Some(Self::InputClipboardMismatch),
            "turn input-value-mismatch\n" => Some(Self::InputValueMismatch),
            "turn control\n" => Some(Self::Control),
            "turn scope\n" => Some(Self::Scope),
            "turn scope-anchor-absent\n" => Some(Self::ScopeAnchorAbsent),
            "turn scope-anchor-ambiguous\n" => Some(Self::ScopeAnchorAmbiguous),
            "turn scope-control-absent\n" => Some(Self::ScopeControlAbsent),
            "turn scope-control-ambiguous\n" => Some(Self::ScopeControlAmbiguous),
            "turn scope-heading-ambiguous\n" => Some(Self::ScopeHeadingAmbiguous),
            "turn scope-prompt-mismatch\n" => Some(Self::ScopePromptMismatch),
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
    if !matches!(
        mode,
        "input" | "input-replace-owned" | "copy" | "retry-ready" | "retry"
    ) || values
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
    fn scoped_selector_failures_remain_passive_and_payload_free() {
        for (label, stage) in [
            ("scope-anchor-absent", ChatTurnStage::ScopeAnchorAbsent),
            (
                "scope-anchor-ambiguous",
                ChatTurnStage::ScopeAnchorAmbiguous,
            ),
            ("scope-control-absent", ChatTurnStage::ScopeControlAbsent),
            (
                "scope-control-ambiguous",
                ChatTurnStage::ScopeControlAmbiguous,
            ),
            (
                "scope-heading-ambiguous",
                ChatTurnStage::ScopeHeadingAmbiguous,
            ),
            ("scope-prompt-mismatch", ChatTurnStage::ScopePromptMismatch),
        ] {
            assert_eq!(
                ChatTurnStage::parse(&format!("turn {label}\n")),
                Some(stage)
            );
            assert!(stage.passive_pending());
            assert_eq!(
                serde_json::to_string(&stage).unwrap(),
                format!("\"{label}\"")
            );
        }
        assert!(ChatTurnStage::parse("turn scope PRIVATE\n").is_none());
    }
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
    fn input_failures_remain_exact_payload_free_receipts() {
        for (name, stage) in [
            ("input-mismatch", ChatTurnStage::InputMismatch),
            (
                "input-initial-unavailable",
                ChatTurnStage::InputInitialUnavailable,
            ),
            (
                "input-initial-nonempty",
                ChatTurnStage::InputInitialNonempty,
            ),
            (
                "input-clipboard-mismatch",
                ChatTurnStage::InputClipboardMismatch,
            ),
            ("input-value-mismatch", ChatTurnStage::InputValueMismatch),
        ] {
            assert_eq!(ChatTurnStage::parse(&format!("turn {name}\n")), Some(stage));
            assert_eq!(ChatTurnStage::parse(&format!("turn {name}\nPRIVATE")), None);
        }
    }
    #[test]
    fn action_transport_diagnostics_are_closed_protocol_labels() {
        use super::super::FailureCategory as F;
        for (phase, label) in [
            (ChatActionPhase::BeforeGuard, "before-guard"),
            (ChatActionPhase::AfterGuard, "after-guard"),
            (ChatActionPhase::Transport, "transport"),
            (ChatActionPhase::PostGuard, "post-guard"),
            (ChatActionPhase::Completed, "completed"),
        ] {
            assert_eq!(serde_json::to_value(phase).unwrap(), label);
        }
        for (failure, label) in [
            (F::InvalidInput, "invalid-input"),
            (F::Spawn, "spawn"),
            (F::Pipe, "pipe"),
            (F::Output, "output"),
            (F::Timeout, "timeout"),
            (F::NonzeroExit, "nonzero-exit"),
            (F::WindowChanged, "window-changed"),
            (F::WindowQueryRejected, "window-query-rejected"),
            (F::SessionUnavailable, "session-unavailable"),
        ] {
            assert_eq!(failure_label(failure), label);
        }
    }
    #[test]
    fn only_passive_pre_action_tree_queries_can_reacquire() {
        fn observed(sequence: &[ChatTurnStage]) -> (usize, ChatTurnStage) {
            for (index, stage) in sequence.iter().copied().enumerate() {
                if !stage.passive_pending() {
                    return (index + 1, stage);
                }
            }
            panic!("fixture must end with a receipt");
        }
        assert_eq!(
            observed(&[
                ChatTurnStage::TreeQuery,
                ChatTurnStage::Scope,
                ChatTurnStage::Copied
            ]),
            (3, ChatTurnStage::Copied),
        );
        for terminal in [
            ChatTurnStage::TreePid,
            ChatTurnStage::TreeWindow,
            ChatTurnStage::TreeFocus,
            ChatTurnStage::TreeType,
            ChatTurnStage::TreeDuplicate,
            ChatTurnStage::TreeLimit,
            ChatTurnStage::ActionUncertain,
            ChatTurnStage::ResponseMismatch,
            ChatTurnStage::Deadline,
            ChatTurnStage::Sent,
            ChatTurnStage::Retried,
        ] {
            assert_eq!(observed(&[terminal, ChatTurnStage::Copied]), (1, terminal));
        }
        for name in [
            "tree-query",
            "tree-duplicate",
            "tree-type",
            "tree-limit",
            "tree-pid",
            "tree-focus",
            "tree-window",
        ] {
            assert!(ChatTurnStage::parse(&format!("turn {name}\n")).is_some());
            assert!(ChatTurnStage::parse(&format!("turn {name}\nPRIVATE")).is_none());
        }
        assert!(ChatTurnStage::parse("turn tree-private-details\n").is_none());
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
        let replacement = request(
            &window,
            "input-replace-owned",
            ["fresh\nprompt", "", "sentinel"],
            500,
            1000,
            9,
        )
        .unwrap();
        assert!(replacement.starts_with("input-replace-owned "));
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
