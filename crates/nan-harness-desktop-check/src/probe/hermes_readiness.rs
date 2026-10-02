//! Explicit Windows cold-start readiness; the worker's 240-second watch remains authoritative.

use crate::report::Reason;
use nan_harness_private_fs::open_private_read;
use serde::Deserialize;
use std::{
    io::Read as _,
    path::Path,
    time::{Duration, Instant},
};

pub(crate) fn requested() -> Result<bool, Reason> {
    let Some(policy) = std::env::var_os("FEASIBILITY_HERMES_READINESS_POLICY") else {
        return Ok(false);
    };
    if policy != "current-catalog"
        || !cfg!(windows)
        || std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
        || std::env::var("RUNNER_OS").as_deref() != Ok("Windows")
        || std::env::var("FEASIBILITY_HERMES_CATALOG_PROFILE").as_deref() != Ok("nan")
    {
        return Err(Reason::IsolationUnavailable);
    }
    Ok(true)
}

pub(crate) fn enabled() -> bool {
    requested() == Ok(true)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    profile: String,
}

pub(crate) fn prepare(
    workspace: &Path,
    deadline: Instant,
    mut guard: impl FnMut() -> Result<(), Reason>,
) -> Result<(), Reason> {
    if !requested()? {
        return Ok(());
    }
    // The launcher creates its profile asynchronously. Wait only for missing
    // files, retaining the same readiness watch and process-ownership guard.
    let files = [
        "profile/nanh/hermes-desktop/ownership.json",
        "profile/hermes/profiles/nan/.nan-harness-owner.json",
        "profile/home/AppData/Roaming/Hermes/active-profile.json",
    ];
    loop {
        guard()?;
        let mut missing = false;
        for relative in files {
            match std::fs::symlink_metadata(workspace.join(relative)) {
                Ok(metadata) if metadata.is_file() => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => missing = true,
                Ok(_) | Err(_) => return Err(Reason::IsolationUnavailable),
            }
        }
        if !missing {
            break;
        }
        if Instant::now() >= deadline {
            return Err(Reason::Timeout);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    super::hermes_policy::verify_owner(workspace)?;
    // nANH begin_session writes this exact fresh selection; Electron reads it
    // before pinning its primary backend and passing --profile to `serve`.
    let relative = "profile/home/AppData/Roaming/Hermes";
    let mut directory = workspace.to_path_buf();
    for component in relative.split('/') {
        directory.push(component);
        if !std::fs::symlink_metadata(&directory).is_ok_and(|metadata| metadata.is_dir()) {
            return Err(Reason::IsolationUnavailable);
        }
    }
    let (file, _) = open_private_read(&directory.join("active-profile.json"))
        .map_err(|_| Reason::IsolationUnavailable)?;
    let mut bytes = Vec::new();
    file.take(4097)
        .read_to_end(&mut bytes)
        .map_err(|_| Reason::IsolationUnavailable)?;
    valid_selection(&bytes)
}

fn valid_selection(bytes: &[u8]) -> Result<(), Reason> {
    if bytes.len() > 4096 {
        return Err(Reason::IsolationUnavailable);
    }
    let selection: Selection =
        serde_json::from_slice(bytes).map_err(|_| Reason::IsolationUnavailable)?;
    if selection.profile != "nan" {
        return Err(Reason::IsolationUnavailable);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_exact_owned_launch_selection_is_accepted() {
        assert_eq!(valid_selection(br#"{"profile":"nan"}"#), Ok(()));
        for value in [
            br#"{"profile":"default"}"#.as_slice(),
            br#"{"profile":"nan","defaultRoute":{"profile":"foreign"}}"#.as_slice(),
            br#"{"profile":"nan","profile":"foreign"}"#.as_slice(),
            b"private malformed payload".as_slice(),
        ] {
            assert_eq!(valid_selection(value), Err(Reason::IsolationUnavailable));
        }
        assert_eq!(
            valid_selection(&vec![b' '; 4097]),
            Err(Reason::IsolationUnavailable)
        );
    }
}
