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
const FAILURES: [&str; 29] = [
    "query",
    "deadline",
    "identity",
    "foreground",
    "bounds",
    "visibility",
    "display",
    "limit",
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
    if words.len() != 9 || words[0] != "uia" {
        return None;
    }
    let status = words[1];
    if status != "observed" {
        return (FAILURES.contains(&status) && words[2..].iter().all(|word| *word == "-"))
            .then(|| failure(status));
    }
    let numbers = words[2..]
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
    Some(value)
}
#[cfg(test)]
mod tests {
    use super::*;
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
