#[cfg(unix)]
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::thread;
use std::time::{Duration, Instant};

use crate::platform::{alias_path, installer_command, installer_file_name};

pub(crate) fn run_installer(
    root: &Path,
    home: &Path,
    install_directory: &Path,
    state_directory: &Path,
    base_url: &str,
) -> Output {
    installer_process(root, home, install_directory, state_directory, base_url)
        .output()
        .expect("release installer should start")
}

pub(crate) fn installer_process(
    root: &Path,
    home: &Path,
    install_directory: &Path,
    state_directory: &Path,
    base_url: &str,
) -> Command {
    let script = repository_root().join(installer_file_name());
    let mut command = installer_command(&script);
    isolate_user_environment(&mut command, root, home, state_directory);
    command
        .current_dir(root)
        .env("NAN_INSTALL_BASE_URL", base_url)
        .env("NAN_INSTALL_DIR", install_directory)
        .env("NO_PROXY", "127.0.0.1,localhost");
    command
}

#[cfg(windows)]
pub(crate) fn installer_process_with_architecture(
    root: &Path,
    home: &Path,
    install_directory: &Path,
    state_directory: &Path,
    base_url: &str,
    process_architecture: &str,
    native_architecture: &str,
) -> Command {
    let wrapper = root.join("installer-architecture-wrapper.ps1");
    fs::write(
        &wrapper,
        "$env:PROCESSOR_ARCHITECTURE = $args[0]\n\
$env:PROCESSOR_ARCHITEW6432 = $args[1]\n\
try { & $args[2] } catch { Write-Error $_; exit 1 }\n\
exit 0\n",
    )
    .expect("architecture wrapper should be writable");
    let mut command = installer_command(&wrapper);
    isolate_user_environment(&mut command, root, home, state_directory);
    command
        .arg(process_architecture)
        .arg(native_architecture)
        .arg(repository_root().join("install.ps1"))
        .current_dir(root)
        .env("NAN_INSTALL_BASE_URL", base_url)
        .env("NAN_INSTALL_DIR", install_directory)
        .env("NO_PROXY", "127.0.0.1,localhost")
}

pub(crate) fn isolated_command(
    executable: &Path,
    root: &Path,
    home: &Path,
    state_directory: &Path,
) -> Command {
    let mut command = Command::new(executable);
    isolate_user_environment(&mut command, root, home, state_directory);
    command
}

#[cfg(unix)]
pub(crate) fn isolated_alias_command(
    install_directory: &Path,
    root: &Path,
    home: &Path,
    state_directory: &Path,
) -> Command {
    isolated_command(&alias_path(install_directory), root, home, state_directory)
}

#[cfg(windows)]
pub(crate) fn isolated_alias_command(
    install_directory: &Path,
    root: &Path,
    home: &Path,
    state_directory: &Path,
) -> Command {
    let mut command = Command::new("cmd.exe");
    command
        .args(["/D", "/C"])
        .arg(alias_path(install_directory));
    isolate_user_environment(&mut command, root, home, state_directory);
    command
}

fn isolate_user_environment(
    command: &mut Command,
    root: &Path,
    home: &Path,
    state_directory: &Path,
) {
    let temporary_directory = root.join("tmp");
    let app_data = home.join("AppData/Roaming");
    let local_app_data = home.join("AppData/Local");
    let xdg_config = home.join(".config");
    let xdg_data = home.join(".local/share");
    let xdg_cache = home.join(".cache");
    for directory in [
        &temporary_directory,
        &app_data,
        &local_app_data,
        &xdg_config,
        &xdg_data,
        &xdg_cache,
    ] {
        fs::create_dir_all(directory).expect("isolated user directory should exist");
    }
    command
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("HERMES_HOME", home.join(".hermes"))
        .env("APPDATA", app_data)
        .env("LOCALAPPDATA", local_app_data)
        .env("XDG_CONFIG_HOME", xdg_config)
        .env("XDG_DATA_HOME", xdg_data)
        .env("XDG_CACHE_HOME", xdg_cache)
        .env("NAN_HARNESS_CONFIG_DIR", state_directory)
        .env("TMPDIR", &temporary_directory)
        .env("TMP", &temporary_directory)
        .env("TEMP", temporary_directory);
}

#[cfg(unix)]
pub(crate) fn assert_curl_option(arguments: &str, option: &str, value: &str) {
    let arguments = arguments.lines().collect::<Vec<_>>();
    assert!(
        arguments.windows(2).any(|pair| pair == [option, value]),
        "curl arguments should contain {option} {value}: {arguments:?}"
    );
}

pub(crate) fn assert_installation_receipt(
    state_directory: &Path,
    binary: &Path,
    install_directory: &Path,
) {
    let receipt: serde_json::Value = serde_json::from_slice(
        &fs::read(state_directory.join("installation.json"))
            .expect("installation receipt should exist"),
    )
    .expect("installation receipt should be valid JSON");
    assert_eq!(receipt["schemaVersion"], 1);
    assert_eq!(receipt["executablePath"], binary.to_string_lossy().as_ref());
    assert_eq!(
        receipt["aliasPath"],
        alias_path(install_directory).to_string_lossy().as_ref()
    );
    assert!(receipt["userPathEntryAdded"].is_boolean());
}

pub(crate) fn wait_until_removed(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while path.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
}

pub(crate) fn path_exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
