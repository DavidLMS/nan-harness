use super::*;

fn paths() -> (tempfile::TempDir, DesktopPaths) {
    let root = tempfile::tempdir().expect("temporary root");
    let paths = DesktopPaths::for_test(root.path());
    (root, paths)
}

mod compatibility;
mod diagnostic_recovery;
mod diagnostics;
mod profiles;
mod recovery_transactions;
mod session;
