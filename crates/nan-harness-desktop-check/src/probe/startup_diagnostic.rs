//! Temporary, user-approved startup diagnostics for synthetic hosted Zed runs.

use super::{ProbeSpec, Reason};
use nan_harness_core::DesktopHarnessKind;
use std::{io::Write as _, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt as _},
    process::{Child, Command},
    task::JoinHandle,
};

pub(super) fn prepare(spec: &ProbeSpec, command: &mut Command) {
    if !spec.live
        && spec.kind == DesktopHarnessKind::Zed
        && !cfg!(target_os = "macos")
        && std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true")
        && std::env::var("NAN_DESKTOP_STARTUP_DIAGNOSTIC").as_deref()
            == Ok("approved-synthetic-zed")
        && std::env::var_os("NAN_API_KEY").is_none()
    {
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
    }
}

pub(super) fn start(process: &mut Child) -> Option<[JoinHandle<Vec<u8>>; 2]> {
    Some([drain(process.stdout.take()?), drain(process.stderr.take()?)])
}

fn drain(mut pipe: impl AsyncRead + Unpin + Send + 'static) -> JoinHandle<Vec<u8>> {
    tokio::spawn(async move {
        let mut retained = Vec::new();
        let mut buffer = [0; 4096];
        while let Ok(count) = pipe.read(&mut buffer).await {
            if count == 0 {
                break;
            }
            let count = count.min(65_536 - retained.len());
            retained.extend_from_slice(&buffer[..count]);
        }
        retained
    })
}

pub(super) async fn finish(
    diagnostic: Option<[JoinHandle<Vec<u8>>; 2]>,
    startup_failed: bool,
    token: &str,
    exit_code: Option<i32>,
) -> Result<(), Reason> {
    let Some(tasks) = diagnostic else {
        return Ok(());
    };
    let mut output = serde_json::Map::new();
    output.insert("exitCode".into(), serde_json::json!(exit_code));
    for (name, mut task) in ["stdout", "stderr"].into_iter().zip(tasks) {
        if startup_failed
            && let Ok(Ok(bytes)) = tokio::time::timeout(Duration::from_secs(2), &mut task).await
        {
            output.insert(
                name.into(),
                serde_json::Value::String(
                    String::from_utf8_lossy(&bytes).replace(token, "[synthetic-session-token]"),
                ),
            );
        }
        task.abort();
    }
    if !startup_failed {
        return Ok(());
    }
    let root = std::env::var_os("RUNNER_TEMP").ok_or(Reason::IsolationUnavailable)?;
    let directory = std::path::PathBuf::from(root).join("owned-zed-startup");
    nan_harness_private_fs::create_private_dir_all(&directory)
        .map_err(|_| Reason::IsolationUnavailable)?;
    let bytes = serde_json::to_vec(&output).map_err(|_| Reason::IsolationUnavailable)?;
    nan_harness_private_fs::open_private_new(
        &directory.join(format!("{}.json", std::process::id())),
    )
    .and_then(|mut file| file.write_all(&bytes))
    .map_err(|_| Reason::IsolationUnavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn excess_output_is_drained_without_retaining_more_than_the_budget() {
        let task = drain(std::io::Cursor::new(vec![b'x'; 200_000]));
        let retained = tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(retained, vec![b'x'; 65_536]);
    }
}
