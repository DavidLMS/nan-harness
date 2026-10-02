use super::*;

#[test]
fn entrypoint_binding_preserves_upstream_startup_and_rejects_drift() {
    let source = "const exitCode = await run(context, {\n existing: true\n});\nvoid main();\n";
    let bound = binding::entrypoint(source).unwrap();
    assert!(bound.contains("existing: true"));
    assert!(bound.contains("projectConfigPath: process.env.NAN_HARNESS_ZCODE_PROJECT_CONFIG_FILE"));
    assert!(bound.contains(BINDING));
    let headless = "const app = await createApp({\n      env: appEnv,\n});";
    assert!(
        binding::headless_entrypoint(headless)
            .unwrap()
            .contains("projectConfigPath: deps.projectConfigPath")
    );
    assert!(binding::headless_entrypoint("changed entrypoint").is_err());
    assert!(binding::headless_entrypoint(&format!("{headless}{headless}")).is_err());
    assert!(binding::entrypoint("void main();").is_err());
    assert!(binding::entrypoint(&format!("{source}{source}")).is_err());
}

#[test]
fn source_installer_preserves_foreign_commands_and_partial_installations() {
    let root = tempfile::tempdir().unwrap();
    let command = root.path().join("zcode");
    fs::write(&command, "user-owned-command").unwrap();
    assert!(check_launcher(&command, &launcher(false)).is_err());
    assert_eq!(fs::read_to_string(&command).unwrap(), "user-owned-command");
    let source = root.path().join(REVISION);
    fs::create_dir(&source).unwrap();
    fs::write(source.join(RECEIPT), "foreign revision").unwrap();
    assert!(prepare_source(root.path()).is_err());
    assert_eq!(
        fs::read_to_string(source.join(RECEIPT)).unwrap(),
        "foreign revision"
    );
}

#[test]
fn source_launchers_quote_paths_and_preserve_native_arguments() {
    let unix = launcher(false);
    assert!(unix.contains("\"$@\""));
    assert!(unix.contains(REVISION));
    let windows = launcher(true);
    assert!(windows.contains("node \"%~dp0..\\share"));
    assert!(windows.ends_with(" %*\r\n"));
}

#[test]
#[ignore = "downloads and builds the pinned official ZCode source; requires Git, Node 24.14 and pnpm 10.33.2"]
fn zcode_official_source_installation_is_repeatable() {
    check_prerequisites().unwrap();
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join(".zcode/cli/config.json");
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    fs::write(&config, r#"{"theme":"user-owned"}"#).unwrap();
    install_into(home.path()).unwrap();
    install_into(home.path()).unwrap();
    assert_eq!(
        fs::read_to_string(&config).unwrap(),
        r#"{"theme":"user-owned"}"#
    );
    let source = home
        .path()
        .join(".local/share/nan-harness/zcode")
        .join(REVISION);
    verify(&source).unwrap();
    let command =
        home.path()
            .join(".local/bin")
            .join(if cfg!(windows) { "zcode.cmd" } else { "zcode" });
    let output = run_command(command.as_os_str(), &["version"], Command::output).unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout.trim_ascii(), b"0.16.9");
}
