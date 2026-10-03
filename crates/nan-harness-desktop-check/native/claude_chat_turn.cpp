// Hosted-only ordinary native Chat actions; private payloads never enter stdout.
#ifdef __APPLE__
#import <AppKit/AppKit.h>
#import <ApplicationServices/ApplicationServices.h>
#include <algorithm>
#include <chrono>
#include <cmath>
#include <iostream>
#include <sstream>
#include <string>
#include <thread>
#include <vector>
#include <time.h>
#include <unistd.h>

bool claude_owned_mac_window(std::uint64_t, pid_t, CGRect);
namespace {
using Clock = std::chrono::steady_clock;
struct Request {
    std::string mode, prompt, marker, sentinel;
    std::uint64_t window = 0;
    unsigned pid = 0, millis = 0, owner = 0;
    std::uint64_t cutoff = 0;
    CGRect bounds = CGRectZero;
    Clock::time_point deadline;
    ~Request() { for (auto value : {&prompt, &marker, &sentinel}) std::fill(value->begin(), value->end(), '\0'); }
};
static bool decode(const std::string& hex, std::string& result) {
    if (hex == "-") return true;
    if (hex.size() > 2048 || hex.size() % 2) return false;
    auto digit = [](char ch) { return ch >= '0' && ch <= '9' ? ch - '0' : ch >= 'a' && ch <= 'f' ? ch - 'a' + 10 : -1; };
    for (std::size_t i = 0; i < hex.size(); i += 2) {
        int first = digit(hex[i]), second = digit(hex[i + 1]);
        if (first < 0 || second < 0 || !(first || second)) return false;
        result.push_back(static_cast<char>(first * 16 + second));
    }
    return true;
}
static bool input_mode(const std::string& mode) {
    return mode == "input" || mode == "input-replace-owned";
}
static bool request(Request& value) {
    std::string line, prompt, marker, sentinel, trailing;
    if (!std::getline(std::cin, line) || line.size() > 8192 || std::cin.peek() != EOF) return false;
    std::istringstream input(line);
    double x, y, width, height;
    if (!(input >> value.mode >> value.window >> value.pid >> x >> y >> width >> height >> value.millis >> value.cutoff >> value.owner >> prompt >> marker >> sentinel) || input >> trailing) return false;
    if (!value.cutoff || value.owner < 2 || !value.window || value.pid < 2 || !value.millis || value.millis > 5000 || !std::isfinite(x) || !std::isfinite(y)
        || !std::isfinite(width) || !std::isfinite(height) || width < 300 || height < 200) return false;
    if (!input_mode(value.mode) && value.mode != "copy" && value.mode != "retry-ready" && value.mode != "retry") return false;
    if (!decode(prompt, value.prompt) || !decode(marker, value.marker) || !decode(sentinel, value.sentinel) || value.sentinel.empty()) return false;
    value.bounds = CGRectMake(x, y, width, height);
    value.deadline = Clock::now() + std::chrono::milliseconds(value.millis);
    std::fill(line.begin(), line.end(), '\0');
    return true;
}
static bool within(const Request& request) {
    timespec ticks{};
    if (clock_gettime(CLOCK_MONOTONIC, &ticks) != 0 || ticks.tv_sec < 0 || getppid() != static_cast<pid_t>(request.owner)) return false;
    auto millis = static_cast<std::uint64_t>(ticks.tv_sec) * 1000 + static_cast<std::uint64_t>(ticks.tv_nsec) / 1000000;
    return millis < request.cutoff && Clock::now() < request.deadline;
}
static bool owned(const Request& request) {
    return within(request) && claude_owned_mac_window(request.window, request.pid, request.bounds) && within(request);
}
static std::string text(CFTypeRef value) {
    if (!value || CFGetTypeID(value) != CFStringGetTypeID()) return {};
    char buffer[4097];
    if (!CFStringGetCString(static_cast<CFStringRef>(value), buffer, sizeof(buffer), kCFStringEncodingUTF8)) return {};
    return buffer;
}
static bool ax_query_failed = false;
static CFTypeRef attribute(AXUIElementRef element, CFStringRef name) {
    CFTypeRef value = nullptr;
    AXError error = AXUIElementCopyAttributeValue(element, name, &value);
    if (error != kAXErrorSuccess && error != kAXErrorNoValue && error != kAXErrorAttributeUnsupported) ax_query_failed = true;
    return error == kAXErrorSuccess ? value : nullptr;
}
static std::string string_attribute(AXUIElementRef element, CFStringRef name) {
    auto value = attribute(element, name);
    auto result = text(value);
    if (value) CFRelease(value);
    return result;
}
static bool rectangle(AXUIElementRef element, CGRect& bounds) {
    auto position = attribute(element, kAXPositionAttribute), size = attribute(element, kAXSizeAttribute);
    CGPoint point; CGSize dimensions;
    bool valid = position && size && CFGetTypeID(position) == AXValueGetTypeID() && CFGetTypeID(size) == AXValueGetTypeID()
        && AXValueGetValue(static_cast<AXValueRef>(position), kAXValueTypeCGPoint, &point)
        && AXValueGetValue(static_cast<AXValueRef>(size), kAXValueTypeCGSize, &dimensions);
    if (position) CFRelease(position);
    if (size) CFRelease(size);
    if (!valid) return false;
    bounds = CGRectMake(point.x, point.y, dimensions.width, dimensions.height);
    return std::isfinite(point.x) && std::isfinite(point.y) && std::isfinite(dimensions.width)
        && std::isfinite(dimensions.height) && dimensions.width > 0 && dimensions.height > 0;
}
struct Node {
    AXUIElementRef element;
    int parent;
    std::string role, label, current;
    CGRect bounds = CGRectZero;
    bool enabled = false;
};
struct NodePropertyPlan {
    bool control_metadata;
    bool current_token;
};
static NodePropertyPlan node_property_plan(const std::string& role, const std::string& label) {
    // All target/retained callers select buttons or the source composer. Text
    // and grouping nodes contribute only role, label and ancestry to scope.
    return {role == "AXButton" || role == "AXTextArea", role == "AXButton" && label == "Chat"};
}
static const char* tree_node_failure(unsigned depth, std::size_t count, bool in_time,
                                     bool pid_read, bool pid_matches, bool duplicate) {
    if (depth > 32 || count >= 1024) return "tree-limit";
    if (!in_time) return "deadline";
    if (!pid_read || !pid_matches) return "tree-pid";
    if (duplicate) return "tree-duplicate";
    return nullptr;
}
// Chromium exposes heading names in Title or Description; heading Value is a level.
static std::string assistant_heading_label(const std::string& description, const std::string& title) {
    const auto source = [](const std::string& text) { return text.rfind("Claude responded:", 0) == 0; };
    if (source(description) && source(title) && description != title) return {};
    if (source(title)) return title;
    return description;
}
struct Tree {
    std::vector<Node> nodes;
    const char* failure = nullptr;
    bool reject(const char* stage) { if (!failure) failure = stage; return false; }
    ~Tree() { for (auto& node : nodes) { if (node.element) CFRelease(node.element); std::fill(node.label.begin(), node.label.end(), '\0'); } }
    bool append(AXUIElementRef element, int parent, unsigned depth, const Request& request) {
        if (depth > 32 || nodes.size() >= 1024) return reject("tree-limit");
        if (!within(request)) return reject("deadline");
        pid_t pid = 0;
        bool pid_read = AXUIElementGetPid(element, &pid) == kAXErrorSuccess;
        bool duplicate = std::any_of(nodes.begin(), nodes.end(), [element](const Node& prior) { return CFEqual(prior.element, element); });
        if (const char* stage = tree_node_failure(depth, nodes.size(), within(request), pid_read,
                pid == static_cast<pid_t>(request.pid), duplicate)) return reject(stage);
        if (AXUIElementSetMessagingTimeout(element, .1F) != kAXErrorSuccess) return reject("tree-query");
        Node node{static_cast<AXUIElementRef>(CFRetain(element)), parent};
        node.role = string_attribute(element, kAXRoleAttribute);
        node.label = string_attribute(element, kAXDescriptionAttribute);
        if (node.role == "AXHeading") {
            const auto title = string_attribute(element, kAXTitleAttribute);
            node.label = assistant_heading_label(node.label, title);
            if (node.label.empty() && title.rfind("Claude responded:", 0) != 0) node.label = title;
        } else if (node.label.empty()) node.label = string_attribute(element, kAXTitleAttribute);
        if (node.label.empty() && node.role != "AXHeading") node.label = string_attribute(element, kAXValueAttribute);
        const auto plan = node_property_plan(node.role, node.label);
        if (plan.current_token) node.current = string_attribute(element, CFSTR("AXARIACurrent"));
        if (plan.control_metadata) {
            auto enabled = attribute(element, kAXEnabledAttribute);
            node.enabled = enabled && CFGetTypeID(enabled) == CFBooleanGetTypeID() && CFBooleanGetValue(static_cast<CFBooleanRef>(enabled));
            if (enabled) CFRelease(enabled);
            rectangle(element, node.bounds);
        }
        int index = static_cast<int>(nodes.size());
        nodes.push_back(std::move(node));
        if (ax_query_failed) return reject("tree-query");
        auto children = attribute(element, kAXChildrenAttribute);
        if (ax_query_failed) {
            if (children) CFRelease(children);
            return reject("tree-query");
        }
        bool valid = !children || CFGetTypeID(children) == CFArrayGetTypeID();
        if (!valid) reject("tree-type");
        if (valid && children) {
            auto array = static_cast<CFArrayRef>(children);
            valid = CFArrayGetCount(array) <= 1024;
            if (!valid) reject("tree-limit");
            for (CFIndex i = 0; valid && i < CFArrayGetCount(array); ++i) {
                auto child = static_cast<AXUIElementRef>(const_cast<void*>(CFArrayGetValueAtIndex(array, i)));
                if (!child || CFGetTypeID(child) != AXUIElementGetTypeID()) valid = reject("tree-type");
                else valid = append(child, index, depth + 1, request);
            }
        }
        if (children) CFRelease(children);
        if (!within(request)) return reject("deadline");
        return valid;
    }
    bool collect(const Request& request) {
        ax_query_failed = false;
        failure = nullptr;
        if (!owned(request)) return reject("tree-window");
        auto app = AXUIElementCreateApplication(request.pid);
        if (!app) return reject("tree-query");
        if (AXUIElementSetMessagingTimeout(app, .1F) != kAXErrorSuccess) {
            CFRelease(app);
            return reject("tree-query");
        }
        auto window = attribute(app, kAXFocusedWindowAttribute);
        CFRelease(app);
        bool valid = false;
        if (ax_query_failed) reject("tree-query");
        else if (!window) reject("tree-focus");
        else if (CFGetTypeID(window) != AXUIElementGetTypeID()) reject("tree-type");
        else valid = append(static_cast<AXUIElementRef>(window), -1, 0, request);
        if (window) CFRelease(window);
        if (ax_query_failed) return reject("tree-query");
        if (!owned(request)) return reject("tree-window");
        return valid;
    }
};
static bool descendant(const Tree& tree, int index, int ancestor) {
    for (unsigned depth = 0; index >= 0 && depth <= 32; ++depth) {
        if (index == ancestor) return true;
        index = tree.nodes[index].parent;
    }
    return false;
}
static bool chat(const Tree& tree) {
    unsigned count = 0;
    for (std::size_t i = 0; i < tree.nodes.size(); ++i) {
        const auto& node = tree.nodes[i];
        if (node.role != "AXButton" || node.label != "Chat" || !node.enabled || node.current != "page") continue;
        int parent = node.parent;
        if (parent >= 0 && tree.nodes[parent].role == "AXGroup" && tree.nodes[parent].label == "Mode") ++count;
    }
    return count == 1;
}
static int unique(const Tree& tree, const std::string& role, const std::string& label, int ancestor = 0) {
    int result = -1;
    for (std::size_t i = 0; i < tree.nodes.size(); ++i) if (tree.nodes[i].role == role && tree.nodes[i].label == label && descendant(tree, i, ancestor)) {
        if (result >= 0) return -1;
        result = static_cast<int>(i);
    }
    return result;
}
static bool retained(const Tree& tree, const Node& node) {
    unsigned count = 0;
    for (const auto& fresh : tree.nodes) if (CFEqual(fresh.element, node.element) && fresh.role == node.role
        && fresh.label == node.label && fresh.enabled == node.enabled && CGRectEqualToRect(fresh.bounds, node.bounds)) ++count;
    return count == 1;
}
static bool contained_control(const Node& node, const Request& request) {
    return CGRectContainsRect(request.bounds, node.bounds) && node.bounds.size.width > 0 && node.bounds.size.height > 0;
}
static bool target(const Node& node, const Request& request) {
    return node.enabled && contained_control(node, request);
}
// Typing can enable the same source-defined button; identity and geometry cannot change.
static int enabled_submission(const Tree& tree, const Node& original, const Request& request) {
    int result = -1;
    unsigned candidates = 0;
    for (std::size_t i = 0; i < tree.nodes.size(); ++i) {
        const auto& node = tree.nodes[i];
        if (node.role != "AXButton" || (node.label != "Start task" && node.label != "Send message")) continue;
        ++candidates;
        if (node.element && original.element && CFEqual(node.element, original.element)
            && node.role == original.role && node.label == original.label
            && CGRectEqualToRect(node.bounds, original.bounds) && target(node, request)) result = static_cast<int>(i);
    }
    return candidates == 1 ? result : -1;
}
static bool clipboard_write(const std::string& value) {
    NSString* string = [[NSString alloc] initWithBytes:value.data() length:value.size() encoding:NSUTF8StringEncoding];
    if (!string) return false;
    NSPasteboard* board = [NSPasteboard generalPasteboard];
    [board clearContents];
    bool success = [board setString:string forType:NSPasteboardTypeString];
    [string release];
    return success;
}
static bool clipboard_matches(const std::string& expected) {
    NSString* string = [[NSPasteboard generalPasteboard] stringForType:NSPasteboardTypeString];
    NSData* bytes = [string dataUsingEncoding:NSUTF8StringEncoding];
    return bytes && bytes.length <= 65536 && bytes.length == expected.size()
        && std::equal(expected.begin(), expected.end(), static_cast<const char*>(bytes.bytes));
}
static bool key(CGKeyCode code, bool command) {
    CGEventRef down = CGEventCreateKeyboardEvent(nullptr, code, true), up = CGEventCreateKeyboardEvent(nullptr, code, false);
    if (!down || !up) { if (down) CFRelease(down); if (up) CFRelease(up); return false; }
    CGEventSetFlags(down, command ? kCGEventFlagMaskCommand : 0);
    CGEventSetFlags(up, 0);
    CGEventPost(kCGHIDEventTap, down); CGEventPost(kCGHIDEventTap, up);
    CFRelease(down); CFRelease(up);
    return true;
}
static const char* press(const Request& request, const Node& node, const char* success) {
    if (!target(node, request) || !owned(request)) return "control";
    Tree fresh;
    if (!fresh.collect(request) || !retained(fresh, node) || !owned(request)) return "control";
    if (AXUIElementPerformAction(node.element, kAXPressAction) != kAXErrorSuccess) return "action-uncertain";
    return owned(request) ? success : "action-uncertain";
}
// Classify the already-read value without another accessibility query.
static const char* initial_input_failure(CFTypeRef value, bool query_failed, bool replace_owned = false) {
    if (query_failed || !value || CFGetTypeID(value) != CFStringGetTypeID()) return "input-initial-unavailable";
    if (!replace_owned && CFStringGetLength(static_cast<CFStringRef>(value)) != 0) return "input-initial-nonempty";
    return nullptr;
}
static const char* input_readback_failure(bool clipboard_verified, bool value_verified) {
    if (!clipboard_verified) return "input-clipboard-mismatch";
    if (!value_verified) return "input-value-mismatch";
    return nullptr;
}
static bool focused_identity(AXUIElementRef focused, const Node& control, pid_t pid,
                             pid_t expected_pid, const std::string& role, CGRect bounds) {
    return focused && CFEqual(focused, control.element) && pid == expected_pid
        && role == control.role && CGRectEqualToRect(bounds, control.bounds);
}
static bool focused_composer(const Request& request, const Node& control) {
    if (!owned(request)) return false;
    AXUIElementRef app = AXUIElementCreateApplication(request.pid);
    if (!app) return false;
    if (AXUIElementSetMessagingTimeout(app, .1F) != kAXErrorSuccess) {
        CFRelease(app);
        return false;
    }
    auto value = attribute(app, kAXFocusedUIElementAttribute);
    CFRelease(app);
    bool matches = false;
    if (value && CFGetTypeID(value) == AXUIElementGetTypeID()) {
        auto focused = static_cast<AXUIElementRef>(value);
        pid_t pid = 0;
        CGRect bounds;
        matches = AXUIElementGetPid(focused, &pid) == kAXErrorSuccess && rectangle(focused, bounds)
            && focused_identity(focused, control, pid, request.pid,
                                string_attribute(focused, kAXRoleAttribute), bounds);
    }
    if (value) CFRelease(value);
    return matches && !ax_query_failed && owned(request);
}
static const char* input(const Request& request, const Tree& tree) {
    if (!chat(tree)) return "mode";
    int editor = unique(tree, "AXTextArea", "Write your prompt to Claude");
    int send = unique(tree, "AXButton", "Start task");
    if (send < 0) send = unique(tree, "AXButton", "Send message");
    if (editor < 0 || send < 0 || request.prompt.empty()) return "composer";
    const auto& control = tree.nodes[editor];
    if (!target(control, request) || !contained_control(tree.nodes[send], request)) return "control";
    auto initial_value = attribute(control.element, kAXValueAttribute);
    const char* initial_failure = initial_input_failure(initial_value, ax_query_failed, request.mode == "input-replace-owned");
    if (initial_value) CFRelease(initial_value);
    if (initial_failure) return initial_failure;
    if (!owned(request)) return "input-focus-guard";
    if (AXUIElementSetAttributeValue(control.element, kAXFocusedAttribute, kCFBooleanTrue) != kAXErrorSuccess)
        return "input-focus-setting";
    if (request.mode == "input-replace-owned") {
        if (!focused_composer(request, control)) return "input-focused-identity";
        if (!key(0, true)) return "input-replace-select-key";
    }
    if (!owned(request)) return "input-prompt-before-guard";
    if (!clipboard_write(request.prompt)) return "input-prompt-clipboard";
    if (!owned(request)) return "input-prompt-after-guard";
    if (request.mode == "input-replace-owned" && !focused_composer(request, control))
        return "input-focused-identity";
    if (!key(9, true)) return "input-paste-key";
    std::this_thread::sleep_for(std::chrono::milliseconds(100));
    if (!owned(request)) return "input-readback-before-guard";
    if (!clipboard_write(request.sentinel)) return "input-sentinel-clipboard";
    if (!owned(request)) return "input-sentinel-after-guard";
    if (!key(0, true)) return "input-readback-select-key";
    if (!owned(request)) return "input-readback-select-guard";
    if (!key(8, true)) return "input-readback-copy-key";
    while (within(request) && !clipboard_matches(request.prompt)) std::this_thread::sleep_for(std::chrono::milliseconds(20));
    bool copied = clipboard_matches(request.prompt);
    const char* readback_failure = input_readback_failure(copied,
        copied && string_attribute(control.element, kAXValueAttribute) == request.prompt);
    if (readback_failure) return readback_failure;
    if (!owned(request)) return "input-collapse-guard";
    if (!key(124, false)) return "input-collapse-key";
    Tree fresh;
    if (!fresh.collect(request) || !chat(fresh) || !retained(fresh, control)) return "control";
    int enabled_send = enabled_submission(fresh, tree.nodes[send], request);
    if (enabled_send < 0) return "control";
    return press(request, fresh.nodes[enabled_send], "sent");
}
static int scoped_control(const Tree& tree, const Request& request, bool retry, const char** failure = nullptr) {
    auto reject = [failure](const char* stage) { if (failure) *failure = stage; return -1; };
    std::vector<int> anchors;
    for (std::size_t i = 0; i < tree.nodes.size(); ++i) {
        const auto& node = tree.nodes[i];
        bool matches = retry ? node.label.find("NAN_CHECK_EXPECTED_FAILURE") != std::string::npos
            : node.role == "AXHeading" && node.label.rfind("Claude responded:", 0) == 0 && node.label.find(request.marker) != std::string::npos;
        if (matches) anchors.push_back(i);
    }
    if (anchors.empty()) {
        if (retry) return reject("scope-anchor-absent");
        unsigned headings = 0, assistant_headings = 0;
        for (const auto& node : tree.nodes) if (node.role == "AXHeading") {
            ++headings;
            if (node.label.rfind("Claude responded:", 0) == 0) ++assistant_headings;
        }
        return reject(headings == 0 ? "scope-heading-absent" : assistant_headings == 0
            ? "scope-assistant-heading-absent" : "scope-marker-heading-absent");
    }
    if (anchors.size() != 1) return reject("scope-anchor-ambiguous");
    bool ambiguous_control = false;
    bool mismatched_prompt = false;
    int ancestor = tree.nodes[anchors[0]].parent;
    for (unsigned depth = 0; ancestor >= 0 && depth < 6; ++depth, ancestor = tree.nodes[ancestor].parent) {
        unsigned controls = 0;
        for (std::size_t i = 0; i < tree.nodes.size(); ++i)
            if (descendant(tree, i, ancestor) && tree.nodes[i].role == "AXButton" && tree.nodes[i].label == (retry ? "Retry" : "Copy")) ++controls;
        if (controls > 1) ambiguous_control = true;
        int control = unique(tree, "AXButton", retry ? "Retry" : "Copy", ancestor);
        if (control < 0) continue;
        unsigned heading_count = 0;
        for (std::size_t i = 0; i < tree.nodes.size(); ++i) if (descendant(tree, i, ancestor) && tree.nodes[i].role == "AXHeading") ++heading_count;
        if (heading_count > 1) return reject("scope-heading-ambiguous");
        if (retry) {
            unsigned prompts = 0;
            for (std::size_t i = 0; i < tree.nodes.size(); ++i) if (descendant(tree, i, ancestor) && tree.nodes[i].label == request.prompt) ++prompts;
            if (prompts != 1) { mismatched_prompt = true; continue; }
        }
        return control;
    }
    return reject(mismatched_prompt ? "scope-prompt-mismatch" : ambiguous_control ? "scope-control-ambiguous" : "scope-control-absent");
}
static const char* action(const Request& request, const Tree& tree) {
    bool retry = request.mode != "copy";
    const char* failure = "scope";
    int index = scoped_control(tree, request, retry, &failure);
    if (index < 0) return failure;
    if (request.mode == "retry-ready") return target(tree.nodes[index], request) && owned(request) ? "retry-ready" : "control";
    if (!retry && !clipboard_write(request.sentinel)) return "response-mismatch";
    const char* result = press(request, tree.nodes[index], retry ? "retried" : "copied");
    if (std::string(result) != "copied") return result;
    while (within(request) && !clipboard_matches(request.marker)) std::this_thread::sleep_for(std::chrono::milliseconds(20));
    return clipboard_matches(request.marker) && owned(request) ? "copied" : "response-mismatch";
}
} // namespace
int claude_chat_turn() {
    @autoreleasepool {
        Request value;
        const char* stage = "request";
        if (request(value)) {
            Tree tree;
            if (!owned(value)) stage = "window";
            else if (!tree.collect(value)) stage = tree.failure ? tree.failure : "tree-query";
            else stage = input_mode(value.mode) ? input(value, tree) : action(value, tree);
            if (!within(value) && std::string(stage) != "action-uncertain") stage = "deadline";
        }
        std::cout << "turn " << stage << '\n';
        return std::cout ? 0 : 5;
    }
}
#else
int claude_chat_turn() { return 5; }
#endif
