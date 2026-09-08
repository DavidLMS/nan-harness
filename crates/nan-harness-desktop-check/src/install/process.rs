use super::{InstallError, MAX_EXPANDED_BYTES};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

pub(super) async fn run(command: &mut tokio::process::Command) -> Result<(), InstallError> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let status = tokio::time::timeout(Duration::from_mins(2), command.status())
        .await
        .map_err(|_| InstallError::Extraction)?
        .map_err(|_| InstallError::Extraction)?;
    if status.success() {
        Ok(())
    } else {
        Err(InstallError::Extraction)
    }
}

pub(super) async fn decompress(
    program: &str,
    input: &Path,
    output: &Path,
) -> Result<(), InstallError> {
    let mut child = tokio::process::Command::new(program)
        .args(["--decompress", "--stdout"])
        .arg(input)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| InstallError::Extraction)?;
    let mut stdout = child.stdout.take().ok_or(InstallError::Extraction)?;
    let mut file = nan_harness_private_fs::open_private_new(output)?;
    let result = tokio::time::timeout(Duration::from_mins(2), async {
        use std::io::Write as _;
        use tokio::io::AsyncReadExt as _;
        let mut total = 0u64;
        let mut buffer = vec![0u8; 64 * 1024];
        loop {
            let count = stdout.read(&mut buffer).await?;
            if count == 0 {
                break;
            }
            total += count as u64;
            if total > MAX_EXPANDED_BYTES {
                return Err(InstallError::TooLarge);
            }
            file.write_all(&buffer[..count])?;
        }
        if !child.wait().await?.success() {
            return Err(InstallError::Extraction);
        }
        Ok(())
    })
    .await
    .map_err(|_| InstallError::Extraction)?;
    if result.is_err() {
        let _ = child.kill().await;
    }
    result
}
