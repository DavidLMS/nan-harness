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

int list_windows() {
    @autoreleasepool {
        auto foreground = [[[NSWorkspace sharedWorkspace] frontmostApplication] processIdentifier];
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
            window_record(number(window, kCGWindowNumber), pid, bounds.origin.x, bounds.origin.y,
                          bounds.size.width, bounds.size.height, name);
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

int list_windows() {
    HWND foreground = GetForegroundWindow();
    DWORD pid = 0;
    GetWindowThreadProcessId(foreground, &pid);
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

static bool query_failed = false;
static const char* query_stage = "open";

static unsigned long property(Display* display, Window window, const char* name, Atom kind) {
    Atom type = None;
    int format = 0;
    unsigned long count = 0, remaining = 0;
    unsigned char* data = nullptr;
    Atom atom = XInternAtom(display, name, True);
    if (atom == None) return 0;
    auto status = XGetWindowProperty(display, window, atom, 0, 1, False, kind,
                                    &type, &format, &count, &remaining, &data);
    unsigned long value = 0;
    if (status == Success && type == kind && format == 32 && count == 1 && data)
        value = *reinterpret_cast<unsigned long*>(data);
    if (data) XFree(data);
    return value;
}

static std::uint32_t process_id(Display* display, Window window, unsigned depth = 0) {
    auto pid = property(display, window, "_NET_WM_PID", XA_CARDINAL);
    if (pid || depth == 3) return static_cast<std::uint32_t>(pid);
    Window root, parent, *children = nullptr;
    unsigned count = 0;
    if (!XQueryTree(display, window, &root, &parent, &children, &count)) return 0;
    if (count > 1024) query_failed = true;
    for (unsigned index = 0; index < count && index < 1024 && !pid; ++index)
        pid = process_id(display, children[index], depth + 1);
    if (children) XFree(children);
    return static_cast<std::uint32_t>(pid);
}

static std::string process_name(std::uint32_t pid) {
    std::ifstream file("/proc/" + std::to_string(pid) + "/comm");
    char name[256] = {};
    file.getline(name, sizeof(name));
    return name;
}

int list_windows() {
    Display* display = XOpenDisplay(nullptr);
    if (!display) return 5;
    XSetErrorHandler([](Display*, XErrorEvent* error) {
        // Fixed stage and numeric protocol metadata only; never titles or pixels.
        if (!query_failed)
            std::cerr << "X11 inventory failure: stage=" << query_stage
                      << " error=" << unsigned(error->error_code)
                      << " request=" << unsigned(error->request_code)
                      << " minor=" << unsigned(error->minor_code)
                      << " resource=" << error->resourceid << '\n';
        query_failed = true;
        return 0;
    });
    Window root = DefaultRootWindow(display);
    query_stage = "foreground";
    Window foreground = property(display, root, "_NET_ACTIVE_WINDOW", XA_WINDOW);
    if (!foreground) {
        int revert;
        XGetInputFocus(display, &foreground, &revert);
    }
    query_stage = "foreground-pid";
    auto pid = foreground > PointerRoot ? process_id(display, foreground) : 0;
    std::cout << "FG " << pid << ' ' << foreground << '\n';
    std::cout << "DISPLAY 0 0 " << DisplayWidth(display, DefaultScreen(display)) << ' '
              << DisplayHeight(display, DefaultScreen(display)) << '\n';
    Window returned_root, parent, *children = nullptr;
    unsigned count = 0;
    query_stage = "root-tree";
    if (!XQueryTree(display, root, &returned_root, &parent, &children, &count) || count > 1024) {
        XCloseDisplay(display); return 5;
    }
    // XQueryTree is bottom-to-top; every platform emits front-to-back.
    for (unsigned index = count; index > 0; --index) {
        auto window = children[index - 1];
        XWindowAttributes attributes;
        query_stage = "window-attributes";
        if (!XGetWindowAttributes(display, window, &attributes)) continue;
        if (attributes.map_state != IsViewable || attributes.c_class == InputOnly) continue;
        int x = 0, y = 0;
        Window child;
        query_stage = "window-coordinates";
        if (!XTranslateCoordinates(display, window, root, 0, 0, &x, &y, &child)) continue;
        query_stage = "window-pid";
        auto owner = process_id(display, window);
        window_record(window, owner, x, y, attributes.width, attributes.height, process_name(owner));
    }
    if (children) XFree(children);
    XSync(display, False);
    XCloseDisplay(display);
    return query_failed || !std::cout ? 5 : 0;
}
#endif
