//! Closed observations of one uncertain native press; never action authority.
use super::ChatTurnStage;

fn label(value: &str, allowed: &[&'static str]) -> Option<&'static str> {
    allowed.iter().copied().find(|allowed| *allowed == value)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct WindowProof {
    failure: &'static str,
    focus_status: &'static str,
    query_stage: &'static str,
    query_error: &'static str,
}
impl WindowProof {
    fn parse(fields: &[&str]) -> Option<Self> {
        Some(Self {
            failure: label(
                fields[0],
                &[
                    "foreground",
                    "application",
                    "focus",
                    "inventory",
                    "identity",
                    "stack",
                    "display",
                    "geometry",
                ],
            )?,
            focus_status: label(
                fields[1],
                &[
                    "unqueried",
                    "untrusted",
                    "query-error",
                    "focus-mismatch",
                    "not-standard",
                    "ready",
                ],
            )?,
            query_stage: label(
                fields[2],
                &[
                    "unqueried",
                    "app-create",
                    "app-timeout",
                    "focused-window",
                    "main-window",
                    "focused-element",
                    "input-timeout",
                    "input-window",
                    "element-type",
                    "pid",
                    "window-timeout",
                    "role",
                    "subrole",
                    "position",
                    "size",
                    "geometry",
                ],
            )?,
            query_error: label(
                fields[3],
                &[
                    "unqueried",
                    "failure",
                    "illegal-argument",
                    "invalid-element",
                    "cannot-complete",
                    "attribute-unsupported",
                    "not-implemented",
                    "api-disabled",
                    "no-value",
                    "other",
                    "empty-value",
                    "type-mismatch",
                    "owner-mismatch",
                    "geometry-invalid",
                ],
            )?,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PressRejection {
    mode: &'static str,
    phase: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    ax_error: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cause: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    window: Option<WindowProof>,
}
impl PressRejection {
    fn parse(line: &str, mode: &str) -> Option<Self> {
        let fields: Vec<_> = line.strip_prefix("press-rejection ")?.split(' ').collect();
        if !matches!(fields.len(), 3 | 7) || fields[0] != mode {
            return None;
        }
        let mode = label(
            mode,
            &[
                "input",
                "input-replace-owned",
                "input-failure-owned",
                "copy",
                "retry",
                "retry-temporal",
                "failure-details",
                "failure-details-temporal",
            ],
        )?;
        let (phase, ax_error, cause, window) = match fields[1..] {
            ["perform-action", error] => (
                "perform-action",
                Some(label(
                    error,
                    &[
                        "failure",
                        "illegal-argument",
                        "invalid-element",
                        "cannot-complete",
                        "action-unsupported",
                        "attribute-unsupported",
                        "not-implemented",
                        "api-disabled",
                        "no-value",
                        "other",
                    ],
                )?),
                None,
                None,
            ),
            ["post-guard", cause] => (
                "post-guard",
                None,
                Some(label(cause, &["deadline-before", "deadline-after"])?),
                None,
            ),
            ["post-guard", "owned-window", ..] if fields.len() == 7 => (
                "post-guard",
                None,
                Some("owned-window"),
                Some(WindowProof::parse(&fields[3..])?),
            ),
            _ => return None,
        };
        Some(Self {
            mode,
            phase,
            ax_error,
            cause,
            window,
        })
    }
    pub(super) fn split<'a>(
        output: &'a str,
        mode: &str,
        stage: ChatTurnStage,
    ) -> Option<(&'a str, Option<Self>)> {
        let Some(index) = output.rfind("\npress-rejection ") else {
            return Some((output, None));
        };
        if stage != ChatTurnStage::ActionUncertain {
            return None;
        }
        let value = Self::parse(output[index + 1..].strip_suffix('\n')?, mode)?;
        Some((&output[..=index], Some(value)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::mac_chat_turn::ChatTurnReceipt;
    #[test]
    fn uncertain_press_diagnostics_are_closed_and_bind_the_actual_mode() {
        for mode in [
            "input",
            "input-replace-owned",
            "input-failure-owned",
            "copy",
            "retry",
            "retry-temporal",
            "failure-details",
            "failure-details-temporal",
        ] {
            for detail in [
                "perform-action action-unsupported",
                "perform-action cannot-complete",
                "post-guard deadline-before",
                "post-guard deadline-after",
                "post-guard owned-window focus query-error focused-window cannot-complete",
            ] {
                let line = format!("press-rejection {mode} {detail}");
                let value = PressRejection::parse(&line, mode).unwrap();
                assert_eq!(serde_json::to_value(value).unwrap()["mode"], mode);
                assert!(PressRejection::parse(&line, "retry-ready").is_none());
                let raw = format!("turn action-uncertain\n{line}\n");
                assert!(
                    ChatTurnReceipt::parse(&raw, mode)
                        .unwrap()
                        .press_rejection
                        .is_some()
                );
            }
        }
        let raw = "turn action-uncertain\ncomposer 1 0 1\npress-rejection input perform-action action-unsupported\n";
        assert!(
            ChatTurnReceipt::parse(raw, "input")
                .unwrap()
                .press_rejection
                .is_some()
        );
        for stage in ["sent", "retried", "input-value-mismatch", "deadline-press"] {
            assert!(
                ChatTurnReceipt::parse(&raw.replacen("action-uncertain", stage, 1), "input")
                    .is_none()
            );
        }
        assert!(
            ChatTurnReceipt::parse(
                &format!("{raw}press-rejection input perform-action failure\n"),
                "input"
            )
            .is_none()
        );
        assert!(
            ChatTurnReceipt::parse("turn action-uncertain\ncomposer 1 0 1\n", "input")
                .unwrap()
                .press_rejection
                .is_none()
        );
    }
    #[test]
    fn malformed_press_diagnostics_never_enter_receipts() {
        for bad in [
            "input perform-action success",
            "input perform-action PRIVATE",
            "input perform-action null",
            "input perform-action {\"cannot-complete\":null}",
            "{\"input\":null} perform-action failure",
            "input {\"perform-action\":null} failure",
            "input post-guard failure",
            "input post-guard owned-window",
            "input post-guard owned-window focus query-error focused-window PRIVATE",
            "input post-guard owned-window focus query-error PRIVATE cannot-complete",
            "input post-guard owned-window focus PRIVATE focused-window cannot-complete",
            "input post-guard owned-window PRIVATE query-error focused-window cannot-complete",
            "input post-guard owned-window focus query-error focused-window cannot-complete PRIVATE",
            "retry-ready perform-action failure",
            "input perform-action  failure",
            "input\tperform-action failure",
        ] {
            assert!(PressRejection::parse(&format!("press-rejection {bad}"), "input").is_none());
        }
    }
}
