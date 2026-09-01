use super::*;

fn paths() -> (tempfile::TempDir, DesktopPaths) {
    let root = tempfile::tempdir().expect("temporary root");
    let paths = DesktopPaths::for_test(root.path());
    (root, paths)
}

mod compatibility;
mod diagnostics;
mod profiles;
mod session;
