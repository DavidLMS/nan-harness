//! Private one-shot handoff between retained UI readiness and the fixture gate.

use crate::report::Reason;
use nan_harness_private_fs::open_private_new;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

pub(super) struct RecoveryBarrier {
    ready: PathBuf,
    release: PathBuf,
    released: bool,
}

impl RecoveryBarrier {
    pub(super) fn new(request: &Path) -> Self {
        Self {
            ready: request.with_extension("retry-ready.private"),
            release: request.with_extension("retry-release.private"),
            released: false,
        }
    }

    pub(super) fn poll(&mut self, release: impl FnOnce()) -> Result<(), Reason> {
        if self.released {
            return Ok(());
        }
        match std::fs::symlink_metadata(&self.ready) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Ok(metadata) if metadata.file_type().is_file() => {}
            Err(_) | Ok(_) => return Err(Reason::IsolationUnavailable),
        }
        let mut bytes = Vec::new();
        open_signal(&self.ready)
            .and_then(|file| file.take(16).read_to_end(&mut bytes))
            .map_err(|_| Reason::IsolationUnavailable)?;
        if bytes.len() < 6 && b"ready\n".starts_with(&bytes) {
            return Ok(());
        }
        if bytes != b"ready\n" {
            return Err(Reason::IsolationUnavailable);
        }
        // Consume before releasing: an uncertain acknowledgement cannot replay
        // the gate transition or authorize a second UI action.
        self.released = true;
        release();
        open_private_new(&self.release)
            .and_then(|mut file| file.write_all(b"released\n"))
            .map_err(|_| Reason::IsolationUnavailable)
    }
}

// Signals are never repaired or followed: their parent is already retained by
// the session guard, and only the original private controller may publish them.
fn open_signal(path: &Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        options.custom_flags(0x0020_0000).share_mode(3);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    let mut private = metadata.is_file();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        private &= metadata.uid() == nix::unistd::geteuid().as_raw()
            && metadata.mode().trailing_zeros() >= 6
            && metadata.nlink() == 1;
    }
    #[cfg(windows)]
    {
        use nan_harness_private_fs::{
            OwnedWindowsDacl, PrivatePathKind, classify_owned_windows_dacl,
        };
        use std::os::windows::fs::MetadataExt as _;
        private &= metadata.file_attributes() & 0x400 == 0
            && matches!(
                classify_owned_windows_dacl(&file, PrivatePathKind::File),
                OwnedWindowsDacl::Protected | OwnedWindowsDacl::Inherited
            );
    }
    if !private {
        return Err(std::io::ErrorKind::PermissionDenied.into());
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_requires_exact_private_readiness_and_occurs_once() {
        let root = tempfile::tempdir().unwrap();
        let mut barrier = RecoveryBarrier::new(&root.path().join("request.private"));
        let mut releases = 0;
        barrier.poll(|| releases += 1).unwrap();
        assert_eq!(releases, 0);
        open_private_new(&barrier.ready)
            .unwrap()
            .write_all(b"ready\n")
            .unwrap();
        barrier.poll(|| releases += 1).unwrap();
        barrier.poll(|| releases += 1).unwrap();
        assert_eq!(releases, 1);
        assert_eq!(std::fs::read(&barrier.release).unwrap(), b"released\n");
    }

    #[test]
    fn incomplete_signal_waits_without_releasing_the_provider() {
        let root = tempfile::tempdir().unwrap();
        let mut barrier = RecoveryBarrier::new(&root.path().join("request.private"));
        let mut writer = open_private_new(&barrier.ready).unwrap();
        writer.write_all(b"rea").unwrap();
        barrier.poll(|| panic!("partial signal released")).unwrap();
        assert!(!barrier.release.exists());
        writer.write_all(b"dy\n").unwrap();
        barrier.poll(|| {}).unwrap();
        assert_eq!(std::fs::read(&barrier.release).unwrap(), b"released\n");
    }

    #[cfg(unix)]
    #[test]
    fn symlink_signal_cannot_release_the_provider() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("target.private");
        open_private_new(&target)
            .unwrap()
            .write_all(b"ready\n")
            .unwrap();
        let mut barrier = RecoveryBarrier::new(&root.path().join("request.private"));
        std::os::unix::fs::symlink(target, &barrier.ready).unwrap();
        assert!(barrier.poll(|| panic!("symlink signal released")).is_err());
    }

    #[test]
    fn malformed_or_existing_acknowledgement_never_authorizes_replay() {
        for bytes in [b"wrong".as_slice(), b"PRIVATE", b"ready\nextra"] {
            let root = tempfile::tempdir().unwrap();
            let mut barrier = RecoveryBarrier::new(&root.path().join("request.private"));
            open_private_new(&barrier.ready)
                .unwrap()
                .write_all(bytes)
                .unwrap();
            assert!(
                barrier
                    .poll(|| panic!("invalid readiness released"))
                    .is_err()
            );
            assert!(!barrier.release.exists());
        }
        let root = tempfile::tempdir().unwrap();
        let mut barrier = RecoveryBarrier::new(&root.path().join("request.private"));
        open_private_new(&barrier.ready)
            .unwrap()
            .write_all(b"ready\n")
            .unwrap();
        open_private_new(&barrier.release)
            .unwrap()
            .write_all(b"foreign\n")
            .unwrap();
        let mut releases = 0;
        assert!(barrier.poll(|| releases += 1).is_err());
        barrier.poll(|| releases += 1).unwrap();
        assert_eq!(releases, 1);
        assert_eq!(std::fs::read(&barrier.release).unwrap(), b"foreign\n");
    }
}
