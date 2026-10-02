//! Capture only closed startup facts from disposable hosted GUI children.

use std::io::Write as _;
use std::process::{Command, Stdio};
use zeroize::Zeroizing;

pub(super) fn spawn(command: &mut Command, app: &'static str) -> std::io::Result<()> {
    if !super::qualification_capture_enabled() {
        return command.stderr(Stdio::null()).spawn().map(|_| ());
    }
    let directory = std::path::PathBuf::from(
        std::env::var_os("NANH_DESKTOP_QUALIFICATION_FACTS").unwrap_or_default(),
    );
    let mut child = command.stderr(Stdio::piped()).spawn()?;
    let pid = child.id();
    let Some(mut stderr) = child.stderr.take() else {
        return Ok(());
    };
    std::thread::spawn(move || {
        let (bytes, overflow) = capture(&mut stderr);
        let Ok(status) = child.wait() else {
            return;
        };
        let hint = if overflow {
            "unclassified"
        } else {
            category(&bytes)
        };
        let value = serde_json::json!({"schemaVersion":1, "mechanism":"renderer-startup", "diagnosticsOnly":true,
            "app":app, "exitCode":status.code(), "stderrPresent":!bytes.is_empty(), "captureTruncated":overflow,
            "startupCategory":hint});
        let path = directory.join(format!("renderer-startup-{pid}.json"));
        if let Ok(bytes) = serde_json::to_vec(&value)
            && let Ok(mut file) = nan_harness_private_fs::open_private_new(&path)
        {
            let _ = file.write_all(&bytes).and_then(|()| file.sync_all());
        }
    });
    Ok(())
}

fn capture(reader: &mut impl std::io::Read) -> (Zeroizing<Vec<u8>>, bool) {
    let mut bytes = Zeroizing::new(Vec::new());
    let mut buffer = Zeroizing::new([0_u8; 4096]);
    let mut overflow = false;
    loop {
        match reader.read(&mut buffer[..]) {
            Ok(0) => break,
            Ok(size) => {
                let retained = size.min(65536 - bytes.len());
                bytes.extend_from_slice(&buffer[..retained]);
                overflow |= retained != size;
            }
            Err(_) => {
                overflow = true;
                break;
            }
        }
    }
    (bytes, overflow)
}

fn category(bytes: &[u8]) -> &'static str {
    let text = String::from_utf8_lossy(bytes);
    if text.contains("No usable sandbox") {
        "no-usable-sandbox"
    } else if text.contains("error while loading shared libraries") {
        "missing-shared-library"
    } else if text.contains("Missing X server") || text.contains("Missing X server or $DISPLAY") {
        "display-unavailable"
    } else {
        "unclassified"
    }
}

#[cfg(test)]
mod tests {
    use super::{capture, category};
    #[test]
    fn capture_drains_child_output_and_retains_a_fixed_memory_budget() {
        let mut source = std::io::Cursor::new(vec![b'x'; 100_000]);
        let (bytes, overflow) = capture(&mut source);
        assert!(overflow);
        assert_eq!(bytes.len(), 65536);
        assert_eq!(source.position(), 100_000);
    }
    #[test]
    fn startup_categories_never_return_child_output() {
        assert_eq!(category(b"PRIVATE No usable sandbox!"), "no-usable-sandbox");
        assert_eq!(
            category(b"error while loading shared libraries: PRIVATE"),
            "missing-shared-library"
        );
        assert_eq!(
            category(b"Missing X server or $DISPLAY PRIVATE"),
            "display-unavailable"
        );
        assert_eq!(
            category(b"PRIVATE credentials and arbitrary error"),
            "unclassified"
        );
    }
}
