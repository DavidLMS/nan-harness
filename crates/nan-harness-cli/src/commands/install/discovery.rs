use nan_harness_core::HarnessKind;
use nan_harness_runtime::is_executable_file;
use std::env;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

pub(crate) fn executable_from_known_locations(kind: HarnessKind) -> Option<PathBuf> {
    let home = env::var_os("HOME").or_else(|| env::var_os("USERPROFILE"))?;
    find_executable(kind, &PathBuf::from(home))
}

fn find_executable(kind: HarnessKind, home: &Path) -> Option<PathBuf> {
    executable_candidates(kind, home)
        .into_iter()
        .find(|executable| is_executable_file(executable))
}

fn executable_candidates(kind: HarnessKind, home: &Path) -> Vec<PathBuf> {
    let path_extensions = env::var_os("PATHEXT");
    let app_data = env::var_os("APPDATA").map(PathBuf::from);
    let local_app_data = env::var_os("LOCALAPPDATA").map(PathBuf::from);
    executable_candidates_for_platform(
        kind,
        home,
        cfg!(windows),
        path_extensions.as_deref(),
        app_data.as_deref(),
        local_app_data.as_deref(),
    )
}

fn executable_candidates_for_platform(
    kind: HarnessKind,
    home: &Path,
    windows: bool,
    path_extensions: Option<&OsStr>,
    app_data: Option<&Path>,
    local_app_data: Option<&Path>,
) -> Vec<PathBuf> {
    let mut directories = match kind {
        HarnessKind::ClaudeCode
        | HarnessKind::Hermes
        | HarnessKind::Aider
        | HarnessKind::Goose
        | HarnessKind::Fx
        | HarnessKind::Omp => vec![home.join(".local/bin")],
        HarnessKind::Codex => vec![home.join(".local/bin"), home.join(".codex/bin")],
        HarnessKind::OpenCode => vec![home.join(".opencode/bin"), home.join(".local/bin")],
        HarnessKind::Pi => vec![
            home.join(".local/bin"),
            home.join(".npm-global/bin"),
            home.join(".local/share/pi-node/current/bin"),
        ],
        HarnessKind::PrimeAgent
        | HarnessKind::DeepSeekHarness
        | HarnessKind::QwenCode
        | HarnessKind::Cline => {
            vec![home.join(".local/bin"), home.join(".npm-global/bin")]
        }
        HarnessKind::OpenClaw => vec![
            home.join(".local/bin"),
            home.join(".openclaw/bin"),
            home.join(".npm-global/bin"),
        ],
        HarnessKind::KimiCode => vec![home.join(".kimi-code/bin"), home.join(".local/bin")],
    };
    if windows
        && matches!(
            kind,
            HarnessKind::Codex
                | HarnessKind::OpenCode
                | HarnessKind::Pi
                | HarnessKind::DeepSeekHarness
                | HarnessKind::OpenClaw
                | HarnessKind::Cline
                | HarnessKind::QwenCode
        )
        && let Some(app_data) = app_data
    {
        directories.push(app_data.join("npm"));
    }
    if windows
        && kind == HarnessKind::Omp
        && let Some(local_app_data) = local_app_data
    {
        directories.push(local_app_data.join("omp"));
    }
    let executable_names = executable_names(kind.binary_name(), windows, path_extensions);
    directories
        .into_iter()
        .flat_map(|directory| {
            executable_names
                .iter()
                .map(move |name| directory.join(name))
        })
        .collect()
}

fn executable_names(
    binary_name: &str,
    windows: bool,
    path_extensions: Option<&OsStr>,
) -> Vec<OsString> {
    if !windows {
        return vec![OsString::from(binary_name)];
    }
    let extensions = path_extensions.unwrap_or_else(|| OsStr::new(".COM;.EXE;.BAT;.CMD"));
    extensions
        .to_string_lossy()
        .split(';')
        .filter(|extension| !extension.is_empty())
        .map(|extension| OsString::from(format!("{binary_name}{extension}")))
        .chain(std::iter::once(OsString::from(binary_name)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{executable_candidates, executable_candidates_for_platform, find_executable};
    use nan_harness_core::HarnessKind;
    use std::fs;

    #[test]
    fn finds_installed_executables_in_official_user_directories() {
        let directory = tempfile::tempdir().expect("temporary home should exist");
        let executable = executable_candidates(HarnessKind::OpenCode, directory.path())
            .into_iter()
            .next()
            .expect("OpenCode should have an executable candidate");
        fs::create_dir_all(executable.parent().expect("executable parent should exist"))
            .expect("OpenCode bin directory should be created");
        fs::write(&executable, "fake opencode executable")
            .expect("fake executable should be written");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))
                .expect("fake executable should be executable");
        }

        assert_eq!(
            find_executable(HarnessKind::OpenCode, directory.path()),
            Some(executable)
        );
    }

    #[cfg(unix)]
    #[test]
    fn ignores_non_executable_files_in_official_user_directories() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().expect("temporary home should exist");
        let executable = directory.path().join(".opencode/bin/opencode");
        fs::create_dir_all(executable.parent().expect("executable parent should exist"))
            .expect("OpenCode bin directory should be created");
        fs::write(&executable, "not executable").expect("fake executable should be written");
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o600))
            .expect("fake executable should not be executable");

        assert_eq!(
            find_executable(HarnessKind::OpenCode, directory.path()),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn candidates_include_paths_used_by_script_installers() {
        let home = std::path::Path::new("/tmp/nan-test-home");
        let candidates = executable_candidates(HarnessKind::KimiCode, home);
        assert!(candidates.contains(&home.join(".kimi-code/bin/kimi")));
        assert!(candidates.contains(&home.join(".local/bin/kimi")));
    }

    #[test]
    fn windows_candidates_include_npm_command_shims() {
        let home = std::path::Path::new("C:/Users/nan");
        let app_data = std::path::Path::new("C:/Users/nan/AppData/Roaming");
        let candidates = executable_candidates_for_platform(
            HarnessKind::Codex,
            home,
            true,
            Some(std::ffi::OsStr::new(".EXE;.CMD")),
            Some(app_data),
            None,
        );

        assert!(candidates.contains(&app_data.join("npm/codex.EXE")));
        assert!(candidates.contains(&app_data.join("npm/codex.CMD")));
    }

    #[test]
    fn windows_omp_candidates_include_the_official_binary_directory() {
        let home = std::path::Path::new("C:/Users/nan");
        let local_app_data = std::path::Path::new("C:/Users/nan/AppData/Local");
        let candidates = executable_candidates_for_platform(
            HarnessKind::Omp,
            home,
            true,
            Some(std::ffi::OsStr::new(".EXE;.CMD")),
            None,
            Some(local_app_data),
        );

        assert!(candidates.contains(&local_app_data.join("omp/omp.EXE")));
        assert!(candidates.contains(&local_app_data.join("omp/omp.CMD")));
    }

    #[test]
    fn deepseek_candidates_include_npm_directories() {
        let home = std::path::Path::new("/Users/nan");
        let candidates = executable_candidates_for_platform(
            HarnessKind::DeepSeekHarness,
            home,
            false,
            None,
            None,
            None,
        );

        assert!(candidates.contains(&home.join(".local/bin/dsh")));
        assert!(candidates.contains(&home.join(".npm-global/bin/dsh")));
    }
}
