use crate::terminal::TerminalCommand;
#[cfg(windows)]
use std::io;
use std::path::Path;

/// Adds only the Windows process prerequisites needed by a cleared conformance environment.
///
/// The OS variables are copied case-insensitively because Windows environment names are
/// case-insensitive while Rust's environment iterator preserves the spelling supplied by the
/// parent. User-scoped locations always point inside the disposable conformance workspace.
#[cfg(windows)]
pub(super) fn apply(command: TerminalCommand, workspace: &Path) -> io::Result<TerminalCommand> {
    let home = workspace.join("home");
    let appdata = home.join("AppData/Roaming");
    let local_appdata = home.join("AppData/Local");
    let temp = workspace.join("tmp");
    for directory in [&home, &appdata, &local_appdata, &temp] {
        std::fs::create_dir_all(directory)?;
    }

    let mut command = command
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("APPDATA", &appdata)
        .env("LOCALAPPDATA", &local_appdata)
        .env("TEMP", &temp)
        .env("TMP", &temp);
    for (canonical, aliases) in [
        ("SystemRoot", &["SystemRoot", "SYSTEMROOT"][..]),
        ("WINDIR", &["WINDIR", "windir"][..]),
        ("ComSpec", &["ComSpec", "COMSPEC"][..]),
        ("PATHEXT", &["PATHEXT", "pathext"][..]),
    ] {
        if let Some(value) = aliases.iter().find_map(|name| {
            std::env::vars_os().find_map(|(key, value)| {
                key.to_string_lossy()
                    .eq_ignore_ascii_case(name)
                    .then_some(value)
            })
        }) {
            command = command.env(canonical, value);
        }
    }
    // A harness whose tools need a POSIX shell on Windows is told where the shell the
    // cell verified lives, instead of rediscovering Git for Windows behind a cleared
    // environment. The names are the closed set a cell may configure: this crate's own
    // variable plus the spellings the two published Kimi CLIs read.
    for name in [
        "NAN_HARNESS_GIT_BASH",
        "KIMI_SHELL_PATH",
        "KIMI_CLI_GIT_BASH_PATH",
    ] {
        if let Some(value) = std::env::var_os(name) {
            command = command.env(name, value);
        }
    }
    Ok(command)
}

#[cfg(not(windows))]
pub(super) fn apply(command: TerminalCommand, _workspace: &Path) -> TerminalCommand {
    command
}

#[cfg(all(test, windows))]
mod tests {
    use super::apply;
    use crate::terminal::TerminalCommand;
    use crate::workspace::ConformanceWorkspace;
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    #[tokio::test]
    async fn native_child_sees_os_prerequisites_and_private_paths_only() {
        const SENTINEL: &str = "NAN_TEST_PARENT_SENTINEL";
        let child_mode = std::env::var_os(SENTINEL).is_some();
        if !child_mode {
            let status = Command::new(std::env::current_exe().expect("test executable"))
                .args([
                    "--exact",
                    concat!(
                        module_path!(),
                        "::native_child_sees_os_prerequisites_and_private_paths_only"
                    ),
                ])
                .env(SENTINEL, "synthetic-parent-secret")
                .status()
                .expect("child test process should launch");
            assert!(status.success(), "child test exit status: {status}");
            return;
        }
        let workspace = ConformanceWorkspace::create().expect("workspace should exist");
        let home = workspace.path().join("home");
        let appdata = home.join("AppData/Roaming");
        let local_appdata = home.join("AppData/Local");
        let temp = workspace.path().join("tmp");
        let script = workspace.path().join("environment-fixture.cmd");
        let quote = |path: &Path| path.display().to_string().replace('"', "\"\"");
        fs::write(
            &script,
            format!(
                "@echo off\r\nif \"%SystemRoot%\"==\"\" exit /b 11\r\nif \"%WINDIR%\"==\"\" exit /b 12\r\nif \"%ComSpec%\"==\"\" exit /b 13\r\nif \"%PATHEXT%\"==\"\" exit /b 14\r\nif /I not \"%USERPROFILE%\"==\"{home}\" exit /b 21\r\nif /I not \"%APPDATA%\"==\"{appdata}\" exit /b 22\r\nif /I not \"%LOCALAPPDATA%\"==\"{local_appdata}\" exit /b 23\r\nif /I not \"%TEMP%\"==\"{temp}\" exit /b 24\r\nif /I not \"%TMP%\"==\"{temp}\" exit /b 25\r\nif defined NAN_TEST_PARENT_SENTINEL exit /b 31\r\nexit /b 0\r\n",
                home = quote(&home),
                appdata = quote(&appdata),
                local_appdata = quote(&local_appdata),
                temp = quote(&temp),
            ),
        )
        .expect("fixture should be written");
        let comspec = std::env::var_os("ComSpec")
            .or_else(|| std::env::var_os("COMSPEC"))
            .expect("Windows should provide ComSpec");
        let command = apply(
            TerminalCommand::new(comspec, workspace.path())
                .clear_environment()
                .args(["/d", "/c", "call", script.to_string_lossy().as_ref()]),
            workspace.path(),
        );
        #[cfg(windows)]
        let command = command.expect("private environment should be prepared");
        let command = command.timeout(std::time::Duration::from_secs(5));
        let output = command.run().await.expect("native fixture should launch");
        assert!(
            output.status.success(),
            "fixture exit status: {:?}",
            output.status
        );
    }
}
