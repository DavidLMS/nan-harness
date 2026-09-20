//! Keeping long-lived helpers free of the handles owned by the process that starts them.
//!
//! nan-harness starts helpers that must outlive one launcher by design: the shared request
//! coordinator and the standalone `SearXNG` host. Such a helper must never keep a handle it received
//! from its launcher.
//!
//! Windows copies **every** inheritable handle of the launcher into a new process, not only the
//! standard streams selected for that child. A helper started with `Stdio::null()` therefore still
//! receives a copy of the launcher's standard handles, and a pipe attached to the launcher stays open
//! for every reader — a pipeline, a script, or a CI step — until the helper itself exits. The
//! standard library offers no stable way to decline that inheritance (`CommandExt::inherit_handles`
//! is unstable), so [`without_inherited_standard_handles`] clears `HANDLE_FLAG_INHERIT` on this
//! process's standard handles for the duration of one spawn and restores it afterwards. That is the
//! stable equivalent of `bInheritHandles = FALSE` for them, and the reason this crate carries the
//! workspace's single `unsafe_code` exception.
//!
//! Unix-like platforms install the child's descriptors at `exec`, so a detached helper already owns
//! nothing of its launcher and the guard only runs the spawn.

#![cfg_attr(
    windows,
    expect(
        unsafe_code,
        reason = "the audited Windows handle-inheritance guard recorded in CONTRIBUTING.md"
    )
)]

#[cfg(windows)]
mod windows;

/// Runs `start` while this process's standard handles cannot be inherited by a new child.
///
/// Callers pass exactly the spawn that must inherit nothing, and keep their own choice of streams:
/// a helper that outlives its launcher normally starts with the null device on all three, so that it
/// never writes into a stream it does not own. Platform-specific process creation (job breakaway
/// flags, process groups) stays with the caller and is preserved.
///
/// On Unix-like platforms the guard only runs `start`, because `exec` already replaces the child's
/// descriptors.
pub fn without_inherited_standard_handles<T>(start: impl FnOnce() -> T) -> T {
    platform(start)
}

#[cfg(windows)]
fn platform<T>(start: impl FnOnce() -> T) -> T {
    windows::without_inherited_standard_handles(start)
}

#[cfg(not(windows))]
fn platform<T>(start: impl FnOnce() -> T) -> T {
    start()
}
