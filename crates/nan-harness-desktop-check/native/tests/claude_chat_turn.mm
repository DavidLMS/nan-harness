// Pure source-bound selectors: never issue AX, clipboard, window or input calls.
#include "../claude_chat_turn.cpp"
#include <cassert>
bool claude_owned_mac_window(std::uint64_t, pid_t, CGRect) { return false; }
static Node fixture(int parent, const char* role, const char* label, const char* current = "") {
    return {nullptr, parent, role, label, current, CGRectZero, true};
}
int main() {
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
    tree.nodes[2].current = "false";
    assert(!chat(tree));
}
