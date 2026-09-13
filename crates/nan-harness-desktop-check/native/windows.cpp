// Window ownership and stacking metadata only; never reads another window's text or pixels.
#include <cstdint>
#include <array>
#include <algorithm>
#include <cctype>
#include <charconv>
#include <iomanip>
#include <iostream>
#include <limits>
#include <string>
#include <sstream>

#if !defined(_WIN32)
int fit_window(const std::string&) { return 5; }
#endif

#if !defined(__APPLE__)
int activate_window(const std::string&) { return 5; }
int observe_claude() { return 5; }
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
#include <libproc.h>

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

int list_windows(bool include_foreground) {
    @autoreleasepool {
        auto foreground = include_foreground
            ? [[[NSWorkspace sharedWorkspace] frontmostApplication] processIdentifier] : 0;
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
        CFRelease(windows);
        return std::cout ? 0 : 5;
    }
}

static void observation(const char* state) {
    std::cout << "OBS " << state << '\n';
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
        if (visible == 0) observation("matching-process-no-visible-window");
        else if (named == 0) observation("window-name-mismatch");
        else if (eligible == 0) observation("window-not-eligible");
        else observation("window-eligible");
        return std::cout ? 0 : 5;
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
