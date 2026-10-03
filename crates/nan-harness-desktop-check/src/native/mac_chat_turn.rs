//! Payload-free native Chat action receipts and private, bounded request framing.
use super::Window;
use std::fmt::Write as _;
use zeroize::Zeroizing;

pub(crate) const CHAT_TURN_MAX_MILLIS: u32 = 15_000;

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
    InputFocusGuard,
    InputFocusSetting,
    InputFocusedIdentity,
    InputReplaceSelectKey,
    InputPromptBeforeGuard,
    InputPromptClipboard,
    InputPromptAfterGuard,
    InputPasteKey,
    InputReadbackBeforeGuard,
    InputSentinelClipboard,
    InputSentinelAfterGuard,
    InputReadbackSelectKey,
    InputReadbackSelectGuard,
    InputReadbackCopyKey,
    InputCollapseGuard,
    InputCollapseKey,
    InputMismatch,
    InputInitialUnavailable,
    InputInitialNonempty,
    InputClipboardMismatch,
    InputValueMismatch,
    Control,
    Scope,
    ScopeAnchorAbsent,
    ScopeHeadingAbsent,
    ScopeAssistantHeadingAbsent,
    ScopeMarkerHeadingAbsent,
    ScopeAnchorAmbiguous,
    ScopeControlAbsent,
    ScopeControlAmbiguous,
    ScopeHeadingAmbiguous,
    ScopePromptMismatch,
    Deadline,
    DeadlineWindow,
    DeadlineTree,
    DeadlineFocus,
    DeadlineInput,
    DeadlineInputPaste,
    DeadlineInputReadback,
    DeadlinePress,
    DeadlineCopy,
    DeadlineRetryReady,
    DeadlineRetry,
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
                | Self::ScopeHeadingAbsent
                | Self::ScopeAssistantHeadingAbsent
                | Self::ScopeMarkerHeadingAbsent
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
            "turn input-focus-guard\n" => Some(Self::InputFocusGuard),
            "turn input-focus-setting\n" => Some(Self::InputFocusSetting),
            "turn input-focused-identity\n" => Some(Self::InputFocusedIdentity),
            "turn input-replace-select-key\n" => Some(Self::InputReplaceSelectKey),
            "turn input-prompt-before-guard\n" => Some(Self::InputPromptBeforeGuard),
            "turn input-prompt-clipboard\n" => Some(Self::InputPromptClipboard),
            "turn input-prompt-after-guard\n" => Some(Self::InputPromptAfterGuard),
            "turn input-paste-key\n" => Some(Self::InputPasteKey),
            "turn input-readback-before-guard\n" => Some(Self::InputReadbackBeforeGuard),
            "turn input-sentinel-clipboard\n" => Some(Self::InputSentinelClipboard),
            "turn input-sentinel-after-guard\n" => Some(Self::InputSentinelAfterGuard),
            "turn input-readback-select-key\n" => Some(Self::InputReadbackSelectKey),
            "turn input-readback-select-guard\n" => Some(Self::InputReadbackSelectGuard),
            "turn input-readback-copy-key\n" => Some(Self::InputReadbackCopyKey),
            "turn input-collapse-guard\n" => Some(Self::InputCollapseGuard),
            "turn input-collapse-key\n" => Some(Self::InputCollapseKey),

            "turn input-mismatch\n" => Some(Self::InputMismatch),
            "turn input-initial-unavailable\n" => Some(Self::InputInitialUnavailable),
            "turn input-initial-nonempty\n" => Some(Self::InputInitialNonempty),
            "turn input-clipboard-mismatch\n" => Some(Self::InputClipboardMismatch),
            "turn input-value-mismatch\n" => Some(Self::InputValueMismatch),
            "turn control\n" => Some(Self::Control),
            "turn scope\n" => Some(Self::Scope),
            "turn scope-anchor-absent\n" => Some(Self::ScopeAnchorAbsent),
            "turn scope-heading-absent\n" => Some(Self::ScopeHeadingAbsent),
            "turn scope-assistant-heading-absent\n" => Some(Self::ScopeAssistantHeadingAbsent),
            "turn scope-marker-heading-absent\n" => Some(Self::ScopeMarkerHeadingAbsent),
            "turn scope-anchor-ambiguous\n" => Some(Self::ScopeAnchorAmbiguous),
            "turn scope-control-absent\n" => Some(Self::ScopeControlAbsent),
            "turn scope-control-ambiguous\n" => Some(Self::ScopeControlAmbiguous),
            "turn scope-heading-ambiguous\n" => Some(Self::ScopeHeadingAmbiguous),
            "turn scope-prompt-mismatch\n" => Some(Self::ScopePromptMismatch),
            "turn deadline\n" => Some(Self::Deadline),
            "turn deadline-window\n" => Some(Self::DeadlineWindow),
            "turn deadline-tree\n" => Some(Self::DeadlineTree),
            "turn deadline-focus\n" => Some(Self::DeadlineFocus),
            "turn deadline-input\n" => Some(Self::DeadlineInput),
            "turn deadline-input-paste\n" => Some(Self::DeadlineInputPaste),
            "turn deadline-input-readback\n" => Some(Self::DeadlineInputReadback),
            "turn deadline-press\n" => Some(Self::DeadlinePress),
            "turn deadline-copy\n" => Some(Self::DeadlineCopy),
            "turn deadline-retry-ready\n" => Some(Self::DeadlineRetryReady),
            "turn deadline-retry\n" => Some(Self::DeadlineRetry),
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

pub(super) fn helper_millis(millis: u32) -> Result<u32, super::FailureCategory> {
    // Reserve transport teardown time without misclassifying an exhausted
    // polling budget as a malformed private request.
    millis
        .checked_sub(50)
        .filter(|value| *value > 0)
        .ok_or(super::FailureCategory::Timeout)
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
        || millis > CHAT_TURN_MAX_MILLIS
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
    fn deadline_phases_are_terminal_closed_and_legacy_deadline_remains_valid() {
        for phase in [
            "deadline",
            "deadline-window",
            "deadline-tree",
            "deadline-focus",
            "deadline-input",
            "deadline-input-paste",
            "deadline-input-readback",
            "deadline-press",
            "deadline-copy",
            "deadline-retry-ready",
            "deadline-retry",
        ] {
            let stage = ChatTurnStage::parse(&format!("turn {phase}\n")).unwrap();
            assert!(!stage.passive_pending());
            assert_eq!(serde_json::to_value(stage).unwrap(), phase);
            assert!(ChatTurnStage::parse(&format!("turn {phase} PRIVATE\n")).is_none());
        }
        assert!(ChatTurnStage::parse("turn deadline-private\n").is_none());
    }
    #[test]
    fn input_boundary_failures_are_closed_terminal_receipts() {
        for (label, expected) in [
            ("input-focus-guard", ChatTurnStage::InputFocusGuard),
            ("input-focus-setting", ChatTurnStage::InputFocusSetting),
            (
                "input-focused-identity",
                ChatTurnStage::InputFocusedIdentity,
            ),
            (
                "input-replace-select-key",
                ChatTurnStage::InputReplaceSelectKey,
            ),
            (
                "input-prompt-before-guard",
                ChatTurnStage::InputPromptBeforeGuard,
            ),
            (
                "input-prompt-clipboard",
                ChatTurnStage::InputPromptClipboard,
            ),
            (
                "input-prompt-after-guard",
                ChatTurnStage::InputPromptAfterGuard,
            ),
            ("input-paste-key", ChatTurnStage::InputPasteKey),
            (
                "input-readback-before-guard",
                ChatTurnStage::InputReadbackBeforeGuard,
            ),
            (
                "input-sentinel-clipboard",
                ChatTurnStage::InputSentinelClipboard,
            ),
            (
                "input-sentinel-after-guard",
                ChatTurnStage::InputSentinelAfterGuard,
            ),
            (
                "input-readback-select-key",
                ChatTurnStage::InputReadbackSelectKey,
            ),
            (
                "input-readback-select-guard",
                ChatTurnStage::InputReadbackSelectGuard,
            ),
            (
                "input-readback-copy-key",
                ChatTurnStage::InputReadbackCopyKey,
            ),
            ("input-collapse-guard", ChatTurnStage::InputCollapseGuard),
            ("input-collapse-key", ChatTurnStage::InputCollapseKey),
        ] {
            let receipt = format!("turn {label}\n");
            assert_eq!(ChatTurnStage::parse(&receipt), Some(expected));
            assert!(!expected.passive_pending());
            assert_eq!(ChatTurnStage::parse(&format!("{receipt}PRIVATE")), None);
            assert_eq!(serde_json::to_value(expected).unwrap(), label);
        }
        assert_eq!(
            ChatTurnStage::parse("turn focus\n"),
            Some(ChatTurnStage::Focus)
        );
        assert_eq!(ChatTurnStage::parse("turn input-private-detail\n"), None);
    }

    #[test]
    fn short_action_budgets_expire_before_a_helper_request() {
        for millis in [0, 1, 49, 50] {
            assert_eq!(
                helper_millis(millis),
                Err(super::super::FailureCategory::Timeout)
            );
        }
        assert_eq!(helper_millis(51), Ok(1));
        assert_eq!(helper_millis(5000), Ok(4950));
        assert_eq!(helper_millis(CHAT_TURN_MAX_MILLIS), Ok(14_950));
    }

    #[test]
    fn scoped_selector_failures_remain_passive_and_payload_free() {
        for (label, stage) in [
            ("scope-anchor-absent", ChatTurnStage::ScopeAnchorAbsent),
            ("scope-heading-absent", ChatTurnStage::ScopeHeadingAbsent),
            (
                "scope-assistant-heading-absent",
                ChatTurnStage::ScopeAssistantHeadingAbsent,
            ),
            (
                "scope-marker-heading-absent",
                ChatTurnStage::ScopeMarkerHeadingAbsent,
            ),
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
        for millis in [1, 5000, CHAT_TURN_MAX_MILLIS] {
            let framed = request(
                &window,
                "input",
                ["prompt", "", "sentinel"],
                millis,
                1000,
                9,
            )
            .unwrap();
            assert!(framed.contains(&format!(" {millis} 1000 9 ")));
        }
        for millis in [0, CHAT_TURN_MAX_MILLIS + 1, u32::MAX] {
            assert!(
                request(
                    &window,
                    "input",
                    ["prompt", "", "sentinel"],
                    millis,
                    1000,
                    9
                )
                .is_none()
            );
        }
    }
    #[test]
    fn private_text_is_hex_framed_without_newlines_or_command_arguments() {
        assert_eq!(hex("fresh\nprompt").as_str(), "66726573680a70726f6d7074");
        assert_eq!(hex("").as_str(), "-");
    }
}
