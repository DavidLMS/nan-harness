// Hosted-only ordinary native Chat actions; private payloads never enter stdout.
#ifdef __APPLE__
#import <AppKit/AppKit.h>
#import <ApplicationServices/ApplicationServices.h>
#include <algorithm>
#include <array>
#include <chrono>
#include <cmath>
#include <iostream>
#include <sstream>
#include <string>
#include <thread>
#include <vector>
#include <time.h>
#include <unistd.h>

#include "mac_owned_window.hpp"
bool claude_owned_mac_window(std::uint64_t, pid_t, CGRect);
MacOwnedWindowState claude_mac_window_state(std::uint64_t, pid_t, CGRect);
namespace {
using Clock = std::chrono::steady_clock;
struct Request {
    std::string mode, prompt, marker, sentinel;
    std::uint64_t window = 0;
    unsigned pid = 0, millis = 0, owner = 0;
    std::uint64_t cutoff = 0;
    CGRect bounds = CGRectZero;
    Clock::time_point deadline;
    mutable const char* deadline_phase = "deadline-window";
    mutable const char* paste_failure = "input-paste-unsettled";
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
    return mode == "input" || mode == "input-replace-owned" || mode == "input-failure-owned";
}
static bool request(Request& value) {
    std::string line, prompt, marker, sentinel, trailing;
    if (!std::getline(std::cin, line) || line.size() > 8192 || std::cin.peek() != EOF) return false;
    std::istringstream input(line);
    double x, y, width, height;
    if (!(input >> value.mode >> value.window >> value.pid >> x >> y >> width >> height >> value.millis >> value.cutoff >> value.owner >> prompt >> marker >> sentinel) || input >> trailing) return false;
    if (!value.cutoff || value.owner < 2 || !value.window || value.pid < 2 || !value.millis || value.millis > 15000 || !std::isfinite(x) || !std::isfinite(y)
        || !std::isfinite(width) || !std::isfinite(height) || width < 300 || height < 200) return false;
    if (!input_mode(value.mode) && value.mode != "copy" && value.mode != "retry-ready" && value.mode != "retry" && value.mode != "failure-details" && value.mode != "failure-details-ready" && value.mode != "failure-details-temporal" && value.mode != "failure-details-ready-temporal" && value.mode != "retry-ready-temporal" && value.mode != "retry-temporal") return false;
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
    request.deadline_phase = "deadline-press";
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
enum class ComposerFocus { Focused, PendingIdentity, Rejected };
struct FocusIdentity {
    AXUIElementRef element = nullptr;
    pid_t pid = 0;
    std::string role;
    CGRect bounds = CGRectZero;
};
static ComposerFocus composer_focus_identity(const FocusIdentity& focused, const Node& control,
                                             const Request& request, bool target_unchanged,
                                             bool query_failed) {
    const auto bounds = focused.bounds;
    if (query_failed || !target_unchanged || !focused.element || focused.pid != static_cast<pid_t>(request.pid)
        || focused.role.empty() || !std::isfinite(bounds.origin.x) || !std::isfinite(bounds.origin.y)
        || !std::isfinite(bounds.size.width) || !std::isfinite(bounds.size.height)
        || bounds.size.width <= 0 || bounds.size.height <= 0 || !CGRectContainsRect(request.bounds, bounds))
        return ComposerFocus::Rejected;
    if (CFEqual(focused.element, control.element))
        return focused_identity(focused.element, control, focused.pid, request.pid, focused.role, bounds)
            ? ComposerFocus::Focused : ComposerFocus::Rejected;
    return ComposerFocus::PendingIdentity;
}
static ComposerFocus focused_composer(const Request& request, const Node& control) {
    if (!within(request) || ax_query_failed) return ComposerFocus::Rejected;
    const auto before_window = claude_mac_window_state(request.window, request.pid, request.bounds);
    if (!within(request) || before_window == MacOwnedWindowState::Rejected) return ComposerFocus::Rejected;
    AXUIElementRef app = AXUIElementCreateApplication(request.pid);
    if (!app) return ComposerFocus::Rejected;
    if (AXUIElementSetMessagingTimeout(app, .1F) != kAXErrorSuccess) {
        CFRelease(app);
        return ComposerFocus::Rejected;
    }
    CFTypeRef value = nullptr;
    const auto error = AXUIElementCopyAttributeValue(app, kAXFocusedUIElementAttribute, &value);
    CFRelease(app);
    ComposerFocus state = ComposerFocus::Rejected;
    if (error == kAXErrorSuccess && value && CFGetTypeID(value) == AXUIElementGetTypeID()) {
        FocusIdentity focused;
        focused.element = static_cast<AXUIElementRef>(value);
        CGRect target_bounds;
        pid_t target_pid = 0;
        const bool target_unchanged = AXUIElementGetPid(control.element, &target_pid) == kAXErrorSuccess
            && target_pid == static_cast<pid_t>(request.pid) && rectangle(control.element, target_bounds)
            && CGRectEqualToRect(target_bounds, control.bounds)
            && string_attribute(control.element, kAXRoleAttribute) == control.role;
        const bool queried = AXUIElementGetPid(focused.element, &focused.pid) == kAXErrorSuccess
            && rectangle(focused.element, focused.bounds);
        if (queried) focused.role = string_attribute(focused.element, kAXRoleAttribute);
        state = composer_focus_identity(focused, control, request, target_unchanged, !queried || ax_query_failed);
    }
    if (value) CFRelease(value);
    if (!within(request)) return ComposerFocus::Rejected;
    const auto after_window = claude_mac_window_state(request.window, request.pid, request.bounds);
    if (!within(request) || after_window == MacOwnedWindowState::Rejected || state == ComposerFocus::Rejected)
        return ComposerFocus::Rejected;
    // Exact composer identity remains required for readiness. Incomplete window
    // proof only allows another passive query under this same absolute cutoff.
    return before_window == MacOwnedWindowState::PendingFocus || after_window == MacOwnedWindowState::PendingFocus
        ? ComposerFocus::PendingIdentity : state;
}
// Focus is requested once by input(). Only a valid earlier focus identity may
// settle passively; uncertainty is terminal and no key is issued while pending.
template<class Query, class Within, class Pause>
static bool settle_composer_focus(Query query, Within within_deadline, Pause pause) {
    while (within_deadline()) {
        const auto state = query();
        if (!within_deadline() || state == ComposerFocus::Rejected) return false;
        if (state == ComposerFocus::Focused) return true;
        pause();
    }
    return false;
}
static bool wait_focused_composer(const Request& request, const Node& control) {
    const char* prior_phase = request.deadline_phase;
    request.deadline_phase = "deadline-focus";
    const bool focused = settle_composer_focus([&] { return focused_composer(request, control); },
        [&] { return within(request); },
        [&] { if (within(request)) std::this_thread::sleep_for(std::min(
            std::chrono::duration_cast<std::chrono::milliseconds>(request.deadline - Clock::now()),
            std::chrono::milliseconds(20))); });
    if (focused) request.deadline_phase = prior_phase;
    return focused;
}
enum class PastedValue { Ready, Pending, Rejected };
struct PrivateInputValue {
    std::string value;
    ~PrivateInputValue() { std::fill(value.begin(),value.end(),'\0'); }
};
static bool private_input_value(CFTypeRef value, std::string& result) {
    char buffer[4097]{};
    const bool valid=value && CFGetTypeID(value)==CFStringGetTypeID()
        && CFStringGetLength(static_cast<CFStringRef>(value))<=1024
        && CFStringGetCString(static_cast<CFStringRef>(value),buffer,sizeof(buffer),kCFStringEncodingUTF8);
    if (valid) result=buffer;
    std::fill(std::begin(buffer),std::end(buffer),'\0');
    return valid && result.size()<=1024;
}
static PastedValue pasted_value_state(const std::string& value, const std::string& prompt,
                                     bool valid_value, bool exact_focus, const std::string& initial = "") {
    if (!valid_value || !exact_focus || value.size()>1024) return PastedValue::Rejected;
    if (value==prompt) return PastedValue::Ready;
    // An asynchronous paste may still expose the exact pre-paste value. This
    // observation only permits waiting; submission still requires exact prompt.
    if (value==initial) return PastedValue::Pending;
    return prompt.compare(0,value.size(),value)==0 ? PastedValue::Pending : PastedValue::Rejected;
}
static PastedValue pasted_composer_value(const Request& request, const Node& control, const std::string& initial) {
    const auto before=focused_composer(request,control);
    if (before==ComposerFocus::Rejected) {
        request.paste_failure="input-paste-focus-before";
        return PastedValue::Rejected;
    }
    if (before==ComposerFocus::PendingIdentity) return PastedValue::Pending;
    auto value=attribute(control.element,kAXValueAttribute);
    PrivateInputValue observed;
    const bool valid=private_input_value(value,observed.value);
    if (value) CFRelease(value);
    const auto after=focused_composer(request,control);
    if (!valid || ax_query_failed || after==ComposerFocus::Rejected) {
        request.paste_failure = !valid ? "input-paste-value-unavailable"
            : ax_query_failed ? "input-paste-query-failed" : "input-paste-focus-after";
        return PastedValue::Rejected;
    }
    if (after==ComposerFocus::PendingIdentity) return PastedValue::Pending;
    const auto state=pasted_value_state(observed.value,request.prompt,true,true,initial);
    if (state==PastedValue::Rejected) request.paste_failure="input-paste-unexpected-value";
    return state;
}
template<class Query, class Within, class Pause>
static bool settle_pasted_value(Query query, Within within_deadline, Pause pause) {
    while (within_deadline()) {
        const auto state=query();
        if (!within_deadline() || state==PastedValue::Rejected) return false;
        if (state==PastedValue::Ready) return true;
        pause();
    }
    return false;
}
static bool wait_pasted_value(const Request& request, const Node& control, const std::string& initial) {
    request.deadline_phase="deadline-input-paste";
    return settle_pasted_value([&] { return pasted_composer_value(request,control,initial); },
        [&] { return within(request); },
        [] { std::this_thread::sleep_for(std::chrono::milliseconds(20)); });
}
static bool clean_failure_input(const Tree& tree, const Request& request, int draft_editor = -1) {
    if (request.prompt.empty()) return false;
    for (std::size_t i = 0; i < tree.nodes.size(); ++i) {
        const auto& node = tree.nodes[i];
        const bool draft = draft_editor >= 0 && descendant(tree, static_cast<int>(i), draft_editor);
        if ((!draft && node.label.find(request.prompt) != std::string::npos)
            || (node.role == "AXStaticText" && node.label == "Server error")
            || (node.role == "AXButton" && node.label == "Try again")) return false;
    }
    return true;
}
static const char* input(const Request& request, const Tree& tree) {
    if (!chat(tree)) return "mode";
    if (request.mode == "input-failure-owned" && !clean_failure_input(tree, request)) return "scope";
    int editor = unique(tree, "AXTextArea", "Write your prompt to Claude");
    int send = unique(tree, "AXButton", "Start task");
    if (send < 0) send = unique(tree, "AXButton", "Send message");
    if (editor < 0 || send < 0 || request.prompt.empty()) return "composer";
    const auto& control = tree.nodes[editor];
    if (!target(control, request) || !contained_control(tree.nodes[send], request)) return "control";
    auto initial_value = attribute(control.element, kAXValueAttribute);
    const char* initial_failure = initial_input_failure(initial_value, ax_query_failed, (request.mode == "input-replace-owned" || request.mode == "input-failure-owned"));
    PrivateInputValue initial;
    if (!initial_failure && !private_input_value(initial_value,initial.value)) initial_failure="input-initial-unavailable";
    if (initial_value) CFRelease(initial_value);
    if (initial_failure) return initial_failure;
    if (!owned(request)) return "input-focus-guard";
    if (AXUIElementSetAttributeValue(control.element, kAXFocusedAttribute, kCFBooleanTrue) != kAXErrorSuccess)
        return "input-focus-setting";
    if (!wait_focused_composer(request, control)) return "input-focused-identity";
    if ((request.mode == "input-replace-owned" || request.mode == "input-failure-owned")) {
        if (!key(0, true)) return "input-replace-select-key";
    }
    if (!wait_focused_composer(request, control)) return "input-prompt-before-guard";
    if (!clipboard_write(request.prompt)) return "input-prompt-clipboard";
    if (!owned(request)) return "input-prompt-after-guard";
    if (!wait_focused_composer(request, control)) return "input-focused-identity";
    if (!key(9, true)) return "input-paste-key";
    if (!wait_pasted_value(request,control,initial.value)) return request.paste_failure;
    request.deadline_phase="deadline-input-readback";
    if (!owned(request)) return "input-readback-before-guard";
    if (!clipboard_write(request.sentinel)) return "input-sentinel-clipboard";
    if (!owned(request)) return "input-sentinel-after-guard";
    if (!wait_focused_composer(request, control)) return "input-focused-identity";
    if (!key(0, true)) return "input-readback-select-key";
    if (!owned(request)) return "input-readback-select-guard";
    if (!wait_focused_composer(request, control)) return "input-focused-identity";
    if (!key(8, true)) return "input-readback-copy-key";
    while (within(request) && !clipboard_matches(request.prompt)) std::this_thread::sleep_for(std::chrono::milliseconds(20));
    bool copied = clipboard_matches(request.prompt);
    const char* readback_failure = input_readback_failure(copied,
        copied && string_attribute(control.element, kAXValueAttribute) == request.prompt);
    if (readback_failure) return readback_failure;
    if (!owned(request)) return "input-collapse-guard";
    if (!wait_focused_composer(request, control)) return "input-focused-identity";
    if (!key(124, false)) return "input-collapse-key";
    Tree fresh;
    if (!fresh.collect(request) || !chat(fresh) || !retained(fresh, control)) return "control";
    if (request.mode == "input-failure-owned") {
        const int draft_editor = unique(fresh, "AXTextArea", "Write your prompt to Claude");
        if (draft_editor < 0 || !CFEqual(fresh.nodes[draft_editor].element, control.element)) return "control";
        // Only the retained composer may contain the freshly verified unsent draft.
        // Existing transcript nonce and error controls remain globally forbidden.
        if (!clean_failure_input(fresh, request, draft_editor)) return "scope";
    }
    int enabled_send = enabled_submission(fresh, tree.nodes[send], request);
    if (enabled_send < 0) return "control";
    return press(request, fresh.nodes[enabled_send], "sent");
}
// Modern $q recovery calls onRetryLastTurn through Bw without a label override.
// The pinned action builder defaults that ordinary button to "Try again".
constexpr const char* MODERN_RETRY_LABEL = "Try again";
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
            if (descendant(tree, i, ancestor) && tree.nodes[i].role == "AXButton" && tree.nodes[i].label == (retry ? MODERN_RETRY_LABEL : "Copy")) ++controls;
        if (controls > 1) ambiguous_control = true;
        int control = unique(tree, "AXButton", retry ? MODERN_RETRY_LABEL : "Copy", ancestor);
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
// Source row labels are generated by the pinned feed keyboard-navigation path.
// Positions stay private; the observation is never used by an action selector.
static unsigned source_row_position(const std::string& label) {
    const std::string prefix = "Message ";
    if (label.rfind(prefix, 0) != 0 || label.size() <= prefix.size() || label.size() > prefix.size() + 4
        || label[prefix.size()] == '0') return 0;
    unsigned position = 0;
    for (std::size_t i = prefix.size(); i < label.size(); ++i) {
        if (label[i] < '0' || label[i] > '9') return 0;
        position = position * 10 + unsigned(label[i] - '0');
    }
    return position <= 4096 ? position : 0;
}
using RowShape = std::array<unsigned, 13>;
struct SourceRow {
    int node;
    unsigned position, prompt = 0, user = 0, headings = 0, assistant = 0, server = 0, retry = 0, details = 0;
};
template<class InTime>
static bool observe_row_shape(const Tree& tree, const Request& request, RowShape& counts, InTime in_time) {
    counts.fill(0);
    if (tree.nodes.size() > 1024 || request.prompt.empty()) return false;
    std::vector<SourceRow> rows;
    for (std::size_t i = 0; i < tree.nodes.size(); ++i) {
        if (!in_time()) return false;
        const auto& node = tree.nodes[i];
        unsigned position = source_row_position(node.label);
        bool streaming = node.label == "Currently streaming message";
        if (node.role == "AXGroup" && (position || streaming)) {
            rows.push_back({static_cast<int>(i), position});
            ++counts[0]; counts[1] += streaming;
        }
        counts[2] += node.role == "AXHeading" && node.label == "You said: " + request.prompt;
        counts[3] += node.label == request.prompt;
        counts[4] += node.role == "AXStaticText" && node.label == "Server error";
        counts[5] += node.role == "AXButton" && node.label == MODERN_RETRY_LABEL;
        counts[6] += node.role == "AXButton" && node.label == "View details";
    }
    for (std::size_t i = 0; i < tree.nodes.size(); ++i) {
        if (!in_time()) return false;
        int parent = static_cast<int>(i), row = -1;
        for (unsigned depth = 0; parent >= 0 && depth <= 32; ++depth, parent = tree.nodes[parent].parent) {
            for (std::size_t r = 0; r < rows.size(); ++r) if (rows[r].node == parent) { row = static_cast<int>(r); break; }
            if (row >= 0) break;
        }
        if (row < 0) continue;
        auto& owner = rows[row]; const auto& node = tree.nodes[i];
        owner.prompt += node.label == request.prompt;
        owner.user += node.role == "AXHeading" && node.label == "You said: " + request.prompt;
        owner.headings += node.role == "AXHeading";
        owner.assistant += node.role == "AXHeading" && node.label.rfind("Claude responded:", 0) == 0;
        owner.server += node.role == "AXStaticText" && node.label == "Server error";
        owner.retry += node.role == "AXButton" && node.label == MODERN_RETRY_LABEL;
        owner.details += node.role == "AXButton" && node.label == "View details";
    }
    std::vector<int> users, errors;
    for (std::size_t i = 0; i < rows.size(); ++i) {
        const auto& row = rows[i];
        if (row.user == 1 && row.prompt == 1 && row.headings == 1) users.push_back(i);
        if (row.server == 1 && row.retry == 1 && row.details == 1 && row.user == 0
            && row.assistant <= 1 && row.headings == row.assistant) {
            errors.push_back(i); counts[11] += row.assistant;
        }
        if (row.position && std::any_of(rows.begin(), rows.begin() + i,
            [&](const auto& previous) { return previous.position == row.position; })) ++counts[12];
    }
    counts[7] = users.size(); counts[8] = errors.size();
    for (int u : users) for (int e : errors) {
        if (!in_time()) return false;
        const auto& user = rows[u]; const auto& error = rows[e];
        int parent = tree.nodes[user.node].parent;
        if (parent < 0 || parent != tree.nodes[error.node].parent) continue;
        if (++counts[9] > 1024) return false;
        bool between = std::any_of(rows.begin(), rows.end(), [&](const auto& row) {
            return row.node > user.node && row.node < error.node && tree.nodes[row.node].parent == parent;
        });
        if (user.position && error.position == user.position + 1 && error.node > user.node && !between) ++counts[10];
    }
    return in_time();
}
struct ScopeShape {
    const char* parent_kind = "none";
    const char* walk_end = "root";
    std::array<unsigned, 7> counts{};
};
static const char* parent_kind(const std::string& role) {
    if (role == "AXGroup") return "group";
    if (role == "AXWebArea") return "web-area";
    if (role == "AXScrollArea") return "scroll-area";
    if (role == "AXWindow") return "window";
    return "other";
}
template<class InTime>
static bool observe_scope_shape(const Tree& tree, ScopeShape& shape, InTime in_time) {
    if (tree.nodes.size() > 1024) return false;
    shape = ScopeShape{};
    unsigned anchors = 0; int anchor = -1;
    for (std::size_t i = 0; i < tree.nodes.size(); ++i) {
        if (!in_time()) return false;
        const auto& node = tree.nodes[i];
        if (node.role == "AXStaticText" && node.label == "Server error" && descendant(tree, i, 0)) {
            ++anchors; anchor = i;
        }
        shape.counts[1] += source_row_position(node.label) != 0;
        shape.counts[2] += node.label == "Currently streaming message";
        shape.counts[3] += node.label == MODERN_RETRY_LABEL;
        shape.counts[4] += node.label == MODERN_RETRY_LABEL && node.role == "AXButton";
        shape.counts[5] += node.label == "View details";
        shape.counts[6] += node.label == "View details" && node.role == "AXButton";
    }
    if (anchors != 1) return false;
    int ancestor = tree.nodes[anchor].parent;
    if (ancestor >= 0) shape.parent_kind = parent_kind(tree.nodes[ancestor].role);
    unsigned depth = 0;
    for (; ancestor > 0 && depth < 6; ++depth, ancestor = tree.nodes[ancestor].parent) {
        if (!in_time()) return false;
        const auto& role = tree.nodes[ancestor].role;
        if (role == "AXWebArea") { shape.walk_end = "boundary-web-area"; return true; }
        if (role == "AXScrollArea") { shape.walk_end = "boundary-scroll-area"; return true; }
        if (role == "AXWindow") { shape.walk_end = "boundary-window"; return true; }
        shape.counts[0] += role == "AXGroup";
    }
    if (ancestor > 0 && depth == 6) shape.walk_end = "depth-limit";
    return in_time();
}
// The source server-error card keeps raw details collapsed. This selector only
// authorizes its disclosure inside one failed-user turn; Retry stays marker-bound.
static int failure_details_control(const Tree& tree, const Request& request, const char** failure = nullptr) {
    const auto reject = [&](const char* stage) { if (failure) *failure = stage; return -1; };
    unsigned anchors = 0;
    for (std::size_t i = 0; i < tree.nodes.size(); ++i) if (descendant(tree, i, 0)
        && tree.nodes[i].role == "AXStaticText" && tree.nodes[i].label == "Server error") ++anchors;
    if (anchors != 1) return reject(anchors ? "scope-anchor-ambiguous" : "scope-anchor-absent");
    if (request.prompt.empty()) return reject("scope-prompt-mismatch");
    int anchor = unique(tree, "AXStaticText", "Server error");
    bool heading_ambiguous = false, prompt_mismatch = false, control_ambiguous = false;
    int ancestor = tree.nodes[anchor].parent;
    for (unsigned depth = 0; ancestor > 0 && depth < 6; ++depth, ancestor = tree.nodes[ancestor].parent) {
        const auto& scope = tree.nodes[ancestor];
        if (scope.role == "AXWindow" || scope.role == "AXWebArea" || scope.role == "AXScrollArea") break;
        if (scope.role != "AXGroup") continue;
        unsigned prompts = 0, headings = 0, user_headings = 0, retries = 0, details_count = 0;
        for (std::size_t i = 0; i < tree.nodes.size(); ++i) if (descendant(tree, i, ancestor)) {
            prompts += tree.nodes[i].label == request.prompt;
            headings += tree.nodes[i].role == "AXHeading";
            user_headings += tree.nodes[i].role == "AXHeading"
                && tree.nodes[i].label == "You said: " + request.prompt;
            retries += tree.nodes[i].role == "AXButton" && tree.nodes[i].label == MODERN_RETRY_LABEL;
            details_count += tree.nodes[i].role == "AXButton" && tree.nodes[i].label == "View details";
        }
        int retry = unique(tree, "AXButton", MODERN_RETRY_LABEL, ancestor);
        int details = unique(tree, "AXButton", "View details", ancestor);
        if (prompts == 1 && headings == 1 && user_headings == 1 && retry >= 0 && details >= 0
            && target(tree.nodes[retry], request) && target(tree.nodes[details], request)) return details;
        heading_ambiguous |= headings > 1;
        prompt_mismatch |= prompts != 1 || user_headings != 1;
        control_ambiguous |= retries > 1 || details_count > 1;
    }
    return reject(heading_ambiguous ? "scope-heading-ambiguous" : control_ambiguous ? "scope-control-ambiguous"
        : prompt_mismatch ? "scope-prompt-mismatch" : "scope-control-absent");
}
static const char* failure_details_ready(const Request& request, const Tree& tree) {
    const char* failure = "scope";
    int index = failure_details_control(tree, request, &failure);
    if (index < 0) return failure;
    return chat(tree) && target(tree.nodes[index], request) && owned(request)
        ? "failure-details-ready" : "control";
}
static const char* open_failure_details(const Request& request, const Tree& tree) {
    const char* failure = "scope";
    int index = failure_details_control(tree, request, &failure);
    if (index < 0) return failure;
    if (!chat(tree) || !owned(request)) return "scope";
    Tree fresh;
    if (!fresh.collect(request)) return fresh.failure ? fresh.failure : "tree-query";
    int current = failure_details_control(fresh, request, &failure);
    if (current < 0) return failure;
    if (!chat(fresh) || !retained(fresh, tree.nodes[index])
        || !CFEqual(fresh.nodes[current].element, tree.nodes[index].element) || !owned(request)) return "control";
    request.deadline_phase = "deadline-press";
    if (AXUIElementPerformAction(fresh.nodes[current].element, kAXPressAction) != kAXErrorSuccess) return "action-uncertain";
    return owned(request) ? "failure-details-opened" : "action-uncertain";
}
// This mode additionally requires a Rust-held clean/Sent/request-specific failure
// capability. Native cross-call identity is the nonce user Heading, not a CF handle.
static int temporal_control(const Tree& tree, const Request& request, bool details, const char** failure, int* error_group = nullptr) {
    auto reject = [&](const char* stage) { *failure = stage; return -1; };
    if (request.prompt.empty()) return reject("scope-prompt-mismatch");
    unsigned headings=0, prompts=0, errors=0, retries=0;
    int anchor=-1;
    for (std::size_t i=0;i<tree.nodes.size();++i) {
        const auto& node=tree.nodes[i];
        headings += node.role=="AXHeading" && node.label=="You said: "+request.prompt;
        prompts += node.label==request.prompt;
        retries += node.role=="AXButton" && node.label==MODERN_RETRY_LABEL;
        if (node.role=="AXStaticText" && node.label=="Server error") { ++errors; anchor=static_cast<int>(i); }
    }
    if (headings!=1 || prompts!=1) return reject("scope-prompt-mismatch");
    if (errors!=1) return reject(errors?"scope-anchor-ambiguous":"scope-anchor-absent");
    if (retries!=1) return reject(retries?"scope-control-ambiguous":"scope-control-absent");
    int parent=tree.nodes[anchor].parent;
    for (unsigned depth=0;parent>0 && depth<6;++depth,parent=tree.nodes[parent].parent) {
        const auto& scope=tree.nodes[parent];
        if (scope.role=="AXWindow" || scope.role=="AXWebArea" || scope.role=="AXScrollArea") break;
        if (scope.role!="AXGroup") continue;
        int retry=unique(tree,"AXButton",MODERN_RETRY_LABEL,parent);
        if (retry<0) continue;
        unsigned scope_headings=0;
        for (std::size_t i=0;i<tree.nodes.size();++i) if (descendant(tree,i,parent)) {
            scope_headings += tree.nodes[i].role=="AXHeading";
        }
        // The smallest shared error/control Group may never become the transcript.
        if (scope_headings) return reject("scope-heading-ambiguous");
        int index=details?unique(tree,"AXButton","View details",parent):retry;
        if (index<0) return reject("scope-control-absent");
        if (error_group) *error_group=parent;
        return target(tree.nodes[retry],request) && target(tree.nodes[index],request)?index:-1;
    }
    return reject("scope-control-absent");
}
static const char* temporal_action(const Request& request, const Tree& tree) {
    bool details=request.mode.rfind("failure-details",0)==0;
    bool ready=request.mode.find("ready")!=std::string::npos;
    const char* failure="scope";
    int group=-1;
    int index=temporal_control(tree,request,details,&failure,&group);
    if (index<0) return failure;
    if (!chat(tree) || !owned(request)) return "control";
    Tree fresh;
    if (!fresh.collect(request)) return fresh.failure?fresh.failure:"tree-query";
    int fresh_group=-1;
    int current=temporal_control(fresh,request,details,&failure,&fresh_group);
    if (current<0) return failure;
    for (const auto& node : tree.nodes) if ((node.role=="AXHeading" && node.label=="You said: "+request.prompt)
        || node.label==request.prompt || (node.role=="AXStaticText" && node.label=="Server error")
        || (node.role=="AXTextArea" && node.label=="Write your prompt to Claude")
        || (node.role=="AXGroup" && node.label=="Mode")
        || (node.role=="AXButton" && node.label=="Chat")) {
        if (!retained(fresh,node)) return "control";
    }
    if (group<0 || fresh_group<0 || !retained(fresh,tree.nodes[group])
        || !CFEqual(tree.nodes[group].element,fresh.nodes[fresh_group].element)) return "control";
    if (!chat(fresh) || !retained(fresh,tree.nodes[index])
        || !CFEqual(fresh.nodes[current].element,tree.nodes[index].element) || !owned(request)) return "control";
    if (ready) return details?"failure-details-ready":"retry-ready";
    request.deadline_phase="deadline-press";
    if (AXUIElementPerformAction(fresh.nodes[current].element,kAXPressAction)!=kAXErrorSuccess) return "action-uncertain";
    return owned(request)?(details?"failure-details-opened":"retried"):"action-uncertain";
}
static const char* action(const Request& request, const Tree& tree) {
    if (request.mode.find("-temporal") != std::string::npos) return temporal_action(request, tree);
    if (request.mode == "failure-details-ready") return failure_details_ready(request, tree);
    if (request.mode == "failure-details") return open_failure_details(request, tree);
    bool retry = request.mode != "copy";
    const char* failure = "scope";
    int index = scoped_control(tree, request, retry, &failure);
    if (index < 0) return failure;
    if (request.mode == "retry-ready") return target(tree.nodes[index], request) && owned(request) ? "retry-ready" : "control";
    if (!retry && !clipboard_write(request.sentinel)) return "response-mismatch";
    const char* result = press(request, tree.nodes[index], retry ? "retried" : "copied");
    if (std::string(result) != "copied") return result;
    request.deadline_phase = "deadline-copy";
    while (within(request) && !clipboard_matches(request.marker)) std::this_thread::sleep_for(std::chrono::milliseconds(20));
    return clipboard_matches(request.marker) && owned(request) ? "copied" : "response-mismatch";
}
static const char* deadline_result(const Request& request, const char* stage, bool expired) {
    return expired && std::string(stage) != "action-uncertain" ? request.deadline_phase : stage;
}
} // namespace
int claude_chat_turn() {
    @autoreleasepool {
        Request value;
        const char* stage = "request";
        RowShape row_shape{}; bool row_shape_observed = false;
        ScopeShape scope_shape{}; bool scope_shape_observed = false;
        if (request(value)) {
            Tree tree;
            if (!owned(value)) stage = "window";
            else {
                value.deadline_phase = "deadline-tree";
                if (!tree.collect(value)) stage = tree.failure ? tree.failure : "tree-query";
                else {
                    value.deadline_phase = input_mode(value.mode) ? "deadline-input"
                        : value.mode == "copy" ? "deadline-copy"
                        : (value.mode == "retry-ready" || value.mode == "retry-ready-temporal") ? "deadline-retry-ready" : "deadline-retry";
                    if (value.mode == "failure-details" || value.mode == "failure-details-ready" || value.mode == "failure-details-temporal" || value.mode == "failure-details-ready-temporal") {
                        row_shape_observed = observe_row_shape(tree, value, row_shape, [&] { return within(value); });
                        scope_shape_observed = row_shape_observed && observe_scope_shape(tree, scope_shape, [&] { return within(value); });
                    }
                    stage = input_mode(value.mode) ? input(value, tree) : action(value, tree);
                }
            }
            stage = deadline_result(value, stage, !within(value));
        }
        std::cout << "turn " << stage << '\n';
        if (row_shape_observed && within(value)) {
            std::cout << "rows";
            for (unsigned count : row_shape) std::cout << ' ' << count;
            std::cout << '\n';
            if (scope_shape_observed && within(value)) {
                std::cout << "scope " << scope_shape.parent_kind << ' ' << scope_shape.walk_end;
                for (unsigned count : scope_shape.counts) std::cout << ' ' << count;
                std::cout << '\n';
            }
        }
        return std::cout ? 0 : 5;
    }
}
#else
int claude_chat_turn() { return 5; }
#endif
