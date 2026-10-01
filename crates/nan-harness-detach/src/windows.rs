//! Windows guard that keeps this process's standard handles out of a new child process.
//!
//! `CreateProcessW` duplicates every inheritable handle of the caller into the child, so the guard
//! clears `HANDLE_FLAG_INHERIT` on the three standard handles while the child is created and restores
//! the flags afterwards. The standard library already duplicates a handle explicitly when a caller
//! asks a child to inherit a stream (`Stdio::Inherit`), and that duplicate carries inheritance from
//! the request rather than from the source flag, so a concurrent spawn keeps its own semantics.
//!
//! This module is the workspace's single audited exception to `unsafe_code`.

use std::sync::Mutex;
use windows_sys::Win32::Foundation::{
    GetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, SetHandleInformation,
};
use windows_sys::Win32::System::Console::{
    GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
};

const STANDARD_HANDLES: [u32; 3] = [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE];
static SPAWN_LOCK: Mutex<()> = Mutex::new(());

/// Runs `start` while no standard handle of this process is inheritable.
pub(super) fn without_inherited_standard_handles<T>(start: impl FnOnce() -> T) -> T {
    // The flags are process-wide: overlapping guards could restore inheritance while
    // another guarded spawn is still using it. Restore before releasing this lock.
    let _lock = SPAWN_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _guard = InheritanceGuard::engage();
    start()
}

/// The standard handles whose inheritance flag the guard cleared, restored when it is dropped.
struct InheritanceGuard {
    cleared: Vec<HANDLE>,
}

impl InheritanceGuard {
    /// Clears the inheritance flag of every standard handle that carries one.
    ///
    /// A standard handle that is absent or cannot be inspected is left exactly as the caller
    /// configured it: the guard narrows inheritance and never blocks a spawn.
    fn engage() -> Self {
        let mut cleared = Vec::new();
        for identifier in STANDARD_HANDLES {
            // SAFETY: the call only reads the standard handle value of this process; a null or
            // `INVALID_HANDLE_VALUE` result is a documented "no such handle" answer and is skipped.
            let handle = unsafe { GetStdHandle(identifier) };
            if handle.is_null() || handle == INVALID_HANDLE_VALUE {
                continue;
            }
            let mut flags = 0_u32;
            // SAFETY: `GetHandleInformation` writes the current flags of a valid handle into the
            // provided out-parameter and reports failure with a zero return value.
            let inspected = unsafe { GetHandleInformation(handle, &raw mut flags) };
            if inspected == 0 || flags & HANDLE_FLAG_INHERIT == 0 {
                continue;
            }
            // SAFETY: the handle is valid and was inspected above; clearing one flag changes only the
            // inheritance of this process's own handle, and `Drop` restores it.
            if unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) } != 0 {
                cleared.push(handle);
            }
        }
        Self { cleared }
    }
}

impl Drop for InheritanceGuard {
    fn drop(&mut self) {
        for handle in self.cleared.drain(..) {
            // SAFETY: the handle stays valid for the duration of the guard, so the original
            // inheritance flag can be restored. A failing restore leaves inheritance off, which is
            // the safe direction for this process's standard handles.
            let _ =
                unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::without_inherited_standard_handles;
    use std::sync::Barrier;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn concurrent_guarded_spawns_do_not_overlap() {
        let barrier = Barrier::new(8);
        let active = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    barrier.wait();
                    without_inherited_standard_handles(|| {
                        assert_eq!(active.fetch_add(1, Ordering::SeqCst), 0);
                        std::thread::sleep(std::time::Duration::from_millis(5));
                        assert_eq!(active.fetch_sub(1, Ordering::SeqCst), 1);
                    });
                });
            }
        });
    }
}
