//! Closed stages of the one-time owned AX fit; failures never authorize another fit.

pub(super) fn parse(output: &str) -> Option<&'static str> {
    match output {
        "" => Some("completed"),
        "fit-rejected request\n" => Some("request"),
        "fit-rejected initial-proof\n" => Some("initial-proof"),
        "fit-rejected screen\n" => Some("screen"),
        "fit-rejected rectangle\n" => Some("rectangle"),
        "fit-rejected settable\n" => Some("settable"),
        "fit-rejected identity-recheck\n" => Some("identity-recheck"),
        "fit-rejected allocation\n" => Some("allocation"),
        "fit-rejected size\n" => Some("size"),
        "fit-rejected position\n" => Some("position"),
        "fit-rejected postcondition\n" => Some("postcondition"),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
pub(super) fn record(stage: &'static str) {
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
        let value = serde_json::json!({"schemaVersion":1,"mechanism":"claude-window-fit",
            "diagnosticsOnly":true,"stage":stage});
        let _ = serde_json::to_writer(file, &value);
    }
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn rejected_fit_and_malformed_output_never_prove_completion() {
        assert_eq!(parse(""), Some("completed"));
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
