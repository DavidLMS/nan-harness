//! Releasing a detached helper from the standard handles it inherited from its launcher.
//!
//! nan-harness starts long-lived helpers — the shared request coordinator and the standalone
//! `SearXNG` host — that must outlive one launcher by design. Windows copies every inheritable
//! handle from the launcher into such a child, so the helper keeps a copy of the launcher's
//! stdout and stderr even when its own standard streams point at the null device. While the
//! helper lives, a pipe attached to the launcher never reaches end of file, and readers such as
//! pipelines, scripts, and CI steps wait for a launcher that already exited.
//!
//! [`release_inherited_standard_handles`] replaces this process's standard handles with the null
//! device and closes the handles it inherited, so a helper stops holding the launcher's pipes as
//! soon as it starts. Call it before anything writes to standard output or error: a cached handle
//! stays closed after the replacement. On platforms whose process model already gives a detached
//! helper its own descriptors the function is a no-op.

#![cfg_attr(
    windows,
    expect(
        unsafe_code,
        reason = "the single audited Windows handle replacement recorded in CONTRIBUTING.md"
    )
)]

#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub use windows::release_inherited_standard_handles;

/// Replaces this process's standard handles with the null device.
///
/// Unix-like platforms already give a detached helper its own descriptors, so the helper never
/// holds the launcher's pipes and the function does nothing.
///
/// # Errors
///
/// Returns the underlying I/O error when the null device or one of the standard handles cannot be
/// replaced on Windows.
#[cfg(not(windows))]
pub fn release_inherited_standard_handles() -> std::io::Result<()> {
    Ok(())
}
