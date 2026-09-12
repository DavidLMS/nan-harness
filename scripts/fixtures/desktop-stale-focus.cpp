// Only run under the private Xvfb session created by test-desktop-check-x11.sh.
#include <X11/Xatom.h>
#include <X11/Xlib.h>
#include <cstring>

static void set_active(Display* display, Window window) {
    Atom active = XInternAtom(display, "_NET_ACTIVE_WINDOW", False);
    XChangeProperty(display, DefaultRootWindow(display), active, XA_WINDOW, 32,
                    PropModeReplace, reinterpret_cast<unsigned char*>(&window), 1);
}

static Window destroyed_window(Display* display) {
    Window window = XCreateSimpleWindow(display, DefaultRootWindow(display), 0, 0, 100, 100,
                                        0, 0, 0);
    XDestroyWindow(display, window);
    return window;
}

// Retains a mapped, synthetic-PID window with server focus behind a stale EWMH hint.
static void focused(Display* display) {
    XSetCloseDownMode(display, RetainPermanent);
    Window root = DefaultRootWindow(display);
    Window frame = XCreateSimpleWindow(display, root, 10, 20, 300, 200, 0, 0, 0);
    Window client = XCreateSimpleWindow(display, frame, 0, 0, 300, 200, 0, 0, 0);
    unsigned long pid = 4242;
    XChangeProperty(display, client, XInternAtom(display, "_NET_WM_PID", False), XA_CARDINAL,
                    32, PropModeReplace, reinterpret_cast<unsigned char*>(&pid), 1);
    XMapWindow(display, client);
    XMapWindow(display, frame);
    XSync(display, False);
    XSetInputFocus(display, client, RevertToPointerRoot, CurrentTime);
    set_active(display, destroyed_window(display));
}

// Creates and destroys root children and nested children until the caller stops it.
static void churn(Display* display) {
    Window root = DefaultRootWindow(display);
    Window parent = XCreateSimpleWindow(display, root, 400, 300, 100, 100, 0, 0, 0);
    XMapWindow(display, parent);
    for (;;) {
        Window top = XCreateSimpleWindow(display, root, 0, 0, 50, 50, 0, 0, 0);
        Window child = XCreateSimpleWindow(display, parent, 0, 0, 10, 10, 0, 0, 0);
        XMapWindow(display, top);
        XMapWindow(display, child);
        XSync(display, False);
        XDestroyWindow(display, child);
        XDestroyWindow(display, top);
        XSync(display, False);
    }
}

int main(int argc, char** argv) {
    Display* display = XOpenDisplay(nullptr);
    if (!display || argc > 2) return 1;
    const char* mode = argc == 2 ? argv[1] : "stale";
    if (!std::strcmp(mode, "stale")) set_active(display, destroyed_window(display));
    else if (!std::strcmp(mode, "focused")) focused(display);
    else if (!std::strcmp(mode, "churn")) churn(display);
    else return 1;
    XSync(display, False);
    XCloseDisplay(display);
    return 0;
}
