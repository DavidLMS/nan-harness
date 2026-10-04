//! Passive UIA counts; no input or acceptance authority.
const COUNT_KEYS: [&str; 7] = [
    "nodeCount",
    "classicEditorCount",
    "modernEditorCount",
    "sendControlCount",
    "startTaskControlCount",
    "assistantHeadingCount",
    "copyControlCount",
];
const FAILURES: [&str; 35] = [
    "query",
    "deadline",
    "identity",
    "foreground",
    "bounds",
    "visibility",
    "display",
    "limit",
    "limit-depth",
    "limit-nodes",
    "limit-name",
    "limit-text",
    "limit-windows",
    "limit-processes",
    "occlusion",
    "duplicate",
    "element-identity",
    "root-process-query",
    "root-process-mismatch",
    "root-process-zero",
    "root-process-invalid",
    "descendant-process-query",
    "descendant-process-mismatch",
    "descendant-process-zero",
    "descendant-process-invalid",
    "owned-descendant-process",
    "foreign-descendant-process",
    "descendant-correlation-unavailable",
    "heading-property",
    "com",
    "root-replaced",
    "transport",
    "protocol",
    "policy",
    "directory",
];
pub(super) fn failure(status: &str) -> serde_json::Value {
    let mut value = serde_json::json!({"schemaVersion":1,"mechanism":"claude-windows-uia","diagnosticsOnly":true,"phase":"post-ready","status":status,"nativeGuardVerified":false,"treeComplete":false});
    for key in COUNT_KEYS {
        value[key] = serde_json::Value::Null;
    }
    value
}
pub(super) fn parse(wire: &str) -> Option<serde_json::Value> {
    let line = wire.strip_suffix('\n')?;
    if line.contains(['\n', '\r']) {
        return None;
    }
    let words = line.split(' ').collect::<Vec<_>>();
    if !matches!(words.len(), 9 | 16 | 24) || words[0] != "uia" {
        return None;
    }
    let status = words[1];
    if status != "observed" {
        return (words.len() == 9
            && FAILURES.contains(&status)
            && words[2..].iter().all(|word| *word == "-"))
        .then(|| failure(status));
    }
    let numbers = words[2..9]
        .iter()
        .map(|word| {
            (!word.is_empty() && word.bytes().all(|byte| byte.is_ascii_digit()))
                .then(|| word.parse::<usize>().ok())
                .flatten()
        })
        .collect::<Option<Vec<_>>>()?;
    if numbers[0] == 0 || numbers[0] > 1024 || numbers.iter().any(|count| *count > numbers[0]) {
        return None;
    }
    let mut value = failure("observed");
    value["nativeGuardVerified"] = true.into();
    value["treeComplete"] = true.into();
    for (key, count) in COUNT_KEYS.into_iter().zip(numbers) {
        value[key] = count.into();
    }
    if words.len() >= 16 {
        value["currentMode"] = parse_mode(&words[9..16], numbers_node_count(&value)?)?;
    }
    if words.len() == 24 {
        value["chatCapability"] = parse_capability(&words[16..], &value)?;
    }
    Some(value)
}
fn parse_capability(words: &[&str], inventory: &serde_json::Value) -> Option<serde_json::Value> {
    const KEYS: [&str; 6] = [
        "valuePattern",
        "valueReadOnly",
        "valueEmpty",
        "password",
        "keyboardFocusable",
        "startTaskInvokePattern",
    ];
    if words.len() != 8
        || words[0] != "capability"
        || !["observed", "missing", "ambiguous", "unavailable", "changed"].contains(&words[1])
    {
        return None;
    }
    let observed = words[1] == "observed";
    let mut value = serde_json::json!({"status":words[1]});
    for (key, word) in KEYS.into_iter().zip(&words[2..]) {
        value[key] = match *word {
            "-" => serde_json::Value::Null,
            "0" if observed => false.into(),
            "1" if observed => true.into(),
            _ => return None,
        };
    }
    if observed {
        if inventory["currentMode"]["status"] != "chat"
            || inventory["classicEditorCount"] != 1
            || inventory["startTaskControlCount"] != 1
            || [
                "valuePattern",
                "password",
                "keyboardFocusable",
                "startTaskInvokePattern",
            ]
            .iter()
            .any(|key| !value[key].is_boolean())
        {
            return None;
        }
        let readable = value["valuePattern"] == true;
        if ["valueReadOnly", "valueEmpty"]
            .iter()
            .any(|key| value[key].is_boolean() != readable)
        {
            return None;
        }
    }
    Some(value)
}
fn numbers_node_count(value: &serde_json::Value) -> Option<usize> {
    value["nodeCount"]
        .as_u64()
        .and_then(|count| usize::try_from(count).ok())
}
fn parse_mode(words: &[&str], node_count: usize) -> Option<serde_json::Value> {
    const KEYS: [&str; 5] = [
        "modeGroupCount",
        "chatCount",
        "coworkCount",
        "currentChatCount",
        "currentCoworkCount",
    ];
    if words.len() != 7
        || words[0] != "mode"
        || ![
            "chat",
            "cowork",
            "missing",
            "ambiguous",
            "unavailable",
            "changed",
        ]
        .contains(&words[1])
    {
        return None;
    }
    let status = words[1];
    let mut value = serde_json::json!({"status": status});
    if matches!(status, "unavailable" | "changed") {
        if !words[2..].iter().all(|word| *word == "-") {
            return None;
        }
        for key in KEYS {
            value[key] = serde_json::Value::Null;
        }
        return Some(value);
    }
    let counts = words[2..]
        .iter()
        .map(|word| {
            (!word.is_empty() && word.bytes().all(|byte| byte.is_ascii_digit()))
                .then(|| word.parse::<usize>().ok())
                .flatten()
        })
        .collect::<Option<Vec<_>>>()?;
    if counts.iter().any(|count| *count > node_count)
        || counts[3] > counts[1]
        || counts[4] > counts[2]
    {
        return None;
    }
    let expected = if counts[0] > 1 || counts[1] > 1 || counts[2] > 1 || counts[3] + counts[4] > 1 {
        "ambiguous"
    } else if counts[0] != 1 || counts[1] + counts[2] == 0 || counts[3] + counts[4] != 1 {
        "missing"
    } else if counts[3] == 1 {
        "chat"
    } else {
        "cowork"
    };
    let legacy_missing = status == "missing"
        && matches!(expected, "chat" | "cowork")
        && (counts[1] == 0 || counts[2] == 0);
    if status != expected && !legacy_missing {
        return None;
    }
    for (key, count) in KEYS.into_iter().zip(counts) {
        value[key] = count.into();
    }
    Some(value)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chat_only_and_passive_capabilities_preserve_legacy_protocol() {
        for mode in ["chat", "missing"] {
            assert!(
                parse(&format!(
                    "uia observed 20 1 0 0 1 0 0 mode {mode} 1 1 0 1 0\n"
                ))
                .is_some()
            );
        }
        let prefix = "uia observed 20 1 0 0 1 0 0 mode chat 1 1 0 1 0";
        let value = parse(&format!("{prefix} capability observed 1 0 1 0 1 1\n")).unwrap();
        assert_eq!(value["chatCapability"]["valueEmpty"], true);
        assert!(parse(&format!("{prefix} capability observed 0 - - 0 1 1\n")).is_some());
        assert!(parse(&format!("{prefix} capability unavailable - - - - - -\n")).is_some());
        for wire in [
            "observed 0 0 1 0 1 1",
            "observed 1 - 1 0 1 1",
            "changed 1 0 1 0 1 1",
            "observed 1 0 1 0 1 PRIVATE",
        ] {
            assert!(parse(&format!("{prefix} capability {wire}\n")).is_none());
        }
    }
    #[test]
    fn current_mode_is_optional_closed_and_never_partial() {
        let value = parse("uia observed 20 1 0 0 1 0 0 mode chat 1 1 1 1 0\n").unwrap();
        assert_eq!(value["currentMode"]["status"], "chat");
        assert!(
            parse("uia observed 20 1 0 0 1 0 0\n")
                .unwrap()
                .get("currentMode")
                .is_none()
        );
        for status in ["unavailable", "changed"] {
            assert!(
                parse(&format!(
                    "uia observed 20 1 0 0 1 0 0 mode {status} - - - - -\n"
                ))
                .unwrap()["currentMode"]["chatCount"]
                    .is_null()
            );
        }
        for suffix in [
            "mode PRIVATE 1 1 1 1 0",
            "mode chat 1 1 1 1 1",
            "mode changed 1 1 1 1 0",
            "mode chat 1 21 1 1 0",
            "mode chat 1 1 1 true 0",
        ] {
            assert!(parse(&format!("uia observed 20 1 0 0 1 0 0 {suffix}\n")).is_none());
        }
    }
    #[test]
    fn accepts_only_complete_closed_protocol() {
        assert_eq!(
            parse("uia observed 12 1 0 1 0 2 1\n").unwrap()["classicEditorCount"],
            1
        );
        let value = parse("uia element-identity - - - - - - -\n").unwrap();
        assert_eq!(value["nativeGuardVerified"], false);
        assert!(value["nodeCount"].is_null());
        for status in [
            "root-process-query",
            "root-process-mismatch",
            "root-process-zero",
            "root-process-invalid",
            "descendant-process-query",
            "descendant-process-mismatch",
            "descendant-process-zero",
            "descendant-process-invalid",
            "owned-descendant-process",
            "foreign-descendant-process",
            "descendant-correlation-unavailable",
        ] {
            let wire = format!("uia {status} - - - - - - -\n");
            let value = parse(&wire).unwrap();
            assert_eq!(value["status"], status);
            assert_eq!(value["treeComplete"], false);
            assert!(value["nodeCount"].is_null());
            assert!(parse(&wire.replace("- -", "1 -")).is_none());
        }
        for invalid in [
            "uia observed 0 0 0 0 0 0 0\n",
            "uia observed 1 2 0 0 0 0 0\n",
            "uia observed 1025 0 0 0 0 0 0\n",
            "uia PRIVATE - - - - - - -\n",
            "uia query 1 - - - - - -\n",
            "uia observed 1 0 0 0 0 0 0\nPRIVATE\n",
            "uia observed 1 -1 0 0 0 0 0\n",
        ] {
            assert!(parse(invalid).is_none());
        }
    }
}
