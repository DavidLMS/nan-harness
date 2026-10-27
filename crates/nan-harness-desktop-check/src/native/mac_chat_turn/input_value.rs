//! Closed observations of existing input-value rejection predicates.
use super::ChatTurnStage;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
enum Site {
    PasteFocusBefore,
    PasteValue,
    PasteFocusAfter,
    PasteRelation,
    ReadbackValue,
}
impl Site {
    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "paste-focus-before" => Self::PasteFocusBefore,
            "paste-value" => Self::PasteValue,
            "paste-focus-after" => Self::PasteFocusAfter,
            "paste-relation" => Self::PasteRelation,
            "readback-value" => Self::ReadbackValue,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
enum Cause {
    Deadline,
    QueryFailed,
    Application,
    FocusedElement,
    TargetPid,
    TargetBoundsUnavailable,
    TargetGeometry,
    TargetRole,
    FocusedPid,
    FocusedBoundsUnavailable,
    FocusedGeometry,
    FocusedRole,
    FocusedIdentity,
    ValueUnavailable,
    ValueType,
    ValueSize,
    ValueEncoding,
    ValueOther,
    WindowUnmeasured,
    WindowForeground,
    WindowApplication,
    WindowFocus,
    WindowGeometry,
    WindowInventory,
    WindowIdentity,
    WindowLayer,
    WindowDisplay,
    WindowStack,
}
impl Cause {
    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "deadline" => Self::Deadline,
            "query-failed" => Self::QueryFailed,
            "application" => Self::Application,
            "focused-element" => Self::FocusedElement,
            "target-pid" => Self::TargetPid,
            "target-bounds-unavailable" => Self::TargetBoundsUnavailable,
            "target-geometry" => Self::TargetGeometry,
            "target-role" => Self::TargetRole,
            "focused-pid" => Self::FocusedPid,
            "focused-bounds-unavailable" => Self::FocusedBoundsUnavailable,
            "focused-geometry" => Self::FocusedGeometry,
            "focused-role" => Self::FocusedRole,
            "focused-identity" => Self::FocusedIdentity,
            "value-unavailable" => Self::ValueUnavailable,
            "value-type" => Self::ValueType,
            "value-size" => Self::ValueSize,
            "value-encoding" => Self::ValueEncoding,
            "value-other" => Self::ValueOther,
            "window-unmeasured" => Self::WindowUnmeasured,
            "window-foreground" => Self::WindowForeground,
            "window-application" => Self::WindowApplication,
            "window-focus" => Self::WindowFocus,
            "window-geometry" => Self::WindowGeometry,
            "window-inventory" => Self::WindowInventory,
            "window-identity" => Self::WindowIdentity,
            "window-layer" => Self::WindowLayer,
            "window-display" => Self::WindowDisplay,
            "window-stack" => Self::WindowStack,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
enum Relation {
    Unmeasured,
    Expected,
    Initial,
    Prefix,
    Other,
}
impl Relation {
    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "unmeasured" => Self::Unmeasured,
            "expected" => Self::Expected,
            "initial" => Self::Initial,
            "prefix" => Self::Prefix,
            "other" => Self::Other,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InputValueRejection {
    site: Site,
    cause: Cause,
    value_relation: Relation,
}
impl InputValueRejection {
    pub(super) fn parse(line: &str) -> Option<Self> {
        let fields: Vec<_> = line
            .strip_prefix("input-value-rejection ")?
            .split(' ')
            .collect();
        if fields.len() != 3 {
            return None;
        }
        let value = Self {
            site: Site::parse(fields[0])?,
            cause: Cause::parse(fields[1])?,
            value_relation: Relation::parse(fields[2])?,
        };
        value.valid().then_some(value)
    }
    fn valid(self) -> bool {
        use Cause as C;
        use Relation as R;
        use Site as S;
        let format = matches!(
            self.cause,
            C::QueryFailed | C::ValueUnavailable | C::ValueType | C::ValueSize | C::ValueEncoding
        );
        let focus = !matches!(
            self.cause,
            C::ValueUnavailable | C::ValueType | C::ValueSize | C::ValueEncoding | C::ValueOther
        );
        match self.site {
            S::PasteFocusBefore => focus && self.value_relation == R::Unmeasured,
            S::PasteFocusAfter => focus,
            S::PasteValue => format && self.value_relation == R::Unmeasured,
            S::PasteRelation => self.cause == C::ValueOther && self.value_relation == R::Other,
            S::ReadbackValue => {
                (format && self.value_relation == R::Unmeasured)
                    || (self.cause == C::ValueOther && self.value_relation == R::Other)
            }
        }
    }
    pub(super) fn split<'a>(
        output: &'a str,
        mode: &str,
        stage: ChatTurnStage,
    ) -> Option<(&'a str, Option<Self>)> {
        let Some(index) = output.rfind("\ninput-value-rejection ") else {
            return Some((output, None));
        };
        if stage != ChatTurnStage::InputValueMismatch
            || !matches!(
                mode,
                "input" | "input-replace-owned" | "input-failure-owned"
            )
        {
            return None;
        }
        let value = Self::parse(output[index + 1..].strip_suffix('\n')?)?;
        Some((&output[..=index], Some(value)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::mac_chat_turn::ChatTurnReceipt;
    #[test]
    fn input_rejection_receipts_bind_closed_causes_to_input_stages() {
        let causes = [
            "deadline",
            "query-failed",
            "application",
            "focused-element",
            "target-pid",
            "target-bounds-unavailable",
            "target-geometry",
            "target-role",
            "focused-pid",
            "focused-bounds-unavailable",
            "focused-geometry",
            "focused-role",
            "focused-identity",
            "window-unmeasured",
            "window-foreground",
            "window-application",
            "window-focus",
            "window-geometry",
            "window-inventory",
            "window-identity",
            "window-layer",
            "window-display",
            "window-stack",
        ];
        for cause in causes {
            for (site, relation) in [
                ("paste-focus-before", "unmeasured"),
                ("paste-focus-after", "expected"),
            ] {
                let raw = format!(
                    "turn input-value-mismatch\ncomposer 1 1 0\ninput-value-rejection {site} {cause} {relation}\n"
                );
                let receipt = ChatTurnReceipt::parse(&raw, "input-replace-owned").unwrap();
                let value = serde_json::to_value(receipt.input_value_rejection.unwrap()).unwrap();
                assert_eq!(value.as_object().unwrap().len(), 3);
                assert_eq!(value["cause"], cause);
                assert_eq!(value["valueRelation"], relation);
                for mode in ["copy", "retry", "PRIVATE"] {
                    assert!(ChatTurnReceipt::parse(&raw, mode).is_none());
                }
                for stage in [
                    "sent",
                    "deadline-input-paste",
                    "composer",
                    "action-uncertain",
                ] {
                    assert!(
                        ChatTurnReceipt::parse(
                            &raw.replacen("input-value-mismatch", stage, 1),
                            "input"
                        )
                        .is_none()
                    );
                }
            }
        }
        for cause in [
            "query-failed",
            "value-unavailable",
            "value-type",
            "value-size",
            "value-encoding",
        ] {
            assert!(
                InputValueRejection::parse(&format!(
                    "input-value-rejection paste-value {cause} unmeasured"
                ))
                .is_some()
            );
        }
        for relation in ["unmeasured", "expected", "initial", "prefix", "other"] {
            assert!(
                InputValueRejection::parse(&format!(
                    "input-value-rejection paste-focus-after target-geometry {relation}"
                ))
                .is_some()
            );
        }
    }
    #[test]
    fn malformed_input_rejections_and_extra_receipt_data_are_rejected() {
        for bad in [
            "paste-value value-other other",
            "paste-relation value-other prefix",
            "readback-value value-other expected",
            "paste-focus-before target-geometry expected",
            "paste-focus-before value-type unmeasured",
            "paste-value target-pid unmeasured",
            "readback-value window-geometry unmeasured",
            "paste-value value-type null",
            "paste-value null unmeasured",
            "null value-type unmeasured",
            "paste-value value-type {\"unmeasured\":null}",
            "paste-value {\"value-type\":null} unmeasured",
            "{\"paste-value\":null} value-type unmeasured",
            "paste-value PRIVATE unmeasured",
            "paste-value  value-type unmeasured",
            "paste-value\tvalue-type unmeasured",
            "paste-value value-type unmeasured PRIVATE",
        ] {
            assert!(
                InputValueRejection::parse(&format!("input-value-rejection {bad}")).is_none(),
                "{bad}"
            );
        }
        let valid = "turn input-value-mismatch\ncomposer 1 1 0\ninput-value-rejection paste-relation value-other other\n";
        assert!(ChatTurnReceipt::parse(valid, "input-failure-owned").is_some());
        assert!(
            ChatTurnReceipt::parse(
                &format!("{valid}input-value-rejection paste-relation value-other other\n"),
                "input"
            )
            .is_none()
        );
        assert!(ChatTurnReceipt::parse(&format!("{valid}PRIVATE"), "input").is_none());
    }
}
