// Pure source-bound selectors: never issue AX, clipboard, window or input calls.
#include "../claude_chat_turn.cpp"
#include <cassert>
bool claude_owned_mac_window(std::uint64_t, pid_t, CGRect) { return false; }
static Node fixture(int parent, const char* role, const char* label, const char* current = "") {
    return {nullptr, parent, role, label, current, CGRectZero, true};
}
int main() {
    assert(pasted_value_state("","prompt",true,true)==PastedValue::Pending);
    assert(pasted_value_state("pro","prompt",true,true)==PastedValue::Pending);
    assert(pasted_value_state("prompt","prompt",true,true)==PastedValue::Ready);
    assert(pasted_value_state("foreign","prompt",true,true)==PastedValue::Rejected);
    assert(pasted_value_state("previous","prompt",true,true,"previous")==PastedValue::Pending);
    assert(pasted_value_state("other","prompt",true,true,"previous")==PastedValue::Rejected);
    assert(pasted_value_state("previous","prompt",true,false,"previous")==PastedValue::Rejected);
    assert(pasted_value_state("prompt","prompt",true,true,"previous")==PastedValue::Ready);
    assert(pasted_value_state("prompt","prompt",false,true)==PastedValue::Rejected);
    assert(pasted_value_state("prompt","prompt",true,false)==PastedValue::Rejected);
    assert(pasted_value_state(std::string(1025,'x'),"prompt",true,true)==PastedValue::Rejected);
    unsigned value_queries=0, readback_keys=0, value_pauses=0;
    if (settle_pasted_value([&] { assert(readback_keys==0); return ++value_queries==1
            ? PastedValue::Pending : PastedValue::Ready; }, [] { return true; },
            [&] { ++value_pauses; })) ++readback_keys;
    assert(value_queries==2 && value_pauses==1 && readback_keys==1);
    value_queries=readback_keys=value_pauses=0;
    unsigned value_deadline_checks=0;
    if (settle_pasted_value([&] { ++value_queries; return PastedValue::Ready; },
            [&] { return ++value_deadline_checks==1; }, [&] { ++value_pauses; })) ++readback_keys;
    assert(value_queries==1 && readback_keys==0 && value_pauses==0);
    value_queries=readback_keys=value_pauses=0;
    if (settle_pasted_value([&] { ++value_queries; return PastedValue::Rejected; },
            [] { return true; }, [&] { ++value_pauses; })) ++readback_keys;
    assert(value_queries==1 && readback_keys==0 && value_pauses==0);

    Request diagnostic;
    for (const char* phase : {"deadline-window", "deadline-tree", "deadline-focus", "deadline-input",
            "deadline-press", "deadline-copy", "deadline-retry-ready", "deadline-retry"}) {
        diagnostic.deadline_phase=phase;
        assert(std::string(deadline_result(diagnostic,"control",true))==phase);
        assert(std::string(deadline_result(diagnostic,"action-uncertain",true))=="action-uncertain");
        assert(std::string(deadline_result(diagnostic,"copied",false))=="copied");
    }

    const auto parsed_budget = [](unsigned millis, const char* suffix = "") {
        std::istringstream frame("input 7 8 0 0 800 600 " + std::to_string(millis) + " 1000 9 70 - 73\n" + suffix);
        auto* original = std::cin.rdbuf(frame.rdbuf());
        std::cin.clear();
        Request parsed;
        const bool valid = request(parsed);
        std::cin.rdbuf(original);
        std::cin.clear();
        if (valid) {
            assert(parsed.millis == millis && parsed.cutoff == 1000);
            // A larger upfront relative cap never renews an exhausted absolute cutoff.
            parsed.owner = static_cast<unsigned>(getppid());
            parsed.cutoff = 1;
            assert(!within(parsed));
            parsed.cutoff = UINT64_MAX;
            parsed.deadline = Clock::now() - std::chrono::milliseconds(1);
            assert(!within(parsed));
        }
        return valid;
    };
    assert(parsed_budget(1));
    assert(parsed_budget(5000));
    assert(parsed_budget(15000));
    assert(!parsed_budget(0));
    assert(!parsed_budget(15001));
    assert(!parsed_budget(15000, "trailing"));

    assert(assistant_heading_label("generic description", "Claude responded: fixture") == "Claude responded: fixture");
    assert(assistant_heading_label("Claude responded: fixture", "") == "Claude responded: fixture");
    assert(assistant_heading_label("Claude responded: fixture", "Claude responded: other").empty());
    assert(assistant_heading_label("generic description", "other heading") == "generic description");

    assert(initial_input_failure(nullptr, false) == std::string("input-initial-unavailable"));
    assert(initial_input_failure(kCFBooleanTrue, false) == std::string("input-initial-unavailable"));
    assert(initial_input_failure(CFSTR(""), true) == std::string("input-initial-unavailable"));
    assert(initial_input_failure(CFSTR("private existing input"), false) == std::string("input-initial-nonempty"));
    assert(initial_input_failure(CFSTR(""), false) == nullptr);
    assert(input_readback_failure(false, false) == std::string("input-clipboard-mismatch"));
    assert(input_readback_failure(false, true) == std::string("input-clipboard-mismatch"));
    assert(input_readback_failure(true, false) == std::string("input-value-mismatch"));
    assert(input_readback_failure(true, true) == nullptr);
    assert(input_mode("input"));
    assert(input_mode("input-replace-owned"));
    assert(!input_mode("replace"));
    assert(initial_input_failure(CFSTR("existing"), false, true) == nullptr);
    assert(initial_input_failure(nullptr, false, true) == std::string("input-initial-unavailable"));
    assert(initial_input_failure(kCFBooleanTrue, false, true) == std::string("input-initial-unavailable"));
    assert(initial_input_failure(CFSTR("existing"), true, true) == std::string("input-initial-unavailable"));
    Node focus_control = fixture(0, "AXTextArea", "Write your prompt to Claude");
    focus_control.element = reinterpret_cast<AXUIElementRef>(CFSTR("retained-composer"));
    focus_control.bounds = CGRectMake(10, 20, 30, 40);
    assert(focused_identity(focus_control.element, focus_control, 7, 7, "AXTextArea", focus_control.bounds));
    assert(!focused_identity(reinterpret_cast<AXUIElementRef>(CFSTR("other")), focus_control, 7, 7, "AXTextArea", focus_control.bounds));
    assert(!focused_identity(focus_control.element, focus_control, 8, 7, "AXTextArea", focus_control.bounds));
    assert(!focused_identity(focus_control.element, focus_control, 7, 7, "AXButton", focus_control.bounds));
    assert(!focused_identity(focus_control.element, focus_control, 7, 7, "AXTextArea", CGRectZero));
    Request focus_request;
    focus_request.pid = 7;
    focus_request.bounds = CGRectMake(0, 0, 100, 100);
    FocusIdentity focused{focus_control.element, 7, "AXTextArea", focus_control.bounds};
    assert(composer_focus_identity(focused, focus_control, focus_request, true, false) == ComposerFocus::Focused);
    auto previous = focused;
    previous.element = reinterpret_cast<AXUIElementRef>(CFSTR("previous-copy-control"));
    previous.role = "AXButton";
    previous.bounds = CGRectMake(60, 60, 10, 10);
    assert(composer_focus_identity(previous, focus_control, focus_request, true, false) == ComposerFocus::PendingIdentity);
    assert(composer_focus_identity(previous, focus_control, focus_request, false, false) == ComposerFocus::Rejected);
    assert(composer_focus_identity(previous, focus_control, focus_request, true, true) == ComposerFocus::Rejected);
    auto replaced = focused;
    replaced.element = previous.element;
    assert(composer_focus_identity(replaced, focus_control, focus_request, true, false) == ComposerFocus::PendingIdentity);
    assert(composer_focus_identity(replaced, focus_control, focus_request, false, false) == ComposerFocus::Rejected);
    auto moved = focused;
    moved.bounds.origin.x += 1;
    assert(composer_focus_identity(moved, focus_control, focus_request, true, false) == ComposerFocus::Rejected);
    auto foreign = previous;
    foreign.pid = 8;
    assert(composer_focus_identity(foreign, focus_control, focus_request, true, false) == ComposerFocus::Rejected);
    auto outside = previous;
    outside.bounds.origin.x = 1000;
    assert(composer_focus_identity(outside, focus_control, focus_request, true, false) == ComposerFocus::Rejected);
    auto missing = previous;
    missing.element = nullptr;
    assert(composer_focus_identity(missing, focus_control, focus_request, true, false) == ComposerFocus::Rejected);
    unsigned queries = 0, pauses = 0, keys = 0;
    assert(!settle_composer_focus([&] { ++queries; return ComposerFocus::Focused; },
        [] { return false; }, [&] { ++pauses; }));
    assert(queries == 0 && pauses == 0 && keys == 0);
    const bool settled = settle_composer_focus([&] {
        ++queries;
        assert(keys == 0);
        return composer_focus_identity(queries == 1 ? replaced : focused,
            focus_control, focus_request, true, false);
    }, [] { return true; }, [&] { ++pauses; });
    if (settled) ++keys;
    assert(queries == 2 && pauses == 1 && keys == 1);
    queries = pauses = keys = 0;
    const bool rejected = settle_composer_focus([&] {
        ++queries; return ComposerFocus::Rejected;
    }, [] { return true; }, [&] { ++pauses; });
    if (rejected) ++keys;
    assert(queries == 1 && pauses == 0 && keys == 0);
    unsigned checks = 0;
    assert(!settle_composer_focus([&] { ++queries; return ComposerFocus::Focused; },
        [&] { return ++checks == 1; }, [&] { ++pauses; }));
    assert(queries == 2 && pauses == 0 && keys == 0);
    checks = queries = pauses = 0;
    assert(!settle_composer_focus([&] { ++queries; return ComposerFocus::PendingIdentity; },
        [&] { return ++checks <= 2; }, [&] { ++pauses; }));
    assert(queries == 1 && pauses == 1);

    assert(tree_node_failure(33, 0, true, true, true, false) == std::string("tree-limit"));
    assert(tree_node_failure(0, 1024, true, true, true, false) == std::string("tree-limit"));
    assert(tree_node_failure(0, 0, false, true, true, false) == std::string("deadline"));
    assert(tree_node_failure(0, 0, true, false, true, false) == std::string("tree-pid"));
    assert(tree_node_failure(0, 0, true, true, false, false) == std::string("tree-pid"));
    assert(tree_node_failure(0, 0, true, true, true, true) == std::string("tree-duplicate"));
    assert(tree_node_failure(0, 0, true, true, true, false) == nullptr);
    Tree failed_tree;
    assert(!failed_tree.reject("tree-type"));
    assert(!failed_tree.reject("tree-query"));
    assert(std::string(failed_tree.failure) == "tree-type");
    ax_query_failed = true;
    for (const char* terminal : {"tree-pid", "tree-type", "tree-limit", "tree-window"}) {
        Tree terminal_tree;
        assert(!terminal_tree.reject(terminal));
        assert(std::string(terminal_tree.failure) == terminal);
    }
    ax_query_failed = false;
    for (const char* role : {"AXGroup", "AXHeading", "AXStaticText", "AXWindow"}) {
        auto plan = node_property_plan(role, "Chat");
        assert(!plan.control_metadata && !plan.current_token);
    }
    for (const char* name : {"Copy", "Retry", "Start task", "Send message"}) {
        auto plan = node_property_plan("AXButton", name);
        assert(plan.control_metadata && !plan.current_token);
    }
    auto mode_plan = node_property_plan("AXButton", "Chat");
    assert(mode_plan.control_metadata && mode_plan.current_token);
    auto editor_plan = node_property_plan("AXTextArea", "Write your prompt to Claude");
    assert(editor_plan.control_metadata && !editor_plan.current_token);
    Request request;
    request.prompt = "fresh user";
    request.marker = "fresh assistant";
    Tree tree;
    tree.nodes = {fixture(-1, "AXWindow", ""), fixture(0, "AXGroup", "Mode"),
        fixture(1, "AXButton", "Chat", "page"), fixture(0, "AXGroup", ""),
        fixture(3, "AXHeading", "Claude responded: fresh assistant"), fixture(3, "AXButton", "Copy")};
    for (auto& node : tree.nodes) {
        auto plan = node_property_plan(node.role, node.label);
        if (!plan.control_metadata) { node.enabled = false; node.bounds = CGRectZero; }
        if (!plan.current_token) node.current.clear();
    }
    assert(chat(tree));
    assert(scoped_control(tree, request, false) == 5);
    tree.nodes.push_back(fixture(3, "AXButton", "Copy"));
    assert(scoped_control(tree, request, false) == -1);
    tree.nodes.pop_back();
    tree.nodes[4].label = "fresh assistant";
    assert(scoped_control(tree, request, false) == -1);
    tree.nodes[4] = fixture(3, "AXStaticText", "NAN_CHECK_EXPECTED_FAILURE");
    tree.nodes[5].label = "Retry";
    assert(scoped_control(tree, request, true) == -1);
    tree.nodes.push_back(fixture(3, "AXStaticText", "fresh user"));
    assert(scoped_control(tree, request, true) == 5);
    tree.nodes.push_back(fixture(3, "AXStaticText", "fresh user"));
    assert(scoped_control(tree, request, true) == -1);
    const char* scope_failure = nullptr;
    assert(scoped_control(tree, request, true, &scope_failure) == -1);
    assert(std::string(scope_failure) == "scope-prompt-mismatch");
    tree.nodes.pop_back();
    tree.nodes[5].label = "unrelated";
    assert(scoped_control(tree, request, true, &scope_failure) == -1);
    assert(std::string(scope_failure) == "scope-control-absent");
    tree.nodes[5].label = "Retry";
    tree.nodes.push_back(fixture(3, "AXHeading", "other heading"));
    tree.nodes.push_back(fixture(3, "AXHeading", "second heading"));
    assert(scoped_control(tree, request, true, &scope_failure) == -1);
    assert(std::string(scope_failure) == "scope-heading-ambiguous");
    tree.nodes[4].label = "unrelated";
    assert(scoped_control(tree, request, true, &scope_failure) == -1);
    assert(std::string(scope_failure) == "scope-anchor-absent");
    tree.nodes[4].label = "NAN_CHECK_EXPECTED_FAILURE";
    tree.nodes.push_back(fixture(3, "AXStaticText", "NAN_CHECK_EXPECTED_FAILURE"));
    assert(scoped_control(tree, request, true, &scope_failure) == -1);
    assert(std::string(scope_failure) == "scope-anchor-ambiguous");
    Request details_request;
    details_request.prompt = "fresh user";
    details_request.bounds = CGRectMake(0, 0, 800, 600);
    Tree details_tree;
    details_tree.nodes = {fixture(-1, "AXWindow", ""), fixture(0, "AXGroup", ""),
        fixture(1, "AXStaticText", "Server error"), fixture(1, "AXStaticText", "fresh user"),
        fixture(1, "AXButton", "Retry"), fixture(1, "AXButton", "View details"),
        fixture(1, "AXHeading", "You said: fresh user")};
    for (auto& node : details_tree.nodes) { node.enabled = true; node.bounds = CGRectMake(20, 20, 30, 30); }
    assert(scoped_control(details_tree, details_request, true) == -1);
    assert(failure_details_control(details_tree, details_request) == 5);
    details_tree.nodes[3].label = "different user";
    assert(failure_details_control(details_tree, details_request) == -1);
    details_tree.nodes[3].label = "fresh user";
    details_tree.nodes[3].parent = 0;
    assert(failure_details_control(details_tree, details_request) == -1);
    details_tree.nodes[3].parent = 1;
    for (const char* label : {"Server error", "Retry", "View details", "fresh user"}) {
        details_tree.nodes.push_back(fixture(1, label == std::string("Retry") || label == std::string("View details") ? "AXButton" : "AXStaticText", label));
        assert(failure_details_control(details_tree, details_request) == -1);
        details_tree.nodes.pop_back();
    }
    details_tree.nodes[5].enabled = false;
    assert(failure_details_control(details_tree, details_request) == -1);
    details_tree.nodes[5].enabled = true;
    details_tree.nodes[6].label = "You said: different user";
    assert(failure_details_control(details_tree, details_request) == -1);
    details_tree.nodes[6].label = "You said: fresh user";
    for (const char* role : {"AXWebArea", "AXScrollArea", "AXWindow"}) {
        details_tree.nodes[1].role = role;
        assert(failure_details_control(details_tree, details_request) == -1);
    }
    details_tree.nodes[1].role = "AXGroup";
    details_tree.nodes.push_back(fixture(1, "AXHeading", "Claude responded: previous turn"));
    assert(failure_details_control(details_tree, details_request) == -1);
    Tree response_scope;
    const char* response_failure = nullptr;
    assert(scoped_control(response_scope, request, false, &response_failure) == -1);
    assert(std::string(response_failure) == "scope-heading-absent");
    response_scope.nodes.push_back(fixture(-1, "AXHeading", "You said: fresh assistant"));
    assert(scoped_control(response_scope, request, false, &response_failure) == -1);
    assert(std::string(response_failure) == "scope-assistant-heading-absent");
    response_scope.nodes[0].label = "Claude responded: stale assistant";
    response_scope.nodes.push_back(fixture(0, "AXStaticText", "fresh assistant"));
    assert(scoped_control(response_scope, request, false, &response_failure) == -1);
    assert(std::string(response_failure) == "scope-marker-heading-absent");
    Request submission_request;
    submission_request.bounds = CGRectMake(0, 0, 800, 600);
    auto identity = reinterpret_cast<AXUIElementRef>(CFSTR("retained-control"));
    Node initial = {identity, 0, "AXButton", "Start task", "", CGRectMake(20, 20, 100, 40), false};
    Tree submission;
    submission.nodes.push_back(initial);
    CFRetain(identity);
    assert(enabled_submission(submission, initial, submission_request) == -1);
    submission.nodes[0].enabled = true;
    assert(enabled_submission(submission, initial, submission_request) == 0);
    submission.nodes.push_back(submission.nodes[0]);
    CFRetain(identity);
    assert(enabled_submission(submission, initial, submission_request) == -1);
    CFRelease(submission.nodes.back().element);
    submission.nodes.pop_back();
    CFRelease(submission.nodes[0].element);
    submission.nodes[0].element = reinterpret_cast<AXUIElementRef>(CFRetain(CFSTR("replacement")));
    assert(enabled_submission(submission, initial, submission_request) == -1);
    tree.nodes[2].enabled = false;
    assert(!chat(tree));
    tree.nodes[2].enabled = true;
    tree.nodes[2].current = "false";
    assert(!chat(tree));
}
