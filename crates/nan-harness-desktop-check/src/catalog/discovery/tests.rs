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
    assert_eq!(
        select(vec![dangling]),
        Err(DiscoveryError::CandidateCanonicalization)
    );
}

#[cfg(unix)]
#[test]
fn official_chatgpt_launcher_normalizes_to_the_direct_executable() {
    let root = tempfile::tempdir().expect("fixture");
    let lib = root.path().join("lib/chatgpt");
    fs::create_dir_all(&lib).expect("install");
    let launcher = lib.join("codex-launcher");
    let direct = lib.join("ChatGPT");
    fs::write(
        &launcher,
        b"#!/bin/sh\nexec \"$(dirname \"$0\")/ChatGPT\" \"$@\"\n",
    )
    .expect("wrapper");
    fs::write(&direct, b"ELF fixture").expect("direct");
    let bin = root.path().join("bin");
    fs::create_dir(&bin).expect("bin");
    let alias = bin.join("chatgpt");
    std::os::unix::fs::symlink(&launcher, &alias).expect("alias");
    assert_eq!(
        select(vec![alias, direct.clone()]),
        Ok(Some(fs::canonicalize(direct).expect("canonical direct")))
    );
}

#[cfg(unix)]
#[test]
fn official_chatgpt_direct_target_symlink_is_deduplicated() {
    let root = tempfile::tempdir().expect("fixture");
    let lib = root.path().join("lib/chatgpt");
    fs::create_dir_all(&lib).expect("install");
    let launcher = lib.join("codex-launcher");
    let direct = lib.join("ChatGPT");
    let target = lib.join("ChatGPT-real");
    fs::write(&launcher, b"wrapper fixture").expect("wrapper");
    fs::write(&target, b"ELF fixture").expect("direct");
    std::os::unix::fs::symlink(&target, &direct).expect("direct alias");
    assert_eq!(
        select(vec![launcher.clone(), direct.clone(), target.clone()]),
        Ok(Some(fs::canonicalize(&target).expect("canonical target")))
    );
    fs::remove_file(&target).expect("remove target");
    assert_eq!(
        select(vec![launcher, direct]),
        Err(DiscoveryError::CandidateCanonicalization)
    );
}

#[cfg(unix)]
#[test]
fn a_launcher_named_wrapper_outside_a_lib_install_stays_ambiguous() {
    // The official launcher is only recognized when it sits under a `lib`
    // directory named `chatgpt`. A same-named wrapper elsewhere is a distinct
    // file and must not be silently merged with a sibling executable.
    let root = tempfile::tempdir().expect("fixture");
    let dir = root.path().join("chatgpt");
    fs::create_dir(&dir).expect("dir");
    let wrapper = dir.join("codex-launcher");
    let direct = dir.join("ChatGPT");
    fs::write(&wrapper, b"#!/bin/sh\nexec something-else \"$@\"\n").expect("wrapper");
    fs::write(&direct, b"ELF fixture").expect("direct");
    assert_eq!(
        select(vec![wrapper, direct]),
        Err(DiscoveryError::Ambiguous)
    );
}

#[cfg(unix)]
#[test]
fn launcher_with_a_missing_direct_target_fails_closed() {
    let root = tempfile::tempdir().expect("fixture");
    let lib = root.path().join("lib/chatgpt");
    fs::create_dir_all(&lib).expect("install");
    let launcher = lib.join("codex-launcher");
    fs::write(&launcher, b"#!/bin/sh\n").expect("wrapper");
    let bin = root.path().join("bin");
    fs::create_dir(&bin).expect("bin");
    let alias = bin.join("chatgpt");
    std::os::unix::fs::symlink(&launcher, &alias).expect("alias");
    assert_eq!(
        select(vec![alias]),
        Err(DiscoveryError::CandidateCanonicalization)
    );
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

#[test]
fn appx_install_locations_accept_multiple_absolute_lines_and_reject_relative_lines() {
    let root = tempfile::tempdir().expect("fixture");
    let locations = parse_windows_install_locations(&format!(
        "{}\n{}\n",
        root.path().join("one").display(),
        root.path().join("two").display()
    ))
    .expect("absolute locations");
    assert_eq!(locations.len(), 2);
    assert!(parse_windows_install_locations("relative\\package").is_err());
}
