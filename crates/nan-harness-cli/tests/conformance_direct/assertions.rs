use nan_harness_test_support::conformance::tool_result_failed;
use std::collections::BTreeSet;

#[test]
fn system_tool_errors_are_classified_as_failures() {
    assert!(tool_result_failed(
        "<system>ERROR: Tool execution failed.</system>\nThe file must be read first."
    ));
}

pub(super) fn assert_hermes_inventory(actual: &BTreeSet<String>) {
    const BASE_TOOLS: &[&str] = &[
        "clarify",
        "cronjob",
        "delegate_task",
        "execute_code",
        "memory",
        "patch",
        "process",
        "read_file",
        "search_files",
        "session_search",
        "skill_manage",
        "skill_view",
        "skills_list",
        "terminal",
        "text_to_speech",
        "todo",
        "write_file",
    ];
    const BROWSER_USE_TOOLS: &[&str] = &["browser_exec"];
    const NATIVE_BROWSER_TOOLS: &[&str] = &[
        "browser_back",
        "browser_click",
        "browser_console",
        "browser_get_images",
        "browser_navigate",
        "browser_press",
        "browser_scroll",
        "browser_snapshot",
        "browser_type",
    ];

    let base = BASE_TOOLS
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<BTreeSet<_>>();
    let mut variable = actual.difference(&base).cloned().collect::<BTreeSet<_>>();
    variable.remove("computer_use");
    let browser_use = BROWSER_USE_TOOLS
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<BTreeSet<_>>();
    let native_browser = NATIVE_BROWSER_TOOLS
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<BTreeSet<_>>();

    assert!(
        base.is_subset(actual),
        "missing Hermes base tools: {actual:?}"
    );
    assert!(
        variable == browser_use || variable == native_browser,
        "unexpected Hermes browser tool surface: {variable:?}"
    );
}
