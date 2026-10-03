// Pure source-bound selectors: never issue AX, clipboard, window or input calls.
#include "../claude_chat_turn.cpp"
#include <cassert>
bool claude_owned_mac_window(std::uint64_t, pid_t, CGRect) { return false; }
static Node fixture(int parent, const char* role, const char* label, const char* current = "") {
    return {nullptr, parent, role, label, current, CGRectZero, true};
}
int main() {
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
    Request request;
    request.prompt = "fresh user";
    request.marker = "fresh assistant";
    Tree tree;
    tree.nodes = {fixture(-1, "AXWindow", ""), fixture(0, "AXGroup", "Mode"),
        fixture(1, "AXButton", "Chat", "page"), fixture(0, "AXGroup", ""),
        fixture(3, "AXHeading", "Claude responded: fresh assistant"), fixture(3, "AXButton", "Copy")};
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
    tree.nodes[2].current = "false";
    assert(!chat(tree));
}
