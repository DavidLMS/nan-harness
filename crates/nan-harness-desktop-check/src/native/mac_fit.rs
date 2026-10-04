//! Closed stages of the one-time owned AX fit; failures never authorize another fit.

pub(super) fn parse(output: &str) -> Option<&'static str> {
    if position_error(output).is_some() {
        return Some("position");
    }
    match output {
        "" => Some("completed"),
        "fit-rejected request\n" => Some("request"),
        "fit-rejected initial-proof\n" => Some("initial-proof"),
        "fit-rejected screen\n" => Some("screen"),
        "fit-rejected rectangle\n" => Some("rectangle"),
        "fit-rejected settable\n" => Some("settable"),
        "fit-rejected identity-recheck\n" => Some("identity-recheck"),
        "fit-rejected pre-resize-identity\n" => Some("pre-resize-identity"),
        "fit-rejected resize-acknowledgement\n" => Some("resize-acknowledgement"),
        "fit-rejected pre-position-identity\n" => Some("pre-position-identity"),
        "fit-rejected allocation\n" => Some("allocation"),
        "fit-rejected size\n" => Some("size"),
        "fit-rejected position\n" => Some("position"),
        "fit-rejected postcondition\n" => Some("postcondition"),
        _ => None,
    }
}

pub(super) fn position_error(output: &str) -> Option<&'static str> {
    match output.strip_prefix("fit-rejected position ")? {
        "cannot-complete\n" => Some("cannot-complete"),
        "attribute-unsupported\n" => Some("attribute-unsupported"),
        "illegal-argument\n" => Some("illegal-argument"),
        "invalid-element\n" => Some("invalid-element"),
        "api-disabled\n" => Some("api-disabled"),
        "failure\n" => Some("failure"),
        "other\n" => Some("other"),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
pub(super) fn record(stage: &'static str) {
    record_result(stage, None);
}

#[cfg(target_os = "macos")]
pub(super) fn record_result(stage: &'static str, position_error: Option<&'static str>) {
    if !super::claude_focus_policy() {
        return;
    }
    let Some(directory) = std::env::var_os("NANH_DESKTOP_QUALIFICATION_FACTS") else {
        return;
    };
    let mut nonce = [0; 8];
    if getrandom::fill(&mut nonce).is_err() {
        return;
    }
    let path = std::path::PathBuf::from(directory).join(format!(
        "claude-window-fit-{}.json",
        u64::from_le_bytes(nonce)
    ));
    if let Ok(file) = nan_harness_private_fs::open_private_new(&path) {
        let mut value = serde_json::json!({"schemaVersion":1,"mechanism":"claude-window-fit",
            "diagnosticsOnly":true,"stage":stage});
        if let Some(error) = position_error {
            value["positionError"] = serde_json::json!(error);
        }
        let _ = serde_json::to_writer(file, &value);
    }
}

#[cfg(test)]
mod tests {
    use super::{parse, position_error};

    #[test]
    fn position_errors_are_closed_and_never_completion() {
        for error in [
            "cannot-complete",
            "attribute-unsupported",
            "illegal-argument",
            "invalid-element",
            "api-disabled",
            "failure",
            "other",
        ] {
            let output = format!("fit-rejected position {error}\n");
            assert_eq!(parse(&output), Some("position"));
            assert_eq!(position_error(&output), Some(error));
        }
        for output in [
            "fit-rejected position PRIVATE\n",
            "fit-rejected size cannot-complete\n",
            "fit-rejected position failure\nextra",
            "fit-rejected position true\n",
        ] {
            assert_eq!(parse(output), None);
            assert_eq!(position_error(output), None);
        }
        assert_eq!(position_error("fit-rejected position\n"), None);
    }

    #[test]
    fn rejected_fit_and_malformed_output_never_prove_completion() {
        assert_eq!(parse(""), Some("completed"));
        for stage in [
            "pre-resize-identity",
            "resize-acknowledgement",
            "pre-position-identity",
        ] {
            assert_eq!(parse(&format!("fit-rejected {stage}\n")), Some(stage));
            assert_eq!(parse(&format!("fit-rejected {stage}\nextra")), None);
        }
        assert_eq!(parse("fit-rejected size\n"), Some("size"));
        assert_eq!(parse("fit-rejected postcondition\n"), Some("postcondition"));
        for output in [
            "\n",
            "fit-rejected PRIVATE\n",
            "fit-rejected size",
            "fit-rejected size\nextra",
        ] {
            assert_eq!(parse(output), None);
        }
    }
}
