// Synthetic contract for the X11 grab deadline; never connects to an X server.
#include <cstdio>
#include <string>
#include "x11_grab.hpp"

struct FakeGrab {
    bool install_fails = false;
    bool arm_fails = false;
    bool disarm_fails = false;
    std::string calls;

    x11_grab::Handler install(int number, x11_grab::Handler handler) {
        calls += number == SIGALRM && handler ? "install " : "bad-install ";
        return install_fails ? SIG_ERR : SIG_DFL;
    }
    int arm(const itimerval* timer) {
        bool armed = timer->it_value.tv_sec || timer->it_value.tv_usec;
        bool bounded = timer->it_value.tv_sec == 2 && !timer->it_value.tv_usec
                       && !timer->it_interval.tv_sec && !timer->it_interval.tv_usec;
        calls += armed ? (bounded ? "arm " : "bad-arm ") : "disarm ";
        return (armed ? arm_fails : disarm_fails) ? -1 : 0;
    }
    void grab() { calls += "grab "; }
    void ungrab() { calls += "ungrab "; }
};

static int failures = 0;

static void expect(bool condition, const char* name) {
    if (condition) return;
    std::fprintf(stderr, "X11 grab arming contract failed: %s\n", name);
    ++failures;
}

int main() {
    FakeGrab signal_failure;
    signal_failure.install_fails = true;
    expect(!x11_grab::acquire(signal_failure), "signal failure is rejected");
    expect(signal_failure.calls == "install ", "signal failure never arms or grabs");

    FakeGrab timer_failure;
    timer_failure.arm_fails = true;
    expect(!x11_grab::acquire(timer_failure), "timer failure is rejected");
    expect(timer_failure.calls == "install arm ", "timer failure never grabs");

    FakeGrab armed;
    expect(x11_grab::acquire(armed), "armed deadline grabs");
    expect(x11_grab::release(armed), "release disarms");
    expect(armed.calls == "install arm grab ungrab disarm ",
           "grab happens only after the deadline and is released before disarming");

    FakeGrab disarm_failure;
    disarm_failure.disarm_fails = true;
    expect(x11_grab::acquire(disarm_failure), "disarm fixture grabs");
    expect(!x11_grab::release(disarm_failure), "failed disarm discards the snapshot");
    expect(disarm_failure.calls == "install arm grab ungrab disarm ",
           "failed disarm still releases the grab");
    return failures ? 1 : 0;
}
