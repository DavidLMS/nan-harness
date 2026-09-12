// Window ownership and stacking metadata only; never reads another window's text or pixels.
#include <cstdint>
#include <iomanip>
#include <iostream>
#include <string>
#include <sstream>

#if !defined(_WIN32)
int fit_window(const std::string&) { return 5; }
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
#elif defined(_WIN32)
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <dwmapi.h>

int fit_window(const std::string& request) {
    std::istringstream input(request);
    std::uintptr_t id = 0;
    DWORD expected_pid = 0;
    std::string extra;
    if (!(input >> id >> expected_pid) || (input >> extra) || !id || !expected_pid) return 5;
    HWND window = reinterpret_cast<HWND>(id);
    DWORD actual_pid = 0;
    GetWindowThreadProcessId(window, &actual_pid);
    // The caller already checked launch ownership. Revalidate identity and focus
    // before changing only that window; never activate or move another app.
    if (actual_pid != expected_pid || GetForegroundWindow() != window) return 5;
    MONITORINFO monitor = {};
    monitor.cbSize = sizeof(monitor);
    if (!GetMonitorInfo(MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST), &monitor)) return 5;
    RECT rect;
    if (!GetWindowRect(window, &rect)) return 5;
    auto work = monitor.rcWork;
    if (rect.left >= work.left && rect.top >= work.top
        && rect.right <= work.right && rect.bottom <= work.bottom) return 0;
    int width = work.right - work.left - 32;
    int height = work.bottom - work.top - 32;
    if (width < 300 || height < 200) return 5;
    if (IsZoomed(window)) ShowWindow(window, SW_RESTORE);
    GetWindowThreadProcessId(window, &actual_pid);
    if (actual_pid != expected_pid || GetForegroundWindow() != window) return 5;
    return SetWindowPos(window, nullptr, work.left + 16, work.top + 16, width, height,
                        SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOOWNERZORDER) ? 0 : 5;
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
