use super::{InstallError, MAX_ENTRIES, MAX_EXPANDED_BYTES, archive, process};
use nan_harness_core::DesktopHarnessKind;
use nan_harness_private_fs::create_private_dir;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub(super) async fn extract(
    input: &Path,
    destination: &Path,
    kind: DesktopHarnessKind,
) -> Result<PathBuf, InstallError> {
    if !cfg!(target_os = "macos") {
        return Err(InstallError::Extraction);
    }
    let mount = destination.join("mounted");
    create_private_dir(&mount)?;
    let mut guard = MountGuard(Some(mount.clone()));
    process::run(
        tokio::process::Command::new("/usr/bin/hdiutil")
            .args([
                "attach",
                "-readonly",
                "-nobrowse",
                "-noautoopen",
                "-mountpoint",
            ])
            .arg(&mount)
            .arg(input),
    )
    .await
    .map_err(|_| InstallError::MountPending)?;
    let result = copy_application(&mount, destination, kind);
    let detached = process::run(
        tokio::process::Command::new("/usr/bin/hdiutil")
            .arg("detach")
            .arg(&mount),
    )
    .await;
    // A failed detach is not hidden behind an earlier copy error: the caller must
    // preserve the journal rather than recursively clean up a mounted filesystem.
    detached.map_err(|_| InstallError::MountPending)?;
    guard.0 = None;
    fs::remove_dir(&mount)?;
    result
}

fn copy_application(
    mount: &Path,
    destination: &Path,
    kind: DesktopHarnessKind,
) -> Result<PathBuf, InstallError> {
    let name = match kind {
        DesktopHarnessKind::ChatGpt => "ChatGPT",
        DesktopHarnessKind::Claude => "Claude",
        DesktopHarnessKind::Hermes => "Hermes",
        DesktopHarnessKind::Pen => "Pen",
        DesktopHarnessKind::Zed => "Zed",
    };
    let source = mount.join(format!("{name}.app"));
    if !fs::symlink_metadata(&source)?.is_dir() {
        return Err(InstallError::Archive);
    }
    let target = destination.join(format!("{name}.app"));
    create_private_dir(&target)?;
    copy_tree(&source, &target)?;
    Ok(target)
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), InstallError> {
    let mut pending = vec![PathBuf::new()];
    let mut links = Vec::new();
    let mut count = 0usize;
    let mut total = 0u64;
    while let Some(relative) = pending.pop() {
        count += 1;
        if count > MAX_ENTRIES {
            return Err(InstallError::TooLarge);
        }
        let path = source.join(&relative);
        let target = destination.join(&relative);
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            let link = fs::read_link(path)?;
            archive::validate_link(&relative, &link)?;
            links.push((target, link));
        } else if metadata.is_dir() {
            if !relative.as_os_str().is_empty() {
                create_private_dir(&target)?;
            }
            for entry in fs::read_dir(&path)? {
                pending.push(relative.join(entry?.file_name()));
            }
        } else if metadata.is_file() {
            total = total
                .checked_add(metadata.len())
                .ok_or(InstallError::TooLarge)?;
            if total > MAX_EXPANDED_BYTES {
                return Err(InstallError::TooLarge);
            }
            fs::copy(path, &target)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                archive::executable_mode(&target, metadata.permissions().mode())?;
            }
        } else {
            return Err(InstallError::Archive);
        }
    }
    archive::install_links(destination, links)
}

struct MountGuard(Option<PathBuf>);

impl Drop for MountGuard {
    fn drop(&mut self) {
        let Some(mount) = &self.0 else {
            return;
        };
        let Ok(mut child) = Command::new("/usr/bin/hdiutil")
            .env_remove("NAN_API_KEY")
            .arg("detach")
            .arg(mount)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            return;
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if matches!(child.try_wait(), Ok(Some(_))) {
                break;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// Extract a validated macOS application archive with the system `ditto`, then copy
/// only the expected bundle so links are checked exactly as for disk images.
pub(super) async fn extract_zip(
    input: &Path,
    root: &Path,
    destination: &Path,
    kind: DesktopHarnessKind,
) -> Result<PathBuf, InstallError> {
    if !cfg!(target_os = "macos") {
        return Err(InstallError::Extraction);
    }
    let entries = crate::catalog::frozen::zip_entries(input).map_err(|()| InstallError::Archive)?;
    if entries.len() > MAX_ENTRIES {
        return Err(InstallError::TooLarge);
    }
    let unpacked = root.join("unpacked");
    create_private_dir(&unpacked)?;
    process::run_within(
        tokio::process::Command::new("/usr/bin/ditto")
            .args(["-x", "-k"])
            .arg(input)
            .arg(&unpacked),
        Duration::from_mins(10),
    )
    .await?;
    copy_application(&unpacked, destination, kind)
}
