// Window ownership and stacking metadata only; never reads another window's text or pixels.
#include <cstdint>
#include <cmath>
#include <array>
#include <algorithm>
#include <cctype>
#include <charconv>
#include <iomanip>
#include <iostream>
#include <limits>
#include <string>
#include <sstream>
#include <chrono>
#include <thread>

#if !defined(_WIN32)
int process_presence(bool) { return 5; }
int process_correlation(bool) { return 5; }
int owned_cleanup_holder() { return 5; }
int windows_claude_storage() { return 5; }
#if !defined(__APPLE__)
int fit_window(const std::string&) { return 5; }
#endif
int window_state(const std::string&) { return 5; }
#endif

#if !defined(__APPLE__)
int claude_chat_press() { return 5; }
int activate_window(const std::string&) { return 5; }
int observe_claude() { return 5; }
int claude_known_folders() { return 5; }
int windows_focus(const std::string&) { return 5; }
#endif

static std::string encode_name(const std::string& name) {
    if (name.empty()) return "-";
    const char* hex = "0123456789abcdef";
    std::string output;
    for (unsigned char byte : name) {
        output += hex[byte >> 4];
        output += hex[byte & 15];
    }
    return output;
}

static void window_record(std::uint64_t id, std::uint32_t pid, double x, double y,
                          double width, double height, const std::string& name) {
    std::cout << "WIN " << id << ' ' << pid << ' ' << std::fixed << std::setprecision(3)
              << x << ' ' << y << ' ' << width << ' ' << height << ' ' << encode_name(name) << '\n';
}

#if defined(__APPLE__)
#import <AppKit/AppKit.h>
#include <CoreGraphics/CoreGraphics.h>
#include <ApplicationServices/ApplicationServices.h>
#include <libproc.h>
#include "inventory.hpp"

// Query Foundation directly, without initializing AppKit or inspecting applications.
int claude_known_folders() {
    @autoreleasepool {
        NSString* expected = NSProcessInfo.processInfo.environment[@"HOME"];
        if (!expected || !expected.isAbsolutePath) return 5;
        NSFileManager* manager = NSFileManager.defaultManager;
        NSURL* support = [manager URLsForDirectory:NSApplicationSupportDirectory
                                        inDomains:NSUserDomainMask].firstObject;
        if (!support) return 5;
        bool aligned = [manager.homeDirectoryForCurrentUser.path isEqualToString:expected]
            && [support.path isEqualToString:[expected stringByAppendingPathComponent:@"Library/Application Support"]];
        std::cout << (aligned ? "true\n" : "false\n");
        return 0;
    }
}

static bool parse_identity_token(const std::string& text, std::uint64_t& value) {
    if (text.empty() || text.find_first_not_of("0123456789") != std::string::npos) return false;
    auto parsed = std::from_chars(text.data(), text.data() + text.size(), value);
    return parsed.ec == std::errc{} && parsed.ptr == text.data() + text.size();
}

static std::int64_t number(CFDictionaryRef dictionary, CFStringRef key) {
    std::int64_t value = 0;
    auto entry = static_cast<CFNumberRef>(CFDictionaryGetValue(dictionary, key));
    if (entry) CFNumberGetValue(entry, kCFNumberSInt64Type, &value);
    return value;
}

// macOS variant that also reports the CoreGraphics window level. The level is
// a closed numeric property used only by the transient wave10 occlusion
// diagnostic; no window title, text, or pixel data is ever emitted.
static void window_record_with_layer(std::uint64_t id, std::uint32_t pid, double x, double y,
                                     double width, double height, const std::string& name,
                                     std::int64_t layer) {
    std::cout << "WIN " << id << ' ' << pid << ' ' << std::fixed << std::setprecision(3)
              << x << ' ' << y << ' ' << width << ' ' << height << ' ' << encode_name(name)
              << ' ' << layer << '\n';
}

const char* classify_ax_error(AXError error) {
    switch (error) {
        case kAXErrorFailure: return "failure";
        case kAXErrorIllegalArgument: return "illegal-argument";
        case kAXErrorInvalidUIElement: return "invalid-element";
        case kAXErrorCannotComplete: return "cannot-complete";
        case kAXErrorAttributeUnsupported: return "attribute-unsupported";
        case kAXErrorNotImplemented: return "not-implemented";
        case kAXErrorAPIDisabled: return "api-disabled";
        case kAXErrorNoValue: return "no-value";
        default: return "other";
    }
}

// Public accessibility focus reads only. No actions, attributes or activation are changed.
struct AxFocus {
    AXUIElementRef focused = nullptr, main = nullptr, input_window = nullptr;
    CGRect bounds = CGRectZero;
    const char* status = "query-error";
    const char* query_stage = nullptr;
    const char* query_error = nullptr;
    void failure(const char* stage, const char* error) { query_stage = stage; query_error = error; }
    ~AxFocus() {
        if (focused) CFRelease(focused);
        if (main) CFRelease(main);
        if (input_window) CFRelease(input_window);
    }
};
static bool ax_attribute(AXUIElementRef element, CFStringRef name, CFTypeRef& value,
                         AxFocus& result, const char* stage) {
    AXError error = AXUIElementCopyAttributeValue(element, name, &value);
    if (error != kAXErrorSuccess) { result.failure(stage, classify_ax_error(error)); return false; }
    if (!value) { result.failure(stage, "empty-value"); return false; }
    return true;
}
static bool ax_timeout(AXUIElementRef element, AxFocus& result, const char* stage) {
    AXError error = AXUIElementSetMessagingTimeout(element, 0.1f);
    if (error != kAXErrorSuccess) { result.failure(stage, classify_ax_error(error)); return false; }
    return true;
}
static void read_ax_focus(pid_t pid, AxFocus& result, bool window_only = false) {
    if (!AXIsProcessTrusted()) { result.status = "untrusted"; return; }
    AXUIElementRef app = AXUIElementCreateApplication(pid);
    if (!app) { result.failure("app-create", "empty-value"); return; }
    if (!ax_timeout(app, result, "app-timeout")) { CFRelease(app); return; }
    CFTypeRef focused = nullptr, main = nullptr, input = nullptr, input_window = nullptr;
    bool read = ax_attribute(app, kAXFocusedWindowAttribute, focused, result, "focused-window")
        && ax_attribute(app, kAXMainWindowAttribute, main, result, "main-window")
        && (window_only || ax_attribute(app, kAXFocusedUIElementAttribute, input, result, "focused-element"));
    CFRelease(app);
    if (window_only && read) {
        // Reuse the ownership/geometry checks without asserting any focused input.
        // This separate diagnostic never substitutes for the full focus proof.
        input_window = focused;
        if (input_window) CFRetain(input_window);
    } else if (read && CFGetTypeID(input) == AXUIElementGetTypeID()) {
        read = ax_timeout(static_cast<AXUIElementRef>(input), result, "input-timeout");
        read = read && ax_attribute(static_cast<AXUIElementRef>(input), kAXWindowAttribute, input_window, result, "input-window");
    } else { if (read) result.failure("element-type", "type-mismatch"); read = false; }
    if (input) CFRelease(input);
    if (!read || !focused || !main || !input_window
        || CFGetTypeID(focused) != AXUIElementGetTypeID()
        || CFGetTypeID(main) != AXUIElementGetTypeID()
        || CFGetTypeID(input_window) != AXUIElementGetTypeID()) {
        if (read) result.failure("element-type", "type-mismatch");
        if (focused) CFRelease(focused);
        if (main) CFRelease(main);
        if (input_window) CFRelease(input_window);
        return;
    }
    result.focused = static_cast<AXUIElementRef>(focused);
    result.main = static_cast<AXUIElementRef>(main);
    result.input_window = static_cast<AXUIElementRef>(input_window);
    if (!CFEqual(focused, main) || !CFEqual(focused, input_window)) {
        result.status = "focus-mismatch"; return;
    }
    for (auto element : {result.focused, result.main, result.input_window}) {
        pid_t owner = 0;
        AXError error = AXUIElementGetPid(element, &owner);
        if (error != kAXErrorSuccess) { result.failure("pid", classify_ax_error(error)); return; }
        if (owner != pid) { result.failure("pid", "owner-mismatch"); return; }
    }
    if (!ax_timeout(result.focused, result, "window-timeout")) return;
    CFTypeRef role = nullptr, subrole = nullptr, position = nullptr, size = nullptr;
    read = ax_attribute(result.focused, kAXRoleAttribute, role, result, "role")
        && ax_attribute(result.focused, kAXSubroleAttribute, subrole, result, "subrole")
        && ax_attribute(result.focused, kAXPositionAttribute, position, result, "position")
        && ax_attribute(result.focused, kAXSizeAttribute, size, result, "size");
    if (read && (!CFEqual(role, kAXWindowRole) || !CFEqual(subrole, kAXStandardWindowSubrole))) {
        result.status = "not-standard"; read = false;
    }
    CGPoint origin;
    CGSize dimensions;
    if (read && CFGetTypeID(position) == AXValueGetTypeID() && CFGetTypeID(size) == AXValueGetTypeID()
        && AXValueGetValue(static_cast<AXValueRef>(position), kAXValueTypeCGPoint, &origin)
        && AXValueGetValue(static_cast<AXValueRef>(size), kAXValueTypeCGSize, &dimensions)
        && std::isfinite(origin.x) && std::isfinite(origin.y)
        && std::isfinite(dimensions.width) && std::isfinite(dimensions.height)
        && dimensions.width > 0 && dimensions.height > 0) {
        result.bounds = CGRectMake(origin.x, origin.y, dimensions.width, dimensions.height);
        result.status = "ready";
    } else if (read) result.failure("geometry", "geometry-invalid");
    if (role) CFRelease(role);
    if (subrole) CFRelease(subrole);
    if (position) CFRelease(position);
    if (size) CFRelease(size);
}
// Used by the read-only proof and synthetic fixtures; never queries the OS.
std::uint64_t match_focus_window(CFArrayRef windows, pid_t owner, CGRect expected, unsigned& matches) {
    matches = 0;
    std::uint64_t id = 0;
    for (CFIndex index = 0; index < CFArrayGetCount(windows); ++index) {
        auto window = static_cast<CFDictionaryRef>(CFArrayGetValueAtIndex(windows, index));
        CGRect bounds;
        auto value = static_cast<CFDictionaryRef>(CFDictionaryGetValue(window, kCGWindowBounds));
        if (number(window, kCGWindowOwnerPID) == owner && number(window, kCGWindowLayer) == 0
            && number(window, kCGWindowNumber) > 0 && value
            && CGRectMakeWithDictionaryRepresentation(value, &bounds) && CGRectEqualToRect(bounds, expected)) {
            ++matches;
            id = number(window, kCGWindowNumber);
        }
    }
    return matches == 1 ? id : 0;
}
const char* classify_focus_agreement(bool stable_identity, unsigned matches) {
    if (!stable_identity) return "identity-changed";
    return matches == 1 ? "proved" : matches == 0 ? "no-match" : "ambiguous";
}

static void print_ax_focus(pid_t foreground, CFArrayRef windows, const AxFocus& before, bool window_only = false) {
    AxFocus after;
    if (std::string(before.status) == "ready") read_ax_focus(foreground, after, window_only);
    const char* status = before.status;
    std::uint64_t id = 0;
    if (std::string(status) == "ready") {
        bool stable = [[[NSWorkspace sharedWorkspace] frontmostApplication] processIdentifier] == foreground
            && std::string(after.status) == "ready" && CFEqual(before.focused, after.focused)
            && CFEqual(before.main, after.main) && CFEqual(before.input_window, after.input_window)
            && CGRectEqualToRect(before.bounds, after.bounds);
        unsigned matches = 0;
        if (stable) id = match_focus_window(windows, foreground, before.bounds, matches);
        status = classify_focus_agreement(stable, matches);
    }
    std::cout << (window_only ? "FOCUS_WINDOW " : "FOCUS ") << status << ' ' << id << '\n';
    const AxFocus& failed = before.query_stage ? before : after;
    if (failed.query_stage) {
        std::cout << (window_only ? "FOCUS_WINDOW_QUERY " : "FOCUS_QUERY ") << (before.query_stage ? "before" : "after") << ' '
                  << failed.query_stage << ' ' << failed.query_error << '\n';
    }
}

static int list_mac_windows(bool include_foreground, bool focus_proof, pid_t expected_pid) {
    @autoreleasepool {
        auto foreground = include_foreground
            ? [[[NSWorkspace sharedWorkspace] frontmostApplication] processIdentifier] : 0;
        AxFocus focus, window_focus;
        if (focus_proof && foreground == expected_pid) read_ax_focus(foreground, window_focus, true);
        else if (focus_proof) window_focus.status = "focus-mismatch";
        if (focus_proof && foreground == expected_pid) read_ax_focus(foreground, focus);
        else if (focus_proof) focus.status = "focus-mismatch";
        std::cout << "FG " << foreground << " 0\n";
        CGDirectDisplayID displays[32];
        std::uint32_t count = 0;
        if (CGGetActiveDisplayList(32, displays, &count) != kCGErrorSuccess || count == 32) return 5;
        for (std::uint32_t index = 0; index < count; ++index) {
            auto rect = CGDisplayBounds(displays[index]);
            std::cout << "DISPLAY " << rect.origin.x << ' ' << rect.origin.y << ' '
                      << rect.size.width << ' ' << rect.size.height << '\n';
        }
        auto windows = CGWindowListCopyWindowInfo(
            kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements, kCGNullWindowID);
        if (!windows) return 5;
        auto length = CFArrayGetCount(windows);
        if (length > 1024) { CFRelease(windows); return 5; }
        for (CFIndex index = 0; index < length; ++index) {
            auto window = static_cast<CFDictionaryRef>(CFArrayGetValueAtIndex(windows, index));
            // CoreGraphics enumerates the hardware cursor as a WindowServer window.
            // xa11y's screen capture excludes it; it is not an occluding application.
            if (number(window, kCGWindowLayer) == CGWindowLevelForKey(kCGCursorWindowLevelKey)) continue;
            CGRect bounds;
            auto rect = static_cast<CFDictionaryRef>(CFDictionaryGetValue(window, kCGWindowBounds));
            if (!rect || !CGRectMakeWithDictionaryRepresentation(rect, &bounds)) {
                CFRelease(windows); return 5;
            }
            double alpha = 1;
            auto alpha_value = static_cast<CFNumberRef>(CFDictionaryGetValue(window, kCGWindowAlpha));
            if (alpha_value) CFNumberGetValue(alpha_value, kCFNumberDoubleType, &alpha);
            if (alpha <= 0 || bounds.size.width <= 0 || bounds.size.height <= 0) continue;
            auto pid = static_cast<std::uint32_t>(number(window, kCGWindowOwnerPID));
            char name[256] = {};
            proc_name(pid, name, sizeof(name));
            window_record_with_layer(number(window, kCGWindowNumber), pid, bounds.origin.x,
                                     bounds.origin.y, bounds.size.width, bounds.size.height,
                                     name, number(window, kCGWindowLayer));
        }
        if (focus_proof) {
            print_ax_focus(foreground, windows, focus);
            print_ax_focus(foreground, windows, window_focus, true);
        }
        CFRelease(windows);
        return std::cout ? 0 : 5;
    }
}
int list_windows(bool include_foreground) { return list_mac_windows(include_foreground, false, 0); }
int windows_focus(const std::string& request) {
    std::uint64_t pid = 0;
    if (!parse_identity_token(request, pid) || pid == 0 || pid > std::numeric_limits<pid_t>::max()) return 5;
    return list_mac_windows(true, true, static_cast<pid_t>(pid));
}


static void observation(const char* state) {
    std::cout << "OBS " << state << '\n';
}

static void inventory_observation(const char* state) {
    std::cout << "INV " << state << '\n';
}

// Classify only the already-filtered PID; no title, path, geometry, or other
// window metadata is read. The caller owns and releases inventory.
const char* classify_window_inventory(CFArrayRef inventory, pid_t expected_pid) {
    if (!inventory || CFArrayGetCount(inventory) > 1024) return "query-unavailable";
    bool saw_on_screen = false;
    bool saw_offscreen = false;
    auto length = CFArrayGetCount(inventory);
    for (CFIndex index = 0; index < length; ++index) {
        auto value = CFArrayGetValueAtIndex(inventory, index);
        if (!value || CFGetTypeID(value) != CFDictionaryGetTypeID()) return "query-unavailable";
        auto window = static_cast<CFDictionaryRef>(value);
        auto owner = CFDictionaryGetValue(window, kCGWindowOwnerPID);
        if (!owner || CFGetTypeID(owner) != CFNumberGetTypeID()) return "query-unavailable";
        std::int64_t owner_pid = 0;
        if (!CFNumberGetValue(static_cast<CFNumberRef>(owner), kCFNumberSInt64Type, &owner_pid))
            return "query-unavailable";
        if (owner_pid != static_cast<std::int64_t>(expected_pid)) continue;
        auto on_screen = CFDictionaryGetValue(window, kCGWindowIsOnscreen);
        if (!on_screen || CFGetTypeID(on_screen) != CFBooleanGetTypeID())
            return "query-unavailable";
        if (CFBooleanGetValue(static_cast<CFBooleanRef>(on_screen))) saw_on_screen = true;
        else saw_offscreen = true;
    }
    if (saw_on_screen && saw_offscreen) return "query-unavailable";
    if (saw_on_screen) return "present-onscreen";
    if (saw_offscreen) return "present-offscreen";
    return "absent";
}

static bool canonical_same(NSURL* left, NSURL* right) {
    if (!left || !right) return false;
    auto canonical_left = [[left URLByResolvingSymlinksInPath] URLByStandardizingPath];
    auto canonical_right = [[right URLByResolvingSymlinksInPath] URLByStandardizingPath];
    return canonical_left.path && [canonical_left.path isEqualToString:canonical_right.path];
}

static bool claude_window_name(const char* name) {
    if (!name || !*name) return false;
    std::string value(name);
    std::transform(value.begin(), value.end(), value.begin(), [](unsigned char byte) {
        return static_cast<char>(std::tolower(byte));
    });
    return value == "claude" || value == "claude-desktop";
}

int observe_claude() {
    @autoreleasepool {
        std::array<char, 4097> input{};
        std::cin.getline(input.data(), input.size());
        auto length = std::cin.gcount();
        if (length <= 1 || length > 4096 || !std::cin
            || std::cin.peek() != std::char_traits<char>::eof()) {
            observation("query-unavailable");
            return std::cout ? 0 : 5;
        }
        std::string bundle_path(input.data(), static_cast<std::size_t>(length) - 1);
        if (bundle_path.empty() || bundle_path.find('\0') != std::string::npos
            || bundle_path.find('\r') != std::string::npos
            || bundle_path.find('\n') != std::string::npos) {
            observation("query-unavailable");
            return std::cout ? 0 : 5;
        }
        auto path = [NSString stringWithUTF8String:bundle_path.c_str()];
        auto bundle_url = path ? [NSURL fileURLWithPath:path isDirectory:YES] : nil;
        auto bundle = bundle_url ? [NSBundle bundleWithURL:bundle_url] : nil;
        auto expected_identifier = bundle.bundleIdentifier;
        auto expected_executable = bundle.executableURL;
        if (!bundle_url || !bundle || !expected_identifier
            || ![expected_identifier isEqualToString:@"com.anthropic.claudefordesktop"]
            || !expected_executable) {
            observation("query-unavailable");
            return std::cout ? 0 : 5;
        }

        auto applications = [[NSWorkspace sharedWorkspace] runningApplications];
        if (!applications) {
            observation("query-unavailable");
            return std::cout ? 0 : 5;
        }
        if (applications.count > 1024) {
            observation("overflow");
            return std::cout ? 0 : 5;
        }
        std::size_t matching_count = 0;
        pid_t expected_pid = 0;
        NSRunningApplication* matched_application = nil;
        for (NSRunningApplication* application in applications) {
            if (![application.bundleIdentifier
                    isEqualToString:@"com.anthropic.claudefordesktop"])
                continue;
            if (!application.bundleURL || !application.executableURL) {
                observation("query-unavailable");
                return std::cout ? 0 : 5;
            }
            auto bundle_matches = canonical_same(application.bundleURL, bundle_url);
            auto executable_matches = canonical_same(application.executableURL, expected_executable);
            if (bundle_matches != executable_matches) {
                observation("ambiguous-identity");
                return std::cout ? 0 : 5;
            }
            if (bundle_matches) {
                ++matching_count;
                expected_pid = application.processIdentifier;
                matched_application = application;
            }
        }
        if (matching_count == 0) {
            observation("no-matching-bundle-process");
            return std::cout ? 0 : 5;
        }
        if (matching_count > 1) {
            observation("ambiguous-identity");
            return std::cout ? 0 : 5;
        }
        if (!matched_application) {
            observation("query-unavailable");
            return std::cout ? 0 : 5;
        }
        std::cout << "READY finished-launching="
                  << ([matched_application isFinishedLaunching] ? "1" : "0")
                  << " hidden=" << ([matched_application isHidden] ? "1" : "0")
                  << " active=" << ([matched_application isActive] ? "1" : "0") << '\n';
        auto windows = CGWindowListCopyWindowInfo(
            kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
            kCGNullWindowID);
        if (!windows) {
            observation("query-unavailable");
            return std::cout ? 0 : 5;
        }
        auto window_count = CFArrayGetCount(windows);
        if (window_count > 1024) {
            CFRelease(windows);
            observation("overflow");
            return std::cout ? 0 : 5;
        }
        std::size_t visible = 0;
        std::size_t named = 0;
        std::size_t eligible = 0;
        for (CFIndex index = 0; index < window_count; ++index) {
            auto window = static_cast<CFDictionaryRef>(CFArrayGetValueAtIndex(windows, index));
            if (number(window, kCGWindowLayer) == CGWindowLevelForKey(kCGCursorWindowLevelKey))
                continue;
            auto pid = static_cast<pid_t>(number(window, kCGWindowOwnerPID));
            if (pid != expected_pid) continue;
            auto rect = static_cast<CFDictionaryRef>(CFDictionaryGetValue(window, kCGWindowBounds));
            CGRect bounds;
            if (!rect || !CGRectMakeWithDictionaryRepresentation(rect, &bounds)) {
                CFRelease(windows);
                observation("query-unavailable");
                return std::cout ? 0 : 5;
            }
            double alpha = 1;
            auto alpha_value = static_cast<CFNumberRef>(CFDictionaryGetValue(window, kCGWindowAlpha));
            if (alpha_value) CFNumberGetValue(alpha_value, kCFNumberDoubleType, &alpha);
            if (alpha <= 0 || bounds.size.width <= 0 || bounds.size.height <= 0) continue;
            ++visible;
            char name[256] = {};
            if (proc_name(static_cast<std::uint32_t>(pid), name, sizeof(name)) <= 0) {
                CFRelease(windows);
                observation("query-unavailable");
                return std::cout ? 0 : 5;
            }
            if (!claude_window_name(name)) continue;
            ++named;
            if (bounds.size.width >= 300 && bounds.size.height >= 200) ++eligible;
        }
        CFRelease(windows);
        if (visible == 0) {
            // The on-screen query cannot distinguish an absent window from a
            // hidden or minimized one.  Only after a successful identity match
            // do one bounded all-window read, filtering by PID before reading
            // any other property.  This is diagnostic evidence only; it never
            // changes candidate eligibility or activates a window.
            if ([matched_application isTerminated]) {
                observation("matching-process-no-visible-window");
                inventory_observation("query-unavailable");
                return std::cout ? 0 : 5;
            }
            auto inventory = CGWindowListCopyWindowInfo(
                kCGWindowListOptionAll | kCGWindowListExcludeDesktopElements, kCGNullWindowID);
            auto inventory_state = classify_window_inventory(inventory, expected_pid);
            if (inventory) CFRelease(inventory);
            // Both visibility states can naturally coexist; the classifier
            // conservatively reports unknown rather than collapsing them.
            // "present-onscreen" means present in this later inventory
            // snapshot, not that it was excluded by the earlier query.
            observation("matching-process-no-visible-window");
            if ([matched_application isTerminated])
                inventory_observation("query-unavailable");
            else
                inventory_observation(inventory_state);
        } else if (named == 0) observation("window-name-mismatch");
        else if (eligible == 0) observation("window-not-eligible");
        else observation("window-eligible");
        return std::cout ? 0 : 5;
    }
}

// Pure coordinate conversion shared with synthetic fixtures.
bool mac_fit_rectangle(CGRect before, CGRect visible, CGFloat primary_top, CGRect& target) {
    target = CGRectMake(visible.origin.x, primary_top - CGRectGetMaxY(visible),
        std::min(before.size.width, visible.size.width), std::min(before.size.height, visible.size.height));
    return std::isfinite(target.origin.x) && std::isfinite(target.origin.y)
        && std::isfinite(target.size.width) && std::isfinite(target.size.height)
        && target.size.width >= 300 && target.size.height >= 200;
}
// Fit only the independently focused original standard window. Never activates it.
static bool fit_mac_proof(std::uint64_t id, pid_t pid, AxFocus& focus, bool require_off_display) {
    if ([[[NSWorkspace sharedWorkspace] frontmostApplication] processIdentifier] != pid) return false;
    auto application = [NSRunningApplication runningApplicationWithProcessIdentifier:pid];
    if (!application || !application.active || application.hidden || !application.finishedLaunching) return false;
    read_ax_focus(pid, focus);
    if (std::string(focus.status) != "ready") return false;
    auto windows = CGWindowListCopyWindowInfo(kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements, kCGNullWindowID);
    if (!windows) return false;
    unsigned matches = 0;
    bool safe = CFArrayGetCount(windows) <= 1024 && match_focus_window(windows, pid, focus.bounds, matches) == id;
    bool found = false;
    for (CFIndex index = 0; safe && index < CFArrayGetCount(windows); ++index) {
        auto window = static_cast<CFDictionaryRef>(CFArrayGetValueAtIndex(windows, index));
        if (number(window, kCGWindowNumber) == id && number(window, kCGWindowOwnerPID) == pid) { found = true; break; }
        if (number(window, kCGWindowLayer) == CGWindowLevelForKey(kCGCursorWindowLevelKey)) continue;
        CGRect bounds;
        auto value = static_cast<CFDictionaryRef>(CFDictionaryGetValue(window, kCGWindowBounds));
        if (!value || !CGRectMakeWithDictionaryRepresentation(value, &bounds)) { safe = false; break; }
        double alpha = 1;
        auto alpha_value = static_cast<CFNumberRef>(CFDictionaryGetValue(window, kCGWindowAlpha));
        if (alpha_value) CFNumberGetValue(alpha_value, kCFNumberDoubleType, &alpha);
        if (alpha <= 0 || bounds.size.width <= 0 || bounds.size.height <= 0) continue;
        if ((number(window, kCGWindowOwnerPID) == pid && number(window, kCGWindowLayer) == 0)
            || CGRectIntersectsRect(bounds, focus.bounds)) safe = false;
    }
    CFRelease(windows);
    bool contained = false;
    for (NSScreen* screen in NSScreen.screens) {
        auto display = static_cast<CGDirectDisplayID>([screen.deviceDescription[@"NSScreenNumber"] unsignedIntValue]);
        contained = contained || CGRectContainsRect(CGDisplayBounds(display), focus.bounds);
    }
    return safe && found && (require_off_display ? !contained : contained);
}
// Shared owned-window proof for the separately scoped native Chat controller.
bool claude_owned_mac_window(std::uint64_t id, pid_t pid, CGRect bounds) {
    AxFocus focus;
    return fit_mac_proof(id, pid, focus, false) && CGRectEqualToRect(bounds, focus.bounds);
}
static int chat_result(const char* stage) {
    std::cout << "chat " << stage << '\n';
    return std::cout ? 0 : 5;
}
using ChatDeadline = std::chrono::steady_clock::time_point;
static bool chat_timely(ChatDeadline deadline) { return std::chrono::steady_clock::now() < deadline; }
static bool chat_geometry(AXUIElementRef element, CGRect& bounds) {
    CFTypeRef position = nullptr, size = nullptr;
    AXError first = AXUIElementCopyAttributeValue(element, kAXPositionAttribute, &position);
    AXError second = AXUIElementCopyAttributeValue(element, kAXSizeAttribute, &size);
    CGPoint origin{}; CGSize dimensions{};
    bool valid = first == kAXErrorSuccess && second == kAXErrorSuccess && position && size
        && CFGetTypeID(position) == AXValueGetTypeID() && CFGetTypeID(size) == AXValueGetTypeID()
        && AXValueGetValue(static_cast<AXValueRef>(position), kAXValueTypeCGPoint, &origin)
        && AXValueGetValue(static_cast<AXValueRef>(size), kAXValueTypeCGSize, &dimensions)
        && std::isfinite(origin.x) && std::isfinite(origin.y)
        && std::isfinite(dimensions.width) && std::isfinite(dimensions.height)
        && dimensions.width > 0 && dimensions.height > 0;
    if (valid) bounds = CGRectMake(origin.x, origin.y, dimensions.width, dimensions.height);
    if (position) CFRelease(position);
    if (size) CFRelease(size);
    return valid;
}
// Synthetic-testable identity/cardinality/geometry boundary; never queries the OS.
bool chat_control_agreement(unsigned groups, unsigned buttons, bool same_control,
                            CGRect held, CGRect fresh, CGRect window) {
    return groups == 1 && buttons == 1 && same_control
        && held.size.width > 0 && held.size.height > 0
        && CGRectEqualToRect(held, fresh) && CGRectContainsRect(window, held);
}
// Chromium maps the source aria-current token to AXARIACurrent. Only the
// exact page token can prove the retained Chat pill is already active.
bool chat_current_page(CFTypeRef value) {
    return value && CFGetTypeID(value) == CFStringGetTypeID()
        && CFEqual(value, CFSTR("page"));
}
static bool chat_is_current(AXUIElementRef button) {
    CFTypeRef value = nullptr;
    bool current = AXUIElementCopyAttributeValue(button, CFSTR("AXARIACurrent"), &value)
        == kAXErrorSuccess && chat_current_page(value);
    if (value) CFRelease(value);
    return current;
}
struct ChatControls {
    AXUIElementRef button = nullptr;
    CGRect bounds{};
    unsigned groups = 0, buttons = 0, nodes = 0;
    ~ChatControls() { if (button) CFRelease(button); }
};
static bool collect_chat(AXUIElementRef element, pid_t pid, CGRect window, ChatDeadline deadline,
                         ChatControls& found, unsigned depth = 0, bool in_mode = false) {
    if (!chat_timely(deadline) || depth > 32 || ++found.nodes > 1024) return false;
    pid_t owner = 0;
    if (AXUIElementGetPid(element, &owner) != kAXErrorSuccess || owner != pid
        || AXUIElementSetMessagingTimeout(element, 0.1) != kAXErrorSuccess) return false;
    const void* keys[] = {kAXRoleAttribute, kAXTitleAttribute, kAXDescriptionAttribute,
                         kAXEnabledAttribute, kAXChildrenAttribute};
    auto attributes = CFArrayCreate(nullptr, keys, 5, &kCFTypeArrayCallBacks);
    CFArrayRef values = nullptr;
    AXError error = AXUIElementCopyMultipleAttributeValues(element, attributes, 0, &values);
    CFRelease(attributes);
    if (error != kAXErrorSuccess || !values || CFArrayGetCount(values) != 5) {
        if (values) CFRelease(values);
        return false;
    }
    auto value = [&](CFIndex index) { return CFArrayGetValueAtIndex(values, index); };
    auto equals = [&](CFIndex index, CFStringRef expected) {
        return CFGetTypeID(value(index)) == CFStringGetTypeID() && CFEqual(value(index), expected);
    };
    bool mode = equals(0, kAXGroupRole) && (equals(1, CFSTR("Mode")) || equals(2, CFSTR("Mode")));
    if (mode) ++found.groups;
    bool valid = true;
    if ((in_mode || mode) && equals(0, kAXButtonRole)
        && (equals(1, CFSTR("Chat")) || equals(2, CFSTR("Chat")))) {
        CGRect bounds;
        valid = CFEqual(value(3), kCFBooleanTrue) && chat_geometry(element, bounds)
            && CGRectContainsRect(window, bounds);
        if (valid) {
            ++found.buttons;
            if (!found.button) { found.button = element; CFRetain(element); found.bounds = bounds; }
        }
    }
    auto children = value(4);
    if (valid && CFGetTypeID(children) == CFArrayGetTypeID()) {
        auto array = static_cast<CFArrayRef>(children);
        valid = CFArrayGetCount(array) <= 1024;
        for (CFIndex index = 0; valid && index < CFArrayGetCount(array); ++index) {
            auto child = CFArrayGetValueAtIndex(array, index);
            valid = CFGetTypeID(child) == AXUIElementGetTypeID()
                && collect_chat(static_cast<AXUIElementRef>(child), pid, window, deadline, found, depth + 1, in_mode || mode);
        }
    } else if (valid) {
        AXError missing = kAXErrorFailure;
        // Apple's batch contract also represents unsupported leaf attributes by CFNull.
        valid = CFGetTypeID(children) == CFNullGetTypeID()
            || (CFGetTypeID(children) == AXValueGetTypeID()
                && AXValueGetType(static_cast<AXValueRef>(children)) == kAXValueAXErrorType
                && AXValueGetValue(static_cast<AXValueRef>(children), static_cast<AXValueType>(kAXValueAXErrorType), &missing)
                && (missing == kAXErrorAttributeUnsupported || missing == kAXErrorNoValue));
    }
    CFRelease(values);
    return valid && chat_timely(deadline);
}
static bool chat_hit_target(AXUIElementRef button, pid_t pid, CGRect bounds, ChatDeadline deadline) {
    auto application = AXUIElementCreateApplication(pid);
    AXUIElementRef hit = nullptr;
    bool valid = application && AXUIElementSetMessagingTimeout(application, 0.1) == kAXErrorSuccess
        && AXUIElementCopyElementAtPosition(application, CGRectGetMidX(bounds), CGRectGetMidY(bounds), &hit) == kAXErrorSuccess;
    if (application) CFRelease(application);
    bool matched = false;
    for (unsigned depth = 0; valid && hit && depth <= 32 && chat_timely(deadline); ++depth) {
        pid_t owner = 0;
        if (AXUIElementGetPid(hit, &owner) != kAXErrorSuccess || owner != pid) break;
        if (CFEqual(hit, button)) { matched = true; break; }
        CFTypeRef parent = nullptr;
        if (AXUIElementSetMessagingTimeout(hit, 0.1) != kAXErrorSuccess
            || AXUIElementCopyAttributeValue(hit, kAXParentAttribute, &parent) != kAXErrorSuccess
            || !parent || CFGetTypeID(parent) != AXUIElementGetTypeID()) {
            if (parent) CFRelease(parent);
            break;
        }
        CFRelease(hit); hit = static_cast<AXUIElementRef>(parent);
    }
    if (hit) CFRelease(hit);
    return matched && chat_timely(deadline);
}
int claude_chat_press() {
    @autoreleasepool {
        std::string request;
        if (!std::getline(std::cin, request) || request.size() > 256 || std::cin.peek() != EOF) return chat_result("request");
        std::istringstream input(request);
        std::string id_text, pid_text, extra;
        std::uint64_t id = 0, pid = 0;
        double x = 0, y = 0, width = 0, height = 0;
        unsigned budget = 0;
        if (!(input >> id_text >> pid_text >> x >> y >> width >> height >> budget) || (input >> extra)
            || !parse_identity_token(id_text, id) || !parse_identity_token(pid_text, pid)
            || id == 0 || id > UINT32_MAX || pid == 0 || pid > static_cast<std::uint64_t>(std::numeric_limits<pid_t>::max())
            || !std::isfinite(x) || !std::isfinite(y) || !std::isfinite(width) || !std::isfinite(height)
            || width <= 0 || height <= 0 || budget == 0 || budget > 5000) return chat_result("request");
        auto deadline = std::chrono::steady_clock::now() + std::chrono::milliseconds(budget);
        CGRect expected = CGRectMake(x, y, width, height);
        AxFocus before;
        if (!fit_mac_proof(id, static_cast<pid_t>(pid), before, false)) return chat_result("initial-proof");
        if (!CGRectEqualToRect(before.bounds, expected)) return chat_result("window-bounds");
        ChatControls held;
        if (!collect_chat(before.focused, pid, expected, deadline, held)) return chat_result(chat_timely(deadline) ? "tree" : "deadline");
        if (held.groups != 1) return chat_result("mode");
        if (held.buttons != 1) return chat_result("chat");
        AxFocus fresh;
        if (!chat_timely(deadline)) return chat_result("deadline");
        if (!fit_mac_proof(id, static_cast<pid_t>(pid), fresh, false)
            || !CFEqual(before.focused, fresh.focused) || !CGRectEqualToRect(fresh.bounds, expected)) return chat_result("control-recheck");
        ChatControls current;
        if (!collect_chat(fresh.focused, pid, expected, deadline, current)) return chat_result(chat_timely(deadline) ? "tree" : "deadline");
        if (!chat_control_agreement(current.groups, current.buttons, current.button && CFEqual(held.button, current.button),
                                    held.bounds, current.bounds, expected)) return chat_result("control-recheck");
        if (!chat_hit_target(held.button, pid, held.bounds, deadline)) return chat_result(chat_timely(deadline) ? "hit-test" : "deadline");
        AxFocus final_focus;
        if (!fit_mac_proof(id, static_cast<pid_t>(pid), final_focus, false)
            || !CFEqual(before.focused, final_focus.focused) || !CGRectEqualToRect(final_focus.bounds, expected)) return chat_result("control-recheck");
        CGRect final_bounds;
        if (!chat_geometry(held.button, final_bounds) || !CGRectEqualToRect(held.bounds, final_bounds)) return chat_result("control-recheck");
        CFTypeRef enabled = nullptr;
        bool still_enabled = AXUIElementCopyAttributeValue(held.button, kAXEnabledAttribute, &enabled) == kAXErrorSuccess
            && enabled && CFEqual(enabled, kCFBooleanTrue);
        if (enabled) CFRelease(enabled);
        if (!still_enabled) return chat_result("control-recheck");
        if (!chat_timely(deadline)) return chat_result("deadline");
        bool held_current = chat_is_current(held.button);
        bool fresh_current = chat_is_current(current.button);
        if (!chat_timely(deadline)) return chat_result("deadline");
        if (held_current && fresh_current) return chat_result("current-chat");
        return chat_result(AXUIElementPerformAction(held.button, kAXPressAction) == kAXErrorSuccess ? "completed" : "press-uncertain");
    }
}

// Closed stage output is rejected by the caller; only empty timely output proves fit.
static int mac_fit_rejected(const char* stage) {
    std::cout << "fit-rejected " << stage << '\n';
    return std::cout ? 5 : 4;
}
int fit_window(const std::string& request) {
    @autoreleasepool {
        std::istringstream input(request);
        std::string id_text, pid_text, extra;
        std::uint64_t id = 0, pid = 0;
        if (!(input >> id_text >> pid_text) || (input >> extra)
            || !parse_identity_token(id_text, id) || !parse_identity_token(pid_text, pid)
            || id == 0 || id > UINT32_MAX || pid == 0 || pid > static_cast<std::uint64_t>(std::numeric_limits<pid_t>::max())) return mac_fit_rejected("request");
        AxFocus before;
        if (!fit_mac_proof(id, static_cast<pid_t>(pid), before, true)) return mac_fit_rejected("initial-proof");
        NSScreen* selected = nil;
        CGFloat area = -1;
        for (NSScreen* screen in NSScreen.screens) {
            auto display = static_cast<CGDirectDisplayID>([screen.deviceDescription[@"NSScreenNumber"] unsignedIntValue]);
            auto intersection = CGRectIntersection(CGDisplayBounds(display), before.bounds);
            CGFloat current = CGRectIsNull(intersection) ? 0 : intersection.size.width * intersection.size.height;
            if (current > area) { selected = screen; area = current; }
        }
        if (!selected || NSScreen.screens.count == 0) return mac_fit_rejected("screen");
        NSRect visible = selected.visibleFrame;
        CGFloat top = NSMaxY([NSScreen.screens[0] frame]);
        CGRect target;
        if (!mac_fit_rectangle(before.bounds, NSRectToCGRect(visible), top, target)) return mac_fit_rejected("rectangle");
        Boolean position_settable = false, size_settable = false;
        if (AXUIElementIsAttributeSettable(before.focused, kAXPositionAttribute, &position_settable) != kAXErrorSuccess
            || AXUIElementIsAttributeSettable(before.focused, kAXSizeAttribute, &size_settable) != kAXErrorSuccess
            || !position_settable || !size_settable) return mac_fit_rejected("settable");
        AxFocus final_before;
        if (!fit_mac_proof(id, static_cast<pid_t>(pid), final_before, true)
            || !CFEqual(before.focused, final_before.focused) || !CGRectEqualToRect(before.bounds, final_before.bounds)) return mac_fit_rejected("identity-recheck");
        AXValueRef size = AXValueCreate(kAXValueTypeCGSize, &target.size);
        AXValueRef position = AXValueCreate(kAXValueTypeCGPoint, &target.origin);
        if (!size || !position) { if (size) CFRelease(size); if (position) CFRelease(position); return mac_fit_rejected("allocation"); }
        AXError resized = AXUIElementSetAttributeValue(before.focused, kAXSizeAttribute, size);
        AXError moved = resized == kAXErrorSuccess ? AXUIElementSetAttributeValue(before.focused, kAXPositionAttribute, position) : resized;
        CFRelease(size); CFRelease(position);
        if (resized != kAXErrorSuccess) return mac_fit_rejected("size");
        if (moved != kAXErrorSuccess) return mac_fit_rejected("position");
        const auto settle_deadline = std::chrono::steady_clock::now() + std::chrono::milliseconds(500);
        do {
            AxFocus after;
            if (fit_mac_proof(id, static_cast<pid_t>(pid), after, false)) return 0;
            std::this_thread::sleep_for(std::chrono::milliseconds(20));
        } while (std::chrono::steady_clock::now() < settle_deadline);
        return mac_fit_rejected("postcondition");
    }
}
int activate_window(const std::string& request) {
    @autoreleasepool {
        std::istringstream input(request);
        std::uint64_t expected_id = 0;
        std::uint64_t expected_pid = 0;
        std::string id_text;
        std::string pid_text;
        std::string extra;
        if (!(input >> id_text >> pid_text) || (input >> extra)
            || !parse_identity_token(id_text, expected_id)
            || !parse_identity_token(pid_text, expected_pid)
            || expected_id == 0 || expected_pid == 0
            || expected_id > std::numeric_limits<std::uint32_t>::max()
            || expected_pid > std::numeric_limits<std::uint32_t>::max()
            || expected_pid > std::numeric_limits<pid_t>::max()) return 5;

        auto windows = CGWindowListCopyWindowInfo(
            kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
            kCGNullWindowID);
        if (!windows) return 5;
        auto length = CFArrayGetCount(windows);
        if (length > 1024) {
            CFRelease(windows);
            return 5;
        }
        bool found = false;
        for (CFIndex index = 0; index < length; ++index) {
            auto window = static_cast<CFDictionaryRef>(CFArrayGetValueAtIndex(windows, index));
            auto id = static_cast<std::uint64_t>(number(window, kCGWindowNumber));
            auto pid = static_cast<std::uint64_t>(number(window, kCGWindowOwnerPID));
            if (id == expected_id && pid == expected_pid) {
                found = true;
                break;
            }
        }
        CFRelease(windows);
        if (!found) return 5;
        auto application = [NSRunningApplication
            runningApplicationWithProcessIdentifier:static_cast<pid_t>(expected_pid)];
        if (!application) return 5;
        return [application activateWithOptions:NSApplicationActivateIgnoringOtherApps
                                                   | NSApplicationActivateAllWindows] ? 0 : 5;
    }
}
#elif defined(_WIN32)
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <tlhelp32.h>
#include <shlobj.h>
#include <bcrypt.h>

// Private wire identities stay in the checker RAM, never in public diagnostics.
#include <vector>
struct CorrelationIdentity { DWORD pid; ULONGLONG created; bool descendant; };
#include "process_correlation.hpp"
static bool correlation_time(std::uint32_t pid, std::uint64_t& value) {
    HANDLE process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid);
    if (!process) return false;
    FILETIME created{}, exited{}, kernel{}, user{};
    const bool ok = GetProcessTimes(process, &created, &exited, &kernel, &user) != FALSE;
    CloseHandle(process);
    value = (static_cast<ULONGLONG>(created.dwHighDateTime) << 32) | created.dwLowDateTime;
    return ok && value != 0;
}
static bool correlation_snapshot(DWORD checker, std::vector<CorrelationEntry>& rows) {
    HANDLE snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
    if (snapshot == INVALID_HANDLE_VALUE) return false;
    struct Guard { HANDLE value; ~Guard() { CloseHandle(value); } } guard{snapshot};
    PROCESSENTRY32W row{}; row.dwSize = sizeof(row);
    if (!Process32FirstW(snapshot, &row)) return false;
    bool own = false; unsigned matches = 0;
    do {
        if (rows.size() >= 65536) return false;
        const auto end = std::find(std::begin(row.szExeFile), std::end(row.szExeFile), L'\0');
        if (end == std::end(row.szExeFile) || end == std::begin(row.szExeFile)) return false;
        const std::wstring name(std::begin(row.szExeFile), end);
        if (name.find_first_of(L"/\\") != std::wstring::npos) return false;
        const bool matching = _wcsicmp(name.c_str(), L"Claude.exe") == 0;
        if (matching && ++matches > 64) return false;
        own = own || row.th32ProcessID == checker;
        rows.push_back({row.th32ProcessID, row.th32ParentProcessID, matching});
    } while (Process32NextW(snapshot, &row));
    return GetLastError() == ERROR_NO_MORE_FILES && own;
}
int process_correlation(bool before) {
    std::string request;
    struct RequestGuard { std::string& value; ~RequestGuard() { if (!value.empty()) SecureZeroMemory(value.data(), value.size()); } } request_guard{request};
    char character;
    while (std::cin.get(character)) {
        if (request.size() >= 8192) return 2;
        request.push_back(character);
    }
    std::istringstream input(request); DWORD checker = 0, launcher = 0;
    if (!(input >> checker >> launcher) || checker == 0 || launcher == 0) return 2;
    stage = "snapshot";
    std::vector<CorrelationEntry> rows;
    if (!correlation_snapshot(checker, rows)) { std::cout << "unavailable\n"; return 0; }
    if (before) {
        std::string extra; if (input >> extra) return 2;
        std::uint64_t launcher_time = 0;
        if (!correlation_time(launcher, launcher_time)) { std::cout << "unavailable\n"; return 0; }
        std::vector<CorrelationEntry> confirm;
        if (!correlation_snapshot(checker, confirm)) { std::cout << "unavailable\n"; return 0; }
        std::vector<CorrelationIdentity> verified{{launcher, launcher_time, false}};
        stage = "ancestry";
    for (const auto& row : rows) {
            if (!row.matching || row.pid == launcher) continue;
            std::uint64_t created = 0;
            if (correlation_time(row.pid, created)
                && historical_descendant(rows, confirm, row.pid, launcher, launcher_time,
                    created, correlation_time)) verified.push_back({row.pid, created, true});
        }
        // Recheck the root identity after the snapshot and ancestor queries.
        std::uint64_t final_time = 0;
        if (!correlation_time(launcher, final_time) || final_time != launcher_time) { std::cout << "unavailable\n"; return 0; }
        std::cout << "snapshot " << verified.size() << '\n';
        for (const auto& item : verified) std::cout << item.pid << ' ' << item.created << ' ' << item.descendant << '\n';
    } else {
        unsigned count = 0; if (!(input >> count) || count == 0 || count > 65) return 2;
        std::vector<CorrelationIdentity> verified;
        for (unsigned i = 0; i < count; ++i) {
            DWORD pid = 0; ULONGLONG created = 0; unsigned descendant = 0;
            if (!(input >> pid >> created >> descendant) || pid == 0 || created == 0 || descendant > 1) return 2;
            if (std::any_of(verified.begin(), verified.end(), [pid](const auto& v) { return v.pid == pid; })) return 2;
            verified.push_back({pid, created, descendant == 1});
        }
        std::string extra; if (input >> extra || verified.front().pid != launcher || verified.front().descendant) return 2;
        std::vector<CorrelationEntry> confirm;
        if (!correlation_snapshot(checker, confirm)) { std::cout << "unavailable\n"; return 0; }
        if (std::count_if(rows.begin(), rows.end(), [](const auto& e) { return e.matching; })
            != std::count_if(confirm.begin(), confirm.end(), [](const auto& e) { return e.matching; })) {
            std::cout << "unavailable\n"; return 0;
        }
        bool launcher_alive = false; unsigned matches = 0, linked = 0, unlinked = 0;
        std::uint64_t time = 0;
        const bool launcher_present = std::any_of(rows.begin(), rows.end(), [launcher](const auto& e) { return e.pid == launcher; });
        if (launcher_present && !correlation_time(launcher, time)) { std::cout << "unavailable\n"; return 0; }
        launcher_alive = launcher_present && time == verified.front().created;
        for (const auto& row : rows) {
            if (!row.matching) continue;
            const auto stable = std::find_if(confirm.begin(), confirm.end(), [&](const auto& e) { return e.pid == row.pid && e.matching; });
            if (stable == confirm.end()) { std::cout << "unavailable\n"; return 0; }
            ++matches;
            if (!correlation_time(row.pid, time)) { std::cout << "unavailable\n"; return 0; }
            const auto found = std::find_if(verified.begin(), verified.end(), [&](const auto& v) { return v.pid == row.pid && v.created == time && v.descendant; });
            if (found == verified.end()) ++unlinked; else ++linked;
        }
        std::cout << "observed " << launcher_alive << ' ' << matches << ' ' << linked << ' ' << unlinked << '\n';
    }
    SecureZeroMemory(request.data(), request.size());
    return std::cout ? 0 : 4;
}

// Retained process handles, not reopened PIDs, are the only termination targets.
struct CleanupHandle {
    HANDLE value = INVALID_HANDLE_VALUE;
    explicit CleanupHandle(HANDLE handle) : value(handle) {}
    CleanupHandle(const CleanupHandle&) = delete;
    CleanupHandle& operator=(const CleanupHandle&) = delete;
    CleanupHandle(CleanupHandle&& other) noexcept : value(other.value) { other.value = INVALID_HANDLE_VALUE; }
    ~CleanupHandle() { if (value != INVALID_HANDLE_VALUE && value != nullptr) CloseHandle(value); }
};
static bool held_creation(HANDLE process, std::uint64_t& time) {
    FILETIME created{}, exited{}, kernel{}, user{};
    if (!GetProcessTimes(process, &created, &exited, &kernel, &user)) return false;
    time = (static_cast<std::uint64_t>(created.dwHighDateTime) << 32) | created.dwLowDateTime;
    return time != 0;
}
static bool held_image(HANDLE process, const BY_HANDLE_FILE_INFORMATION& expected,
    const std::wstring& expected_path) {
    wchar_t path[32768] = {}; DWORD size = 32768;
    if (!QueryFullProcessImageNameW(process, 0, path, &size) || size == 0 || size >= 32768) return false;
    CleanupHandle file(CreateFileW(path, FILE_READ_ATTRIBUTES, FILE_SHARE_READ,
        nullptr, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr));
    if (file.value == INVALID_HANDLE_VALUE) return false;
    BY_HANDLE_FILE_INFORMATION actual{};
    wchar_t canonical[32768] = {};
    const DWORD length = GetFinalPathNameByHandleW(file.value, canonical, 32768, FILE_NAME_NORMALIZED);
    return length > 0 && length < 32768 && _wcsicmp(canonical, expected_path.c_str()) == 0
        && GetFileInformationByHandle(file.value, &actual)
        && actual.dwVolumeSerialNumber == expected.dwVolumeSerialNumber
        && actual.nFileIndexHigh == expected.nFileIndexHigh && actual.nFileIndexLow == expected.nFileIndexLow
        && actual.nFileSizeHigh == expected.nFileSizeHigh && actual.nFileSizeLow == expected.nFileSizeLow
        && actual.ftLastWriteTime.dwHighDateTime == expected.ftLastWriteTime.dwHighDateTime
        && actual.ftLastWriteTime.dwLowDateTime == expected.ftLastWriteTime.dwLowDateTime;
}
static bool pinned_digest(HANDLE file, const std::string& expected) {
    if (expected.size() != 64 || expected.find_first_not_of("0123456789abcdef") != std::string::npos) return false;
    LARGE_INTEGER length{};
    if (!GetFileSizeEx(file, &length) || length.QuadPart < 0 || length.QuadPart > 512LL * 1024 * 1024) return false;
    BCRYPT_ALG_HANDLE algorithm = nullptr; BCRYPT_HASH_HANDLE hash = nullptr;
    if (BCryptOpenAlgorithmProvider(&algorithm, BCRYPT_SHA256_ALGORITHM, nullptr, 0) < 0) return false;
    struct CryptoGuard { BCRYPT_ALG_HANDLE algorithm; BCRYPT_HASH_HANDLE& hash;
        ~CryptoGuard() { if (hash) BCryptDestroyHash(hash); BCryptCloseAlgorithmProvider(algorithm, 0); } } guard{algorithm, hash};
    if (BCryptCreateHash(algorithm, &hash, nullptr, 0, nullptr, 0, 0) < 0) return false;
    std::array<unsigned char,65536> buffer{}; std::uint64_t total = 0;
    while (true) {
        DWORD read = 0;
        if (!ReadFile(file, buffer.data(), static_cast<DWORD>(buffer.size()), &read, nullptr)) return false;
        if (read == 0) break;
        total += read;
        if (total > static_cast<std::uint64_t>(length.QuadPart) || BCryptHashData(hash, buffer.data(), read, 0) < 0) return false;
    }
    std::array<unsigned char,32> digest{};
    if (total != static_cast<std::uint64_t>(length.QuadPart) || BCryptFinishHash(hash, digest.data(), static_cast<ULONG>(digest.size()), 0) < 0) return false;
    const char* hex = "0123456789abcdef"; std::string actual;
    for (const auto byte : digest) { actual.push_back(hex[byte >> 4]); actual.push_back(hex[byte & 15]); }
    SecureZeroMemory(buffer.data(), buffer.size());
    return actual == expected;
}

static bool cleanup_line(std::string& line) {
    line.clear(); char byte;
    while (std::cin.get(byte)) {
        if (byte == '\n') return true;
        if (line.size() >= 8192 || byte == '\0' || byte == '\r') return false;
        line.push_back(byte);
    }
    return false;
}
int owned_cleanup_holder() {
    const char* stage = "request";
    auto unavailable = [&] { std::cout << "unavailable " << stage << '\n' << std::flush; return 0; };
    std::string request;
    if (!cleanup_line(request)) return unavailable();
    std::istringstream input(request); DWORD checker = 0, launcher = 0;
    std::string encoded, digest, extra;
    if (!(input >> checker >> launcher >> encoded >> digest) || input >> extra || checker == 0 || launcher == 0
        || encoded.empty() || encoded.size() % 2 != 0 || encoded.size() > 4096) return unavailable();
    std::string path;
    for (std::size_t i = 0; i < encoded.size(); i += 2) {
        unsigned byte = 0;
        const auto parsed = std::from_chars(encoded.data() + i, encoded.data() + i + 2, byte, 16);
        if (parsed.ec != std::errc() || parsed.ptr != encoded.data() + i + 2 || byte == 0) return unavailable();
        path.push_back(static_cast<char>(byte));
    }
    stage = "path";
    const int wide_size = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, path.data(), static_cast<int>(path.size()), nullptr, 0);
    if (wide_size <= 0) return unavailable();
    std::wstring expected_path(static_cast<std::size_t>(wide_size), L'\0');
    if (MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, path.data(), static_cast<int>(path.size()), expected_path.data(), wide_size) != wide_size)
        return unavailable();
    // The checker sends the canonical installed executable, never a CLI wrapper.
    if (expected_path.rfind(L"\\\\?\\", 0) != 0 || expected_path.size() < 15
        || _wcsicmp(expected_path.substr(expected_path.find_last_of(L"\\") + 1).c_str(), L"Claude.exe") != 0) return unavailable();
    stage = "file-open";
    CleanupHandle expected_file(CreateFileW(expected_path.c_str(), GENERIC_READ, FILE_SHARE_READ,
        nullptr, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr));
    if (expected_file.value == INVALID_HANDLE_VALUE) return unavailable();
    stage = "file-hash";
    if (!pinned_digest(expected_file.value, digest)) return unavailable();
    stage = "file-identity";
    BY_HANDLE_FILE_INFORMATION expected{}; wchar_t canonical[32768] = {};
    const DWORD canonical_size = GetFinalPathNameByHandleW(expected_file.value, canonical, 32768, FILE_NAME_NORMALIZED);
    if (!GetFileInformationByHandle(expected_file.value, &expected) || canonical_size == 0 || canonical_size >= 32768
        || _wcsicmp(canonical, expected_path.c_str()) != 0 || expected.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) return unavailable();
    stage = "process-open";
    CleanupHandle owner(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE, FALSE, checker));
    CleanupHandle root(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, launcher));
    std::uint64_t root_time = 0;
    if (!owner.value || !root.value || !held_creation(root.value, root_time)) return unavailable();
    stage = "snapshot";
    std::vector<CorrelationEntry> rows, confirm;
    if (!correlation_snapshot(checker, rows) || !correlation_snapshot(checker, confirm)) return unavailable();
    stage = "inspector-parent";
    const auto self = std::find_if(rows.begin(), rows.end(), [](const auto& row) { return row.pid == GetCurrentProcessId(); });
    if (self == rows.end() || self->parent != checker) return unavailable();
    struct Target {
        CleanupHandle handle; std::uint64_t created; bool targeted = false;
        Target(HANDLE value, std::uint64_t time) : handle(value), created(time) {}
    };
    std::vector<Target> targets;
    for (const auto& row : rows) {
        if (!row.matching) continue;
        stage = "ancestry";
        std::uint64_t created = 0;
        if (!correlation_time(row.pid, created) || !historical_descendant(rows, confirm, row.pid,
            launcher, root_time, created, correlation_time)) return unavailable();
        stage = "target-open";
        HANDLE handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | SYNCHRONIZE, FALSE, row.pid);
        if (!handle) return unavailable();
        targets.emplace_back(handle, created);
        stage = "target-identity";
        std::uint64_t held_time = 0;
        if (!held_creation(handle, held_time) || held_time != created || !held_image(handle, expected, expected_path)) return unavailable();
    }
    stage = "owner-recheck";
    std::uint64_t final_root_time = 0;
    if (!held_creation(root.value, final_root_time) || final_root_time != root_time
        || WaitForSingleObject(owner.value, 0) != WAIT_TIMEOUT) return unavailable();
    std::cout << "ready " << targets.size() << ' ' << GetTickCount64() << '\n' << std::flush;
    if (!cleanup_line(request)) return 0; // EOF drops handles without terminating any target.
    std::istringstream trigger(request); std::string action; std::uint64_t cutoff = 0;
    const auto now = GetTickCount64();
    if (!(trigger >> action >> cutoff) || trigger >> extra || action != "cleanup"
        || cutoff <= now || cutoff - now > 5000 || WaitForSingleObject(owner.value, 0) != WAIT_TIMEOUT) return unavailable();
    unsigned already = 0, targeted = 0, rejected = 0;
    // Unknown/new matching processes never become targets. If the complete
    // post-stop inventory cannot correlate, none of the retained handles is used.
    std::vector<CorrelationEntry> post;
    if (!correlation_snapshot(checker, post)) {
        return unavailable();
    }
    for (const auto& row : post) {
        if (!row.matching) continue;
        std::uint64_t created = 0;
        const auto retained = std::find_if(targets.begin(), targets.end(), [&](const auto& target) {
            return GetProcessId(target.handle.value) == row.pid;
        });
        if (retained == targets.end() || !correlation_time(row.pid, created) || created != retained->created) {
            return unavailable();
        }
    }
    for (auto& target : targets) {
        if (GetTickCount64() >= cutoff || WaitForSingleObject(owner.value, 0) != WAIT_TIMEOUT) { ++rejected; continue; }
        const DWORD state = WaitForSingleObject(target.handle.value, 0);
        if (state == WAIT_OBJECT_0) { ++already; continue; }
        std::uint64_t created = 0;
        if (state != WAIT_TIMEOUT || !held_creation(target.handle.value, created) || created != target.created
            || !held_image(target.handle.value, expected, expected_path)) { ++rejected; continue; }
        if (!terminate_verified_handle({GetTickCount64() < cutoff,
                WaitForSingleObject(owner.value, 0) == WAIT_TIMEOUT, true, true},
            [&] { return TerminateProcess(target.handle.value, 1) != FALSE; })) { ++rejected; continue; }
        target.targeted = true; ++targeted;
    }
    unsigned exited = 0;
    do {
        exited = 0;
        for (const auto& target : targets) if (target.targeted && WaitForSingleObject(target.handle.value, 0) == WAIT_OBJECT_0) ++exited;
        if (exited == targeted || GetTickCount64() >= cutoff) break;
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    } while (true);
    std::cout << "result " << targets.size() << ' ' << already << ' ' << targeted << ' ' << exited << ' ' << rejected << '\n' << std::flush;
    SecureZeroMemory(request.data(), request.size()); SecureZeroMemory(path.data(), path.size());
    return std::cout ? 0 : 4;
}

// Fixed known-folder metadata only: never enumerates or opens profile files.
static bool storage_metadata(const std::wstring& path, bool directory, bool& present) {
    DWORD attributes = GetFileAttributesW(path.c_str());
    if (attributes == INVALID_FILE_ATTRIBUTES) {
        DWORD error = GetLastError();
        present = false;
        return error == ERROR_FILE_NOT_FOUND || error == ERROR_PATH_NOT_FOUND;
    }
    if (attributes & FILE_ATTRIBUTE_REPARSE_POINT) return false;
    present = true;
    return bool(attributes & FILE_ATTRIBUTE_DIRECTORY) == directory;
}

static bool storage_root(REFKNOWNFOLDERID folder, const wchar_t* name, bool& state, bool& preferences) {
    PWSTR value = nullptr;
    HRESULT result = SHGetKnownFolderPath(folder, KF_FLAG_DONT_VERIFY, nullptr, &value);
    if (FAILED(result) || !value) {
        if (value) CoTaskMemFree(value);
        return false;
    }
    std::wstring root(value);
    CoTaskMemFree(value);
    bool exists = false;
    if (!storage_metadata(root, true, exists) || !exists) return false;
    root += L"\\";
    root += name;
    if (!storage_metadata(root, true, exists)) return false;
    if (!exists) { state = preferences = false; return true; }
    if (!storage_metadata(root + L"\\Local State", false, state)) return false;
    bool default_exists = false;
    if (!storage_metadata(root + L"\\Default", true, default_exists)) return false;
    if (!default_exists) { preferences = false; return true; }
    return storage_metadata(root + L"\\Default\\Preferences", false, preferences);
}

int windows_claude_storage() {
    bool normal_state = false, normal_preferences = false;
    bool third_state = false, third_preferences = false;
    if (!storage_root(FOLDERID_RoamingAppData, L"Claude", normal_state, normal_preferences)
        || !storage_root(FOLDERID_LocalAppData, L"Claude-3p", third_state, third_preferences)) return 5;
    std::cout << "storage " << normal_state << ' ' << normal_preferences << ' '
        << third_state << ' ' << third_preferences << '\n';
    return std::cout ? 0 : 4;
}

// Only the complete read-only snapshot may establish absence. The caller's
// checker PID (not this helper's PID) must occur in the snapshot.
int process_presence(bool claude) {
    auto fail = [](const char* stage) {
        std::cout << "error " << stage << '\n';
        return std::cout ? 0 : 4;
    };
    char request[32] = {};
    if (!std::cin.getline(request, sizeof(request))
        || std::cin.peek() != std::char_traits<char>::eof()) return fail("schema");
    std::uint32_t checker = 0;
    const std::string text(request);
    const auto parsed = std::from_chars(text.data(), text.data() + text.size(), checker);
    if (parsed.ec != std::errc() || parsed.ptr != text.data() + text.size() || checker == 0)
        return fail("schema");
    HANDLE snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
    if (snapshot == INVALID_HANDLE_VALUE) return fail("snapshot");
    struct HandleGuard {
        HANDLE value;
        ~HandleGuard() { CloseHandle(value); }
    } guard{snapshot};
    PROCESSENTRY32W entry{};
    entry.dwSize = sizeof(entry);
    if (!Process32FirstW(snapshot, &entry)) return fail("first");
    bool present = false, own_seen = false;
    std::size_t count = 0;
    const wchar_t* expected = claude ? L"Claude.exe" : L"ChatGPT.exe";
    do {
        if (++count > 65536) return fail("oversize");
        const auto end = std::find(std::begin(entry.szExeFile), std::end(entry.szExeFile), L'\0');
        if (end == std::end(entry.szExeFile) || end == std::begin(entry.szExeFile)) return fail("schema");
        const std::wstring name(std::begin(entry.szExeFile), end);
        if (name.find_first_of(L"/\\") != std::wstring::npos) return fail("schema");
        for (std::size_t i = 0; i < name.size(); ++i) {
            const auto unit = static_cast<unsigned>(name[i]);
            if (unit >= 0xD800 && unit <= 0xDBFF) {
                if (++i >= name.size() || static_cast<unsigned>(name[i]) < 0xDC00
                    || static_cast<unsigned>(name[i]) > 0xDFFF) return fail("schema");
            } else if (unit >= 0xDC00 && unit <= 0xDFFF) return fail("schema");
        }
        own_seen = own_seen || entry.th32ProcessID == checker;
        present = present || _wcsicmp(name.c_str(), expected) == 0;
    } while (Process32NextW(snapshot, &entry));
    if (GetLastError() != ERROR_NO_MORE_FILES) return fail("next");
    if (!own_seen) return fail("schema");
    std::cout << (present ? "present\n" : "absent\n");
    return std::cout ? 0 : 4;
}


#include <dwmapi.h>

static int fit_failure(const char* stage, const char* detail = nullptr) {
    std::cout << "FIT_FAILURE " << stage;
    if (detail) std::cout << ' ' << detail;
    std::cout << '\n';
    return 0;
}

static bool contains_rect(const RECT& outer, const RECT& inner) {
    return inner.right > inner.left && inner.bottom > inner.top
        && outer.right > outer.left && outer.bottom > outer.top
        && inner.left >= outer.left && inner.top >= outer.top
        && inner.right <= outer.right && inner.bottom <= outer.bottom;
}

static int fit_foreground_failure(const char* stage, HWND foreground, DWORD expected_pid) {
    DWORD foreground_pid = 0;
    if (!GetWindowThreadProcessId(foreground, &foreground_pid) || foreground_pid == 0)
        return fit_failure(stage, "identity-unavailable");
    return fit_failure(
        stage,
        foreground_pid == expected_pid ? "same-process-different-window" : "different-process");
}

int fit_window(const std::string& request) {
    std::istringstream input(request);
    std::string id_text;
    std::string pid_text;
    std::string extra;
    if (!(input >> id_text >> pid_text) || (input >> extra)
        || id_text.empty() || pid_text.empty()
        || !std::all_of(id_text.begin(), id_text.end(), [](unsigned char c) { return std::isdigit(c); })
        || !std::all_of(pid_text.begin(), pid_text.end(), [](unsigned char c) { return std::isdigit(c); }))
        return fit_failure("request");
    std::uintmax_t id_value = 0;
    std::uintmax_t pid_value = 0;
    std::istringstream id_input(id_text), pid_input(pid_text);
    if (!(id_input >> id_value) || !(pid_input >> pid_value)
        || id_value == 0 || pid_value == 0
        || id_value > (std::numeric_limits<std::uintptr_t>::max)()
        || pid_value > (std::numeric_limits<DWORD>::max)()) return fit_failure("request");
    auto id = static_cast<std::uintptr_t>(id_value);
    auto expected_pid = static_cast<DWORD>(pid_value);
    HWND window = reinterpret_cast<HWND>(id);
    DWORD actual_pid = 0;
    if (!GetWindowThreadProcessId(window, &actual_pid)) return fit_failure("identity-read");
    // The caller already checked launch ownership. Revalidate identity and focus
    // before changing only that window; never activate or move another app.
    if (actual_pid != expected_pid) return fit_failure("identity-mismatch");
    HWND foreground = GetForegroundWindow();
    if (!foreground) return fit_failure("foreground-read", "identity-unavailable");
    if (foreground != window)
        return fit_foreground_failure("foreground-mismatch", foreground, expected_pid);
    MONITORINFO monitor = {};
    monitor.cbSize = sizeof(monitor);
    HMONITOR monitor_handle = MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST);
    if (!monitor_handle) return fit_failure("monitor-read");
    if (!GetMonitorInfo(monitor_handle, &monitor)) return fit_failure("workarea-read");
    RECT rect;
    if (!GetWindowRect(window, &rect)) return fit_failure("window-read");
    auto work = monitor.rcWork;
    if (rect.left >= work.left && rect.top >= work.top
        && rect.right <= work.right && rect.bottom <= work.bottom) return 0;
    int width = work.right - work.left - 32;
    int height = work.bottom - work.top - 32;
    if (width < 300 || height < 200) return fit_failure("workarea-invalid");
    if (IsZoomed(window)) ShowWindow(window, SW_RESTORE);
    if (!GetWindowThreadProcessId(window, &actual_pid)) return fit_failure("identity-read");
    if (actual_pid != expected_pid) return fit_failure("identity-changed");
    foreground = GetForegroundWindow();
    if (!foreground) return fit_failure("foreground-read", "identity-unavailable");
    if (foreground != window)
        return fit_foreground_failure("foreground-changed", foreground, expected_pid);
    if (!SetWindowPos(window, nullptr, work.left + 16, work.top + 16, width, height,
                      SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOOWNERZORDER))
        return fit_failure("resize");
    if (!GetWindowThreadProcessId(window, &actual_pid))
        return fit_failure("postcondition-identity-read");
    if (actual_pid != expected_pid)
        return fit_failure("postcondition-identity-mismatch");
    foreground = GetForegroundWindow();
    if (!foreground)
        return fit_failure("postcondition-foreground-read");
    if (foreground != window)
        return fit_failure("postcondition-foreground-mismatch");
    if (!GetWindowRect(window, &rect))
        return fit_failure("postcondition-window-read");
    if (!contains_rect(work, rect))
        return fit_failure("postcondition-geometry");
    return 0;
}

static std::string process_name(DWORD pid);

// Read only the already-owned window's visibility; this never activates it.
int window_state(const std::string& request) {
    std::istringstream input(request);
    std::string id_text, pid_text, extra;
    if (!(input >> id_text >> pid_text) || (input >> extra)
        || id_text.empty() || pid_text.empty()
        || id_text.find_first_not_of("0123456789") != std::string::npos
        || pid_text.find_first_not_of("0123456789") != std::string::npos) return 5;
    std::uintmax_t id = 0, pid = 0;
    std::istringstream id_input(id_text), pid_input(pid_text);
    if (!(id_input >> id) || !(pid_input >> pid) || !id || !pid
        || id > (std::numeric_limits<std::uintptr_t>::max)()
        || pid > (std::numeric_limits<DWORD>::max)()) return 5;
    HWND window = reinterpret_cast<HWND>(static_cast<std::uintptr_t>(id));
    const char* state = "visible";
    DWORD actual_pid = 0, cloaked = 0;
    RECT bounds = {};
    if (!IsWindow(window)) state = "gone";
    else if (!GetWindowThreadProcessId(window, &actual_pid)) state = "query-unavailable";
    else if (actual_pid != pid) state = "identity-changed";
    else if (IsIconic(window)) state = "minimized";
    else if (!IsWindowVisible(window)) state = "hidden";
    else if (SUCCEEDED(DwmGetWindowAttribute(window, DWMWA_CLOAKED, &cloaked, sizeof(cloaked))) && cloaked) state = "cloaked";
    else if (GetAncestor(window, GA_ROOT) != window) state = "child-window";
    else if (!GetWindowRect(window, &bounds)) state = "query-unavailable";
    else if (bounds.right - bounds.left < 300 || bounds.bottom - bounds.top < 200) state = "candidate-too-small";
    else if (process_name(actual_pid).empty()) state = "process-name-unavailable";
    std::cout << state << '\n';
    return std::cout ? 0 : 5;
}

static std::string process_name(DWORD pid) {
    HANDLE process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid);
    if (!process) return {};
    char path[4096] = {};
    DWORD length = sizeof(path);
    bool read = QueryFullProcessImageNameA(process, 0, path, &length);
    CloseHandle(process);
    if (!read) return {};
    std::string name(path, length);
    auto slash = name.find_last_of("\\/");
    if (slash != std::string::npos) name.erase(0, slash + 1);
    return name.size() <= 256 ? name : std::string();
}

static BOOL CALLBACK display_record(HMONITOR monitor, HDC, LPRECT, LPARAM) {
    MONITORINFO info = {};
    info.cbSize = sizeof(info);
    if (!GetMonitorInfo(monitor, &info)) return FALSE;
    RECT rect = info.rcMonitor;
    std::cout << "DISPLAY " << rect.left << ' ' << rect.top << ' '
              << rect.right - rect.left << ' ' << rect.bottom - rect.top << '\n';
    return TRUE;
}

static BOOL CALLBACK enumerate_window(HWND window, LPARAM context) {
    auto count = reinterpret_cast<unsigned*>(context);
    if (++*count > 1024) return FALSE;
    if (!IsWindowVisible(window) || IsIconic(window)) return TRUE;
    DWORD cloaked = 0;
    if (SUCCEEDED(DwmGetWindowAttribute(window, DWMWA_CLOAKED, &cloaked, sizeof(cloaked))) && cloaked) return TRUE;
    RECT rect;
    if (!GetWindowRect(window, &rect)) return FALSE;
    if (rect.right <= rect.left || rect.bottom <= rect.top) return TRUE;
    DWORD pid = 0;
    GetWindowThreadProcessId(window, &pid);
    window_record(reinterpret_cast<std::uintptr_t>(window), pid, rect.left, rect.top,
                  rect.right - rect.left, rect.bottom - rect.top, process_name(pid));
    return TRUE;
}

int list_windows(bool include_foreground) {
    HWND foreground = include_foreground ? GetForegroundWindow() : nullptr;
    DWORD pid = 0;
    if (foreground) GetWindowThreadProcessId(foreground, &pid);
    std::cout << "FG " << pid << ' ' << reinterpret_cast<std::uintptr_t>(foreground) << '\n';
    if (!EnumDisplayMonitors(nullptr, nullptr, display_record, 0)) return 5;
    unsigned count = 0;
    if (!EnumWindows(enumerate_window, reinterpret_cast<LPARAM>(&count)) || count > 1024) return 5;
    return std::cout ? 0 : 5;
}
#else
#include <X11/Xatom.h>
#include <X11/Xlib.h>
#include <fstream>
#include <vector>
#include "x11_grab.hpp"

// Exit 5: unavailable session or exceeded workload; 6: BadWindow inside the grabbed
// snapshot; 7: any other rejected query. Only a complete snapshot is ever printed.
static bool query_failed = false;
static int query_exit = 5;
static const char* query_stage = "open";
// BadWindow for this exact resource is expected evidence, not a failed snapshot.
static Window probe_window = None;
static bool probe_missing = false;
// Every round trip made while the server is grabbed spends this bounded budget.
static unsigned query_budget = 16384;
static Atom net_wm_pid = None;

static bool spend(unsigned count) {
    if (query_failed || query_budget < count) {
        query_failed = true;
        return false;
    }
    query_budget -= count;
    return true;
}

// The grab freezes every other client, so it is bounded twice: by the query budget
// and by the hard deadline armed in x11_grab::acquire before any grab is requested.
struct XlibGrab {
    Display* display;
    x11_grab::Handler install(int number, x11_grab::Handler handler) {
        return std::signal(number, handler);
    }
    int arm(const itimerval* timer) { return setitimer(ITIMER_REAL, timer, nullptr); }
    void grab() {
        XGrabServer(display);
        XSync(display, False);
    }
    void ungrab() {
        XUngrabServer(display);
        XSync(display, False);
    }
};

static unsigned long property(Display* display, Window window, Atom atom, Atom kind) {
    Atom type = None;
    int format = 0;
    unsigned long count = 0, remaining = 0;
    unsigned char* data = nullptr;
    if (atom == None || !spend(1)) return 0;
    auto status = XGetWindowProperty(display, window, atom, 0, 1, False, kind,
                                    &type, &format, &count, &remaining, &data);
    unsigned long value = 0;
    if (status == Success && type == kind && format == 32 && count == 1 && data)
        value = *reinterpret_cast<unsigned long*>(data);
    if (data) XFree(data);
    return value;
}

static std::uint32_t process_id(Display* display, Window window, unsigned depth = 0) {
    auto pid = property(display, window, net_wm_pid, XA_CARDINAL);
    if (pid || depth == 3 || !spend(1)) return static_cast<std::uint32_t>(pid);
    Window root, parent, *children = nullptr;
    unsigned count = 0;
    if (!XQueryTree(display, window, &root, &parent, &children, &count)) {
        query_failed = true;
        return 0;
    }
    if (count > 1024) query_failed = true;
    for (unsigned index = 0; index < count && !query_failed && !pid; ++index)
        pid = process_id(display, children[index], depth + 1);
    if (children) XFree(children);
    return static_cast<std::uint32_t>(pid);
}

// Ownership is attributed to the root child, exactly like inventory records.
static Window top_level(Display* display, Window window, Window root) {
    for (unsigned depth = 0; depth < 32 && spend(1); ++depth) {
        Window returned_root, parent, *children = nullptr;
        unsigned count = 0;
        if (!XQueryTree(display, window, &returned_root, &parent, &children, &count)) {
            query_failed = true;
            return None;
        }
        if (children) XFree(children);
        if (parent == root) return window;
        if (parent == None) return None;
        window = parent;
    }
    return None;
}

static bool window_exists(Display* display, Window window) {
    if (!spend(2)) return false;
    XWindowAttributes attributes;
    probe_window = window;
    probe_missing = false;
    XGetWindowAttributes(display, window, &attributes);
    XSync(display, False);
    probe_window = None;
    return !probe_missing;
}

static std::string process_name(std::uint32_t pid) {
    std::ifstream file("/proc/" + std::to_string(pid) + "/comm");
    char name[256] = {};
    file.getline(name, sizeof(name));
    return name;
}

struct Record {
    Window id;
    std::uint32_t pid;
    int x, y;
    unsigned width, height;
};

int list_windows(bool include_foreground) {
    Display* display = XOpenDisplay(nullptr);
    if (!display) return 5;
    XSetErrorHandler([](Display*, XErrorEvent* error) {
        if (probe_window != None && error->error_code == BadWindow
            && error->resourceid == probe_window) {
            probe_missing = true;
            return 0;
        }
        if (!query_failed) {
            query_exit = error->error_code == BadWindow ? 6 : 7;
            // Fixed stage and numeric protocol metadata only; never titles or pixels.
            std::cerr << "X11 inventory failure: stage=" << query_stage
                      << " error=" << unsigned(error->error_code)
                      << " request=" << unsigned(error->request_code)
                      << " minor=" << unsigned(error->minor_code) << '\n';
        }
        query_failed = true;
        return 0;
    });
    Window root = DefaultRootWindow(display);
    Window foreground = None;
    std::uint32_t pid = 0;
    std::vector<Record> records;
    // Focus, stacking, attributes and ownership come from one frozen server state.
    // Without the grab, a destroyed popup or child makes every retry equally partial.
    XlibGrab grab{display};
    if (!x11_grab::acquire(grab)) {
        XCloseDisplay(display);
        return 5;
    }
    {
        query_stage = "atoms";
        Atom active = None;
        if (spend(2)) {
            net_wm_pid = XInternAtom(display, "_NET_WM_PID", True);
            active = XInternAtom(display, "_NET_ACTIVE_WINDOW", True);
        }
        if (include_foreground) {
            query_stage = "foreground";
            Window hint = property(display, root, active, XA_WINDOW);
            if (hint && window_exists(display, hint)) {
                foreground = hint;
                query_stage = "foreground-pid";
                pid = process_id(display, foreground);
            } else if (spend(1)) {
                // EWMH focus is a window-manager hint and may retain a destroyed ID.
                // Server focus is exact under the grab and reverts once unviewable.
                int revert;
                XGetInputFocus(display, &foreground, &revert);
                if (foreground > PointerRoot) {
                    query_stage = "focus-owner";
                    foreground = top_level(display, foreground, root);
                    pid = foreground ? process_id(display, foreground) : 0;
                }
            }
        }
        Window returned_root, parent, *children = nullptr;
        unsigned count = 0;
        query_stage = "root-tree";
        if (spend(1) && !XQueryTree(display, root, &returned_root, &parent, &children, &count))
            query_failed = true;
        if (count > 1024) query_failed = true;
        // XQueryTree is bottom-to-top; every platform emits front-to-back.
        for (unsigned index = count; index > 0 && !query_failed; --index) {
            auto window = children[index - 1];
            XWindowAttributes attributes;
            query_stage = "window-attributes";
            if (!spend(2) || !XGetWindowAttributes(display, window, &attributes)) {
                query_failed = true;
                break;
            }
            if (attributes.map_state != IsViewable || attributes.c_class == InputOnly) continue;
            int x = 0, y = 0;
            Window child;
            query_stage = "window-coordinates";
            if (!spend(1) || !XTranslateCoordinates(display, window, root, 0, 0, &x, &y, &child)) {
                query_failed = true;
                break;
            }
            query_stage = "window-pid";
            auto owner = process_id(display, window);
            records.push_back({window, owner, x, y, unsigned(attributes.width),
                               unsigned(attributes.height)});
        }
        if (children) XFree(children);
    }
    query_stage = "release";
    if (!x11_grab::release(grab) && !query_failed) {
        query_failed = true;
        query_exit = 5;
    }
    auto width = DisplayWidth(display, DefaultScreen(display));
    auto height = DisplayHeight(display, DefaultScreen(display));
    XCloseDisplay(display);
    if (query_failed) return query_exit;
    // Output and /proc reads happen after release so a slow reader cannot extend the grab.
    std::cout << "FG " << pid << ' ' << foreground << '\n';
    std::cout << "DISPLAY 0 0 " << width << ' ' << height << '\n';
    for (const auto& record : records)
        window_record(record.id, record.pid, record.x, record.y, record.width, record.height,
                      process_name(record.pid));
    return std::cout ? 0 : 5;
}
#endif
