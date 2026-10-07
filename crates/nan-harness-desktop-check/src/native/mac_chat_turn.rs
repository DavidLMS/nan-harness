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
    InputPasteUnsettled,
    InputPasteFocusBefore,
    InputPasteValueUnavailable,
    InputPasteQueryFailed,
    InputPasteFocusAfter,
    InputPasteUnexpectedValue,
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
    FailureDetailsReady,
    FailureDetailsOpened,
    Retried,
    #[cfg(target_os = "macos")]
    Completed,
}
impl ChatTurnStage {
    pub(crate) fn disclosure_ready(self) -> Option<bool> {
        if self == Self::FailureDetailsReady {
            Some(true)
        } else if self.pre_action_pending() {
            Some(false)
        } else {
            None
        }
    }
    // Control is returned only before a press. Passive callers may measure again;
    // a completed or uncertain action never grants another attempt.
    pub(crate) fn pre_action_pending(self) -> bool {
        self == Self::Control || self.passive_pending()
    }
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
            "turn input-paste-unsettled\n" => Some(Self::InputPasteUnsettled),
            "turn input-paste-focus-before\n" => Some(Self::InputPasteFocusBefore),
            "turn input-paste-value-unavailable\n" => Some(Self::InputPasteValueUnavailable),
            "turn input-paste-query-failed\n" => Some(Self::InputPasteQueryFailed),
            "turn input-paste-focus-after\n" => Some(Self::InputPasteFocusAfter),
            "turn input-paste-unexpected-value\n" => Some(Self::InputPasteUnexpectedValue),
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
            "turn failure-details-ready\n" => Some(Self::FailureDetailsReady),
            "turn failure-details-opened\n" => Some(Self::FailureDetailsOpened),
            "turn retried\n" => Some(Self::Retried),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FailureRowCounts {
    source_rows: u16,
    streaming_rows: u16,
    exact_user_headings: u16,
    exact_prompt_nodes: u16,
    server_error_labels: u16,
    retry_controls: u16,
    details_controls: u16,
    user_rows: u16,
    error_rows: u16,
    shared_parent_pairs: u16,
    adjacent_pairs: u16,
    assistant_headings_in_error_rows: u16,
    duplicate_positions: u16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FailureRowShape {
    source_version: &'static str,
    source_sha256: &'static str,
    navigation_source_sha256: &'static str,
    phase: &'static str,
    counts: FailureRowCounts,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FailureScopeShape {
    parent_kind: &'static str,
    walk_end: &'static str,
    group_ancestor_count: u16,
    source_row_labels_any_role: u16,
    streaming_labels_any_role: u16,
    try_again_labels_any_role: u16,
    try_again_buttons: u16,
    view_details_labels_any_role: u16,
    view_details_buttons: u16,
}
impl FailureScopeShape {
    fn parse(line: &str) -> Option<Self> {
        let fields: Vec<_> = line.strip_prefix("scope ")?.split(' ').collect();
        if fields.len() != 9 {
            return None;
        }
        let parent_kind = match fields[0] {
            "none" => "none",
            "group" => "group",
            "web-area" => "web-area",
            "scroll-area" => "scroll-area",
            "window" => "window",
            "other" => "other",
            _ => return None,
        };
        let walk_end = match fields[1] {
            "boundary-web-area" => "boundary-web-area",
            "boundary-scroll-area" => "boundary-scroll-area",
            "boundary-window" => "boundary-window",
            "root" => "root",
            "depth-limit" => "depth-limit",
            _ => return None,
        };
        let mut values = [0_u16; 7];
        for (i, field) in fields[2..].iter().enumerate() {
            if field.is_empty()
                || field.len() > 4
                || !field.bytes().all(|v| v.is_ascii_digit())
                || (field.len() > 1 && field.starts_with('0'))
            {
                return None;
            }
            values[i] = field.parse().ok()?;
            if values[i] > 1024 {
                return None;
            }
        }
        if values[0] > 6
            || values[4] > values[3]
            || values[6] > values[5]
            || (parent_kind == "none" && (values[0] != 0 || walk_end != "root"))
        {
            return None;
        }
        Some(Self {
            parent_kind,
            walk_end,
            group_ancestor_count: values[0],
            source_row_labels_any_role: values[1],
            streaming_labels_any_role: values[2],
            try_again_labels_any_role: values[3],
            try_again_buttons: values[4],
            view_details_labels_any_role: values[5],
            view_details_buttons: values[6],
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ChatTurnReceipt {
    pub(crate) stage: ChatTurnStage,
    pub(crate) row_shape: Option<FailureRowShape>,
    pub(crate) scope_shape: Option<FailureScopeShape>,
}
impl ChatTurnReceipt {
    pub(crate) fn parse(output: &str, mode: &str) -> Option<Self> {
        if output.len() > 4096 {
            return None;
        }
        let (first, remainder) = output.split_once('\n')?;
        let stage = ChatTurnStage::parse(&format!("{first}\n"))?;
        if remainder.is_empty() {
            return Some(Self {
                stage,
                row_shape: None,
                scope_shape: None,
            });
        }
        if !matches!(
            mode,
            "failure-details"
                | "failure-details-ready"
                | "failure-details-temporal"
                | "failure-details-ready-temporal"
        ) {
            return None;
        }
        let (row_line, remainder) = remainder.split_once('\n')?;
        let scope_shape = if remainder.is_empty() {
            None
        } else {
            Some(FailureScopeShape::parse(remainder.strip_suffix('\n')?)?)
        };
        let row_line = row_line.strip_prefix("rows ")?;
        let fields: Vec<_> = row_line.split(' ').collect();
        if fields.len() != 13 {
            return None;
        }
        let mut values = [0_u16; 13];
        for (index, field) in fields.iter().enumerate() {
            if field.is_empty()
                || field.len() > 4
                || !field.bytes().all(|value| value.is_ascii_digit())
                || (field.len() > 1 && field.starts_with('0'))
            {
                return None;
            }
            values[index] = field.parse().ok()?;
            if values[index] > 1024 {
                return None;
            }
        }
        if values[1] > values[0]
            || values[7] > values[0]
            || values[8] > values[0]
            || values[10] > values[9]
            || values[12] > values[0]
            || u32::from(values[9]) > u32::from(values[7]) * u32::from(values[8])
        {
            return None;
        }
        Some(Self {
            stage,
            scope_shape,
            row_shape: Some(FailureRowShape {
                source_version: "2.19675.0",
                source_sha256: "87e6b710a540352fcd4f9a1f0f6a8f9f9b6377ca676fd99c3e4d8bc87653dceb",
                navigation_source_sha256: "948270963cdf93cc411d95393157f5c7e2c06f18916d8c4ea1971828fb0c677c",
                phase: "pre-disclosure",
                counts: FailureRowCounts {
                    source_rows: values[0],
                    streaming_rows: values[1],
                    exact_user_headings: values[2],
                    exact_prompt_nodes: values[3],
                    server_error_labels: values[4],
                    retry_controls: values[5],
                    details_controls: values[6],
                    user_rows: values[7],
                    error_rows: values[8],
                    shared_parent_pairs: values[9],
                    adjacent_pairs: values[10],
                    assistant_headings_in_error_rows: values[11],
                    duplicate_positions: values[12],
                },
            }),
        })
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
        "input"
            | "input-replace-owned"
            | "copy"
            | "retry-ready"
            | "retry"
            | "failure-details"
            | "failure-details-ready"
            | "input-failure-owned"
            | "failure-details-temporal"
            | "failure-details-ready-temporal"
            | "retry-ready-temporal"
            | "retry-temporal"
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
    #[test]
    fn optional_scope_shape_is_bounded_and_never_changes_stage() {
        let legacy = "turn scope-control-absent\nrows 0 0 0 0 1 1 1 0 0 0 0 0 0\n";
        assert!(
            ChatTurnReceipt::parse(legacy, "failure-details-ready")
                .unwrap()
                .scope_shape
                .is_none()
        );
        let framed = format!("{legacy}scope web-area boundary-web-area 0 0 0 1 1 1 1\n");
        let receipt = ChatTurnReceipt::parse(&framed, "failure-details-ready").unwrap();
        assert_eq!(receipt.stage, ChatTurnStage::ScopeControlAbsent);
        assert!(receipt.scope_shape.is_some());
        assert!(ChatTurnReceipt::parse(&framed, "copy").is_none());
        for line in [
            "scope PRIVATE root 0 0 0 0 0 0 0",
            "scope group PRIVATE 0 0 0 0 0 0 0",
            "scope group root 7 0 0 0 0 0 0",
            "scope group root 1 1025 0 0 0 0 0",
            "scope group root 1 0 0 0 1 0 0",
            "scope group root 1 0 0 0 0 0 1",
            "scope none depth-limit 0 0 0 0 0 0 0",
            "scope group root 01 0 0 0 0 0 0",
            "scope group root 1 0 0 0 0 0 0 PRIVATE",
        ] {
            assert!(
                ChatTurnReceipt::parse(&format!("{legacy}{line}\n"), "failure-details").is_none()
            );
        }
        assert!(
            ChatTurnReceipt::parse(
                &format!("{framed}scope group root 0 0 0 0 0 0 0\n"),
                "failure-details"
            )
            .is_none()
        );
    }

    #[test]
    fn failure_rows_are_closed_advisory_and_only_accepted_for_disclosure() {
        let observed = "turn scope-heading-ambiguous\nrows 2 0 1 1 1 1 1 1 1 1 1 1 0\n";
        let receipt = ChatTurnReceipt::parse(observed, "failure-details").unwrap();
        assert!(ChatTurnReceipt::parse(observed, "failure-details-ready").is_some());
        assert_eq!(receipt.stage, ChatTurnStage::ScopeHeadingAmbiguous);
        let shape = receipt.row_shape.unwrap();
        assert_eq!(shape.counts.adjacent_pairs, 1);
        assert!(!serde_json::to_string(&shape).unwrap().contains("PRIVATE"));
        assert!(ChatTurnReceipt::parse(observed, "retry").is_none());
        for invalid in [
            observed.replace("rows 2", "rows 1025"),
            observed.replace("rows 2", "rows 02"),
            observed.replace("rows 2", "rows PRIVATE"),
            format!("{observed}PRIVATE"),
            observed.replace("1 1 1 0\n", "1 2 1 0\n"),
        ] {
            assert!(ChatTurnReceipt::parse(&invalid, "failure-details").is_none());
        }
        assert_eq!(
            ChatTurnReceipt::parse("turn scope\n", "failure-details")
                .unwrap()
                .row_shape,
            None
        );
        assert!(ChatTurnReceipt::parse("turn scope PRIVATE\n", "failure-details").is_none());
    }

    use super::*;

    #[test]
    fn disclosure_receipt_does_not_authorize_retry_or_accept_payloads() {
        let stage = ChatTurnStage::parse("turn failure-details-opened\n").unwrap();
        assert_eq!(stage, ChatTurnStage::FailureDetailsOpened);
        assert!(!stage.passive_pending());
        assert_eq!(
            serde_json::to_value(stage).unwrap(),
            "failure-details-opened"
        );
        assert!(ChatTurnStage::parse("turn failure-details-opened PRIVATE\n").is_none());
    }

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
        assert!(ChatTurnStage::Control.pre_action_pending());
        assert_eq!(ChatTurnStage::Control.disclosure_ready(), Some(false));
        assert!(!ChatTurnStage::Control.passive_pending());
        for stage in [
            ChatTurnStage::ActionUncertain,
            ChatTurnStage::Retried,
            ChatTurnStage::Window,
        ] {
            assert!(!stage.pre_action_pending());
        }
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
            ("input-paste-unsettled", ChatTurnStage::InputPasteUnsettled),
            (
                "input-paste-focus-before",
                ChatTurnStage::InputPasteFocusBefore,
            ),
            (
                "input-paste-value-unavailable",
                ChatTurnStage::InputPasteValueUnavailable,
            ),
            (
                "input-paste-query-failed",
                ChatTurnStage::InputPasteQueryFailed,
            ),
            (
                "input-paste-focus-after",
                ChatTurnStage::InputPasteFocusAfter,
            ),
            (
                "input-paste-unexpected-value",
                ChatTurnStage::InputPasteUnexpectedValue,
            ),
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

#[cfg(test)]
mod disclosure_readiness_tests {
    use super::ChatTurnReceipt;
    use super::ChatTurnStage as S;
    #[test]
    fn pending_does_not_consume_disclosure_and_uncertainty_is_terminal() {
        let mut attempts = 0;
        for stage in [S::ScopeAnchorAbsent, S::TreeQuery, S::FailureDetailsReady] {
            match stage.disclosure_ready() {
                Some(true) => attempts += 1,
                Some(false) => {}
                None => panic!("unexpected terminal stage"),
            }
        }
        assert_eq!(attempts, 1);
        for stage in [S::ActionUncertain, S::TreePid, S::TreeWindow, S::Deadline] {
            assert_eq!(stage.disclosure_ready(), None);
        }
    }

    #[test]
    fn temporal_modes_preserve_closed_receipts_and_do_not_admit_extra_payloads() {
        for mode in ["failure-details-temporal", "failure-details-ready-temporal"] {
            assert!(
                ChatTurnReceipt::parse(
                    "turn scope-anchor-absent\nrows 0 0 1 1 1 1 0 0 0 0 0 0 0\n",
                    mode
                )
                .is_some()
            );
            assert!(ChatTurnReceipt::parse("turn scope-anchor-absent\nPRIVATE", mode).is_none());
        }
        assert!(
            ChatTurnReceipt::parse(
                "turn retry-ready\nrows 0 0 1 1 1 1 0 0 0 0 0 0 0\n",
                "retry-ready-temporal"
            )
            .is_none()
        );
    }
}
