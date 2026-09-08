// Only run under the private Xvfb session created by test-desktop-check-x11.sh.
#include <X11/Xatom.h>
#include <X11/Xlib.h>

int main() {
    Display* display = XOpenDisplay(nullptr);
    if (!display) return 1;
    Window root = DefaultRootWindow(display);
    Window window = XCreateSimpleWindow(display, root, 0, 0, 100, 100, 0, 0, 0);
    Atom active = XInternAtom(display, "_NET_ACTIVE_WINDOW", False);
    XChangeProperty(display, root, active, XA_WINDOW, 32, PropModeReplace,
                    reinterpret_cast<unsigned char*>(&window), 1);
    XDestroyWindow(display, window);
    XSync(display, False);
    XCloseDisplay(display);
    return 0;
}
