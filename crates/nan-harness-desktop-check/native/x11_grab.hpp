// Bounded X11 server grab. Only included by the X11 inventory and its synthetic tests.
#pragma once
#include <sys/time.h>
#include <unistd.h>
#include <csignal>

namespace x11_grab {

using Handler = void (*)(int);

// Exiting closes the connection, and the X protocol releases a server grab when its
// client connection closes. Exit 5 is the closed "unavailable" inventory category.
inline void expired(int) { _exit(5); }

// Ops supplies install(signal, handler) -> Handler, arm(const itimerval*) -> int with
// setitimer semantics, and grab()/ungrab(). The grab is requested only after the hard
// deadline exists; any arming failure returns false with the server untouched.
template <class Ops>
bool acquire(Ops& ops) {
    if (ops.install(SIGALRM, expired) == SIG_ERR) return false;
    itimerval timer = {};
    timer.it_value.tv_sec = 2;
    if (ops.arm(&timer) != 0) return false;
    ops.grab();
    return true;
}

// Releases before disarming. A failed disarm could still end the process later, so
// the caller must discard the snapshot instead of printing it.
template <class Ops>
bool release(Ops& ops) {
    ops.ungrab();
    itimerval timer = {};
    return ops.arm(&timer) == 0;
}

}  // namespace x11_grab
