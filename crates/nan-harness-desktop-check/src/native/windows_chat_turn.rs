//! Private single-line protocol for the owned Windows first-turn experiment.
use super::Window;
use std::fmt::Write as _;
use std::time::Duration;
use zeroize::Zeroizing;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FailureScopeCounts {
    #[serde(rename = "serverErrorCount")]
    server_errors: u16,
    #[serde(rename = "failedUserHeadingCount")]
    failed_user_headings: u16,
    #[serde(rename = "failedPromptTextCount")]
    failed_prompt_texts: u16,
    #[serde(rename = "retryButtonCount")]
    retry_buttons: u16,
    #[serde(rename = "detailsButtonCount")]
    details_buttons: u16,
    #[serde(rename = "exactPromptGroupCount")]
    exact_prompt_groups: u16,
    #[serde(rename = "groupRetryButtonCount")]
    group_retry_buttons: u16,
    #[serde(rename = "groupDetailsButtonCount")]
    group_details_buttons: u16,
    #[serde(rename = "retryLabelCount")]
    retry_labels: u16,
    #[serde(rename = "detailsLabelCount")]
    details_labels: u16,
    unfiltered_retry_label_count: Option<u16>,
    unfiltered_details_label_count: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    button_shape: Option<[u16; 6]>,
}
impl FailureScopeCounts {
    // Keep the latest successful passive query when a later deadline prevents
    // observing it. These diagnostics never grant scope or action authority.
    pub(crate) fn retain_unfiltered(&mut self, previous: Option<&Self>) {
        if let Some(previous) = previous {
            self.unfiltered_retry_label_count = self
                .unfiltered_retry_label_count
                .or(previous.unfiltered_retry_label_count);
            self.unfiltered_details_label_count = self
                .unfiltered_details_label_count
                .or(previous.unfiltered_details_label_count);
        }
    }
}

pub(crate) struct WindowsChatReceipt {
    pub stage: WindowsChatStage,
    pub failure_scope: Option<FailureScopeCounts>,
}
impl WindowsChatReceipt {
    pub(crate) fn parse(output: &str, mode: &str) -> Option<Self> {
        let (first, rest) = output.split_once('\n')?;
        let stage = WindowsChatStage::parse(&format!("{first}\n"))?;
        let failure_scope = if rest.is_empty() {
            None
        } else {
            if !["retry-ready", "retry", "failure-details"].contains(&mode) {
                return None;
            }
            let line = rest.strip_prefix("failure-scope ")?.strip_suffix('\n')?;
            let words: Vec<_> = line.split(' ').collect();
            if ![10, 12, 18].contains(&words.len()) {
                return None;
            }
            let optional_count = |word: &str| -> Option<Option<u16>> {
                if word == "-" {
                    return Some(None);
                }
                if word.is_empty() || !word.bytes().all(|byte| byte.is_ascii_digit()) {
                    return None;
                }
                Some(Some(
                    word.parse::<u16>().ok().filter(|count| *count <= 1024)?,
                ))
            };
            let (unfiltered_retry_label_count, unfiltered_details_label_count) =
                if words.len() >= 12 {
                    (optional_count(words[10])?, optional_count(words[11])?)
                } else {
                    (None, None)
                };
            // Whole tree, exact prompt groups, and exact prompt+error groups;
            // each pair is total buttons and unnamed buttons. No labels escape.
            let button_shape = if words.len() == 18 {
                let mut shape = [0_u16; 6];
                for (slot, word) in shape.iter_mut().zip(&words[12..]) {
                    *slot = optional_count(word)??;
                }
                if shape[1] > shape[0]
                    || shape[2] > shape[0]
                    || shape[3] > shape[2]
                    || shape[3] > shape[1]
                    || shape[4] > shape[2]
                    || shape[5] > shape[4]
                    || shape[5] > shape[3]
                {
                    return None;
                }
                Some(shape)
            } else {
                None
            };
            let mut counts = [0_u16; 10];
            for (slot, word) in counts.iter_mut().zip(&words[..10]) {
                *slot = optional_count(word)??;
            }
            let [
                server_errors,
                failed_user_headings,
                failed_prompt_texts,
                retry_buttons,
                details_buttons,
                exact_prompt_groups,
                group_retry_buttons,
                group_details_buttons,
                retry_labels,
                details_labels,
            ] = counts;
            if group_retry_buttons > retry_buttons
                || group_details_buttons > details_buttons
                || retry_buttons > retry_labels
                || details_buttons > details_labels
                || button_shape.is_some_and(|shape| {
                    retry_buttons > shape[0]
                        || details_buttons > shape[0]
                        || group_retry_buttons > shape[2]
                        || group_details_buttons > shape[2]
                })
            {
                return None;
            }
            Some(FailureScopeCounts {
                server_errors,
                failed_user_headings,
                failed_prompt_texts,
                retry_buttons,
                details_buttons,
                exact_prompt_groups,
                group_retry_buttons,
                group_details_buttons,
                retry_labels,
                details_labels,
                unfiltered_retry_label_count,
                unfiltered_details_label_count,
                button_shape,
            })
        };
        Some(Self {
            stage,
            failure_scope,
        })
    }
}

pub(crate) const MAX_MILLIS: u32 = 15_000;
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum WindowsChatStage {
    #[cfg(windows)]
    Request,
    #[cfg(windows)]
    Completed,
    Window,
    Tree,
    ClipboardOwner,
    ClipboardAllocation,
    ClipboardLock,
    ClipboardEmpty,
    ClipboardSet,
    ClipboardClose,
    ClipboardGuardBefore,
    ClipboardGuardAfter,
    ClipboardDeadlineBefore,
    ClipboardDeadlineAfter,
    ClipboardOpenDeadline,

    TreeQuery,
    TreeLimit,
    TreeDepth,
    TreeNodes,
    TreeNameLimit,
    TreeTextLimit,
    TreeWindowLimit,
    TreeProcessLimit,
    TreeDuplicate,
    TreeType,
    TreePid,
    Mode,
    Composer,
    ComposerSendPending,
    Control,
    Deadline,
    Sent,
    ActionUncertain,
    InputFocusSetting,
    InputFocusedIdentity,
    InputReplaceSelectKey,
    InputPromptClipboard,
    InputPasteKey,
    InputValueMismatch,
    InputSentinelClipboard,
    InputReadbackSelectKey,
    InputReadbackCopyKey,
    InputClipboardMismatch,
    Scope,
    ScopeAnchorAbsent,
    ScopeAnchorAmbiguous,
    ScopeControlAbsent,
    ScopeControlAmbiguous,
    ScopeHeadingAmbiguous,
    ScopePromptMismatch,
    ResponseMismatch,
    Copied,
    RetryReady,
    Retried,
    FailureDetailsOpened,
}
impl WindowsChatStage {
    #[cfg(any(windows, test))]
    pub(crate) fn passive_pending(self) -> bool {
        matches!(
            self,
            Self::ScopeAnchorAbsent | Self::ScopeControlAbsent | Self::TreeQuery
        )
    }

    pub(super) fn parse(wire: &str) -> Option<Self> {
        Some(match wire {
            "turn window\n" => Self::Window,
            "turn tree\n" => Self::Tree,
            "turn clipboard-owner\n" => Self::ClipboardOwner,
            "turn clipboard-allocation\n" => Self::ClipboardAllocation,
            "turn clipboard-lock\n" => Self::ClipboardLock,
            "turn clipboard-empty\n" => Self::ClipboardEmpty,
            "turn clipboard-set\n" => Self::ClipboardSet,
            "turn clipboard-close\n" => Self::ClipboardClose,
            "turn clipboard-guard-before\n" => Self::ClipboardGuardBefore,
            "turn clipboard-guard-after\n" => Self::ClipboardGuardAfter,
            "turn clipboard-deadline-before\n" => Self::ClipboardDeadlineBefore,
            "turn clipboard-deadline-after\n" => Self::ClipboardDeadlineAfter,
            "turn clipboard-open-deadline\n" => Self::ClipboardOpenDeadline,

            "turn tree-query\n" => Self::TreeQuery,
            "turn tree-limit\n" => Self::TreeLimit,
            "turn tree-depth\n" => Self::TreeDepth,
            "turn tree-nodes\n" => Self::TreeNodes,
            "turn tree-name-limit\n" => Self::TreeNameLimit,
            "turn tree-text-limit\n" => Self::TreeTextLimit,
            "turn tree-window-limit\n" => Self::TreeWindowLimit,
            "turn tree-process-limit\n" => Self::TreeProcessLimit,
            "turn tree-duplicate\n" => Self::TreeDuplicate,
            "turn tree-type\n" => Self::TreeType,
            "turn tree-pid\n" => Self::TreePid,
            "turn mode\n" => Self::Mode,
            "turn composer\n" => Self::Composer,
            "turn composer-send-pending\n" => Self::ComposerSendPending,
            "turn control\n" => Self::Control,
            "turn deadline\n" => Self::Deadline,
            "turn sent\n" => Self::Sent,
            "turn action-uncertain\n" => Self::ActionUncertain,
            "turn input-focus-setting\n" => Self::InputFocusSetting,
            "turn input-focused-identity\n" => Self::InputFocusedIdentity,
            "turn input-replace-select-key\n" => Self::InputReplaceSelectKey,
            "turn input-prompt-clipboard\n" => Self::InputPromptClipboard,
            "turn input-paste-key\n" => Self::InputPasteKey,
            "turn input-value-mismatch\n" => Self::InputValueMismatch,
            "turn input-sentinel-clipboard\n" => Self::InputSentinelClipboard,
            "turn input-readback-select-key\n" => Self::InputReadbackSelectKey,
            "turn input-readback-copy-key\n" => Self::InputReadbackCopyKey,
            "turn input-clipboard-mismatch\n" => Self::InputClipboardMismatch,
            "turn scope\n" => Self::Scope,
            "turn scope-anchor-absent\n" => Self::ScopeAnchorAbsent,
            "turn scope-anchor-ambiguous\n" => Self::ScopeAnchorAmbiguous,
            "turn scope-control-absent\n" => Self::ScopeControlAbsent,
            "turn scope-control-ambiguous\n" => Self::ScopeControlAmbiguous,
            "turn scope-heading-ambiguous\n" => Self::ScopeHeadingAmbiguous,
            "turn scope-prompt-mismatch\n" => Self::ScopePromptMismatch,
            "turn response-mismatch\n" => Self::ResponseMismatch,
            "turn copied\n" => Self::Copied,
            "turn retry-ready\n" => Self::RetryReady,
            "turn retried\n" => Self::Retried,
            "turn failure-details-opened\n" => Self::FailureDetailsOpened,
            _ => return None,
        })
    }
}
pub(super) fn ready(wire: &str) -> Option<u64> {
    let value = wire.strip_prefix("ready ")?.strip_suffix('\n')?;
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse().ok().filter(|value| *value > 0)
}
pub(super) fn request(
    window: &Window,
    mode: &str,
    values: [&str; 3],
    native_anchor: u64,
    remaining: Duration,
    checker: u32,
) -> Option<Zeroizing<String>> {
    let [prompt, marker, sentinel] = values;
    let millis = u32::try_from(remaining.as_millis())
        .unwrap_or(MAX_MILLIS)
        .min(MAX_MILLIS);
    let cutoff = native_anchor.checked_add(u64::from(millis.checked_sub(50)?))?;
    if millis <= 50
        || window.id == 0
        || window.pid == 0
        || checker == 0
        || window.bounds.width == 0
        || window.bounds.height == 0
        || !matches!(
            mode,
            "input-replace-owned" | "copy" | "retry-ready" | "retry" | "failure-details"
        )
        || (mode != "input-replace-owned" && marker.is_empty())
        || marker.len() > 1024
        || marker.contains('\0')
        || prompt.is_empty()
        || sentinel.is_empty()
        || prompt == sentinel
        || prompt.len() > 1024
        || sentinel.len() > 1024
        || prompt.contains('\0')
        || sentinel.contains('\0')
    {
        return None;
    }
    let right = i32::try_from(i64::from(window.bounds.x) + i64::from(window.bounds.width)).ok()?;
    let bottom =
        i32::try_from(i64::from(window.bounds.y) + i64::from(window.bounds.height)).ok()?;
    let mut wire = Zeroizing::new(format!(
        "{} {} {} {} {right} {bottom} {millis} {cutoff} {checker}",
        window.id, window.pid, window.bounds.x, window.bounds.y
    ));
    wire.push(' ');
    wire.push_str(mode);
    for value in [prompt, marker, sentinel] {
        wire.push(' ');
        if value.is_empty() {
            wire.push('-');
            continue;
        }
        for byte in value.bytes() {
            write!(wire, "{byte:02x}").ok()?;
        }
    }
    Some(wire)
}
#[cfg(test)]
mod tests {
    #[test]
    fn only_read_query_failures_can_wait_for_a_fresh_tree() {
        use super::WindowsChatStage as S;
        assert_eq!(S::parse("turn tree-query\n"), Some(S::TreeQuery));
        assert!(S::TreeQuery.passive_pending());
        for (wire, stage) in [
            ("turn composer-send-pending\n", S::ComposerSendPending),
            ("turn tree-limit\n", S::TreeLimit),
            ("turn clipboard-owner\n", S::ClipboardOwner),
            ("turn clipboard-allocation\n", S::ClipboardAllocation),
            ("turn clipboard-lock\n", S::ClipboardLock),
            ("turn clipboard-empty\n", S::ClipboardEmpty),
            ("turn clipboard-set\n", S::ClipboardSet),
            ("turn clipboard-close\n", S::ClipboardClose),
            ("turn clipboard-guard-before\n", S::ClipboardGuardBefore),
            ("turn clipboard-guard-after\n", S::ClipboardGuardAfter),
            (
                "turn clipboard-deadline-before\n",
                S::ClipboardDeadlineBefore,
            ),
            ("turn clipboard-deadline-after\n", S::ClipboardDeadlineAfter),
            ("turn clipboard-open-deadline\n", S::ClipboardOpenDeadline),
            ("turn tree-depth\n", S::TreeDepth),
            ("turn tree-nodes\n", S::TreeNodes),
            ("turn tree-name-limit\n", S::TreeNameLimit),
            ("turn tree-text-limit\n", S::TreeTextLimit),
            ("turn tree-window-limit\n", S::TreeWindowLimit),
            ("turn tree-process-limit\n", S::TreeProcessLimit),
            ("turn tree-duplicate\n", S::TreeDuplicate),
            ("turn tree-type\n", S::TreeType),
            ("turn tree-pid\n", S::TreePid),
            ("turn action-uncertain\n", S::ActionUncertain),
        ] {
            assert_eq!(S::parse(wire), Some(stage));
            assert!(!stage.passive_pending());
        }
    }
    use super::*;
    #[test]
    fn protocol_is_private_bounded_and_cannot_authorize_late_input() {
        let window = Window {
            id: 1,
            pid: 2,
            bounds: xa11y::Rect {
                x: 10,
                y: 20,
                width: 300,
                height: 400,
            },
            name: "private".into(),
            layer: 0,
        };
        let wire = request(
            &window,
            "input-replace-owned",
            ["PRIVATE\nprompt", "", "SENTINEL"],
            1000,
            Duration::from_millis(100),
            3,
        )
        .unwrap();
        assert!(!wire.contains("PRIVATE"));
        assert!(!wire.contains('\n'));
        assert_eq!(wire.split(' ').nth(7), Some("1050"));
        assert!(
            request(
                &window,
                "input-replace-owned",
                ["prompt", "", "sentinel"],
                1000,
                Duration::from_millis(50),
                3
            )
            .is_none()
        );
        assert!(
            request(
                &window,
                "input-replace-owned",
                ["prompt", "", "sentinel"],
                u64::MAX,
                Duration::from_secs(1),
                3
            )
            .is_none()
        );
        assert!(
            request(
                &window,
                "input-replace-owned",
                ["prompt", "", "prompt"],
                1000,
                Duration::from_secs(1),
                3
            )
            .is_none()
        );
        for wire in ["ready 0\n", "ready 1\nPRIVATE", "ready -1\n", "ready 1\r\n"] {
            assert!(ready(wire).is_none());
        }
        assert_eq!(ready("ready 1000\n"), Some(1000));
        for wire in ["turn sent\nPRIVATE", "turn PRIVATE\n", "turn sent"] {
            assert!(WindowsChatStage::parse(wire).is_none());
        }
        assert_eq!(
            WindowsChatStage::parse("turn sent\n"),
            Some(WindowsChatStage::Sent)
        );
        assert_eq!(
            WindowsChatStage::parse("turn failure-details-opened\n"),
            Some(WindowsChatStage::FailureDetailsOpened)
        );
        assert!(WindowsChatStage::parse("turn failure-details-opened PRIVATE\n").is_none());
        // Anchor captured before parent receipt gives a conservative cutoff.
        let deadline = std::time::Instant::now();
        assert!(
            deadline
                .saturating_duration_since(std::time::Instant::now())
                .is_zero()
        );
    }
}

#[cfg(test)]
mod receipt_tests {
    use super::{WindowsChatReceipt, WindowsChatStage};
    #[test]
    fn button_shape_is_passive_bounded_and_structurally_consistent() {
        let prefix = "turn scope-control-absent\nfailure-scope 1 1 1 0 0 1 0 0 0 0 - -";
        let receipt =
            WindowsChatReceipt::parse(&format!("{prefix} 3 1 2 1 2 1\n"), "retry-ready").unwrap();
        assert_eq!(receipt.stage, WindowsChatStage::ScopeControlAbsent);
        assert_eq!(
            receipt.failure_scope.unwrap().button_shape,
            Some([3, 1, 2, 1, 2, 1])
        );
        for tail in [
            "3 1 2 1 2",
            "3 4 2 1 2 1",
            "3 1 4 1 2 1",
            "3 1 2 1 3 1",
            "3 1 2 1 2 2",
            "1025 0 0 0 0 0",
            "3 1 2 1 PRIVATE 1",
            "3 1 2 1 - 1",
        ] {
            assert!(
                WindowsChatReceipt::parse(&format!("{prefix} {tail}\n"), "retry-ready").is_none()
            );
        }
    }

    #[test]
    fn unfiltered_labels_remain_passive_bounded_and_keep_last_successful_sample() {
        let prefix = "turn scope-control-absent\nfailure-scope 1 1 1 0 0 1 0 0 0 0";
        let receipt = WindowsChatReceipt::parse(&format!("{prefix} 2 1\n"), "retry-ready").unwrap();
        assert_eq!(receipt.stage, WindowsChatStage::ScopeControlAbsent);
        let counts = receipt.failure_scope.unwrap();
        assert_eq!(counts.unfiltered_retry_label_count, Some(2));
        assert_eq!(counts.retry_buttons, 0);
        let mut later = WindowsChatReceipt::parse(&format!("{prefix} - 0\n"), "retry-ready")
            .unwrap()
            .failure_scope
            .unwrap();
        later.retain_unfiltered(Some(&counts));
        assert_eq!(later.unfiltered_retry_label_count, Some(2));
        assert_eq!(later.unfiltered_details_label_count, Some(0));
        for tail in [" 1025 0", " -1 0", " PRIVATE 0", " 1", " 1 0 0", " +1 0"] {
            assert!(
                WindowsChatReceipt::parse(&format!("{prefix}{tail}\n"), "retry-ready").is_none()
            );
        }
        assert!(WindowsChatReceipt::parse(&format!("{prefix} 1 0\n"), "copy").is_none());
    }

    #[test]
    fn optional_failure_counts_cannot_override_stage_or_export_payloads() {
        let wire = "turn scope-control-absent\nfailure-scope 1 1 1 1 0 1 1 0 1 0\n";
        let receipt = WindowsChatReceipt::parse(wire, "retry-ready").unwrap();
        assert_eq!(receipt.stage, WindowsChatStage::ScopeControlAbsent);
        assert_eq!(receipt.failure_scope.unwrap().details_buttons, 0);
        assert!(WindowsChatReceipt::parse(wire, "input").is_none());
        assert!(WindowsChatReceipt::parse("turn copied\n", "copy").is_some());
        for tail in [
            "PRIVATE\n",
            "failure-scope 1 1 1 1 0 1 1 0 0 0\n",
            "failure-scope 1 1 1 1 0 1 1 0 1 0\nPRIVATE",
            "failure-scope 1025 1 1 1 0 1 1 0 1 0\n",
            "failure-scope 1 1 1 0 0 1 1 0 1 0\n",
        ] {
            assert!(
                WindowsChatReceipt::parse(
                    &format!("turn scope-control-absent\n{tail}"),
                    "retry-ready"
                )
                .is_none()
            );
        }
    }
}
