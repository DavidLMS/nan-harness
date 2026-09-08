use super::*;

#[test]
fn zed_bundle_entry_points_resolve_to_the_managed_cli() {
    let root = tempfile::tempdir().expect("fixture");
    let directory = root.path().join("Zed.app/Contents/MacOS");
    fs::create_dir_all(&directory).expect("bundle");
    let gui = directory.join("zed");
    let cli = directory.join("cli");
    fs::write(&gui, b"GUI executable").expect("GUI fixture");
    fs::write(&cli, b"CLI executable").expect("CLI fixture");
    assert_eq!(
        select(vec![gui, cli.clone()]),
        Ok(Some(fs::canonicalize(cli).expect("canonical CLI")))
    );
}

#[test]
fn windows_zed_bundle_uses_its_cli_and_rejects_a_missing_shim() {
    let root = tempfile::tempdir().expect("fixture");
    let directory = root.path().join("Zed");
    fs::create_dir_all(directory.join("bin")).expect("bundle");
    let gui = directory.join("Zed.exe");
    let cli = directory.join("bin/zed.exe");
    fs::write(&gui, b"GUI executable").expect("GUI fixture");
    assert_eq!(select(vec![gui.clone()]), Err(DiscoveryError::Incomplete));
    fs::write(&cli, b"CLI executable").expect("CLI fixture");
    assert_eq!(
        select(vec![gui, cli.clone()]),
        Ok(Some(fs::canonicalize(cli).expect("canonical CLI")))
    );
}

#[test]
fn distinct_installs_are_ambiguous_and_missing_is_absent() {
    let root = tempfile::tempdir().expect("fixture");
    let first = root.path().join("first");
    let second = root.path().join("second");
    fs::write(&first, b"fixture").expect("first fixture");
    fs::write(&second, b"fixture").expect("second fixture");
    assert_eq!(
        select(vec![first.clone(), second]),
        Err(DiscoveryError::Ambiguous)
    );
    assert_eq!(
        select(vec![first.clone(), first.clone()]),
        Ok(Some(fs::canonicalize(first).expect("canonical fixture")))
    );
    assert_eq!(select(vec![root.path().join("missing")]), Ok(None));
}

#[test]
fn malformed_installation_is_not_absent() {
    let root = tempfile::tempdir().expect("fixture");
    assert_eq!(
        select(vec![root.path().to_path_buf()]),
        Err(DiscoveryError::Incomplete)
    );
    let bundle = root.path().join("ChatGPT.app");
    fs::create_dir(&bundle).expect("bundle");
    assert_eq!(
        add_bundle(&mut Vec::new(), &bundle, "ChatGPT"),
        Err(DiscoveryError::Incomplete)
    );
}

#[cfg(unix)]
#[test]
fn aliases_are_deduplicated_but_dangling_links_fail_closed() {
    let root = tempfile::tempdir().expect("fixture");
    let executable = root.path().join("binary");
    let alias = root.path().join("alias");
    fs::write(&executable, b"fixture").expect("executable");
    std::os::unix::fs::symlink(&executable, &alias).expect("alias");
    assert_eq!(
        select(vec![executable.clone(), alias]),
        Ok(Some(
            fs::canonicalize(executable).expect("canonical fixture")
        ))
    );
    let dangling = root.path().join("dangling");
    std::os::unix::fs::symlink(root.path().join("missing"), &dangling).expect("dangling");
    assert_eq!(select(vec![dangling]), Err(DiscoveryError::Unreadable));
}

#[test]
fn desktop_catalog_does_not_mistake_cli_names_for_apps() {
    assert_eq!(
        executable_name(DesktopHarnessKind::Hermes, Platform::Linux),
        "hermes-desktop"
    );
    assert_eq!(
        executable_name(DesktopHarnessKind::Claude, Platform::Linux),
        "claude-desktop"
    );
}
