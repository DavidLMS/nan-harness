//! Readiness-anchored single action transport; never renews the caller deadline.
use super::FailureCategory;
use crate::native::{Window, windows_chat_turn};
use std::{
    io::{Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use zeroize::Zeroizing;
struct ChildOwner {
    child: Child,
    reader: Option<JoinHandle<()>>,
    writer: Option<JoinHandle<std::io::Result<()>>>,
}
impl Drop for ChildOwner {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}
fn frame(input: &mut impl Read) -> Result<Zeroizing<String>, FailureCategory> {
    let mut bytes = Zeroizing::new(Vec::new());
    for _ in 0..128 {
        let mut byte = [0];
        input
            .read_exact(&mut byte)
            .map_err(|_| FailureCategory::Output)?;
        bytes.push(byte[0]);
        if byte[0] == b'\n' {
            return String::from_utf8(bytes.to_vec())
                .map(Zeroizing::new)
                .map_err(|_| FailureCategory::Output);
        }
    }
    Err(FailureCategory::Output)
}
pub(in crate::native) fn run(
    executable: &Path,
    window: &Window,
    mode: &str,
    values: [&str; 3],
    deadline: Instant,
) -> Result<Zeroizing<String>, FailureCategory> {
    if Instant::now() >= deadline {
        return Err(FailureCategory::Timeout);
    }
    let mut command = Command::new(executable);
    command
        .env_clear()
        .arg("--windows-claude-chat-turn")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        command.creation_flags(0x0800_0000);
    }
    for name in ["SystemRoot", "WINDIR"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    let child = command.spawn().map_err(|_| FailureCategory::Spawn)?;
    let mut owner = ChildOwner {
        child,
        reader: None,
        writer: None,
    };
    let mut input = owner.child.stdin.take().ok_or(FailureCategory::Pipe)?;
    let mut output = owner.child.stdout.take().ok_or(FailureCategory::Pipe)?;
    let (sender, receiver) = mpsc::sync_channel(2);
    owner.reader = Some(thread::spawn(move || {
        let ready = frame(&mut output);
        let failed = ready.is_err();
        if sender.send(ready).is_err() || failed {
            return;
        }
        let result = frame(&mut output).and_then(|frame| {
            let mut trailing = [0];
            match output.read(&mut trailing) {
                Ok(0) => Ok(frame),
                _ => Err(FailureCategory::Output),
            }
        });
        let _ = sender.send(result);
    }));
    let receive = || {
        receiver
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|_| {
                if Instant::now() >= deadline {
                    FailureCategory::Timeout
                } else {
                    FailureCategory::Output
                }
            })?
    };
    let anchor = windows_chat_turn::ready(&receive()?).ok_or(FailureCategory::Output)?;
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining <= Duration::from_millis(50) {
        return Err(FailureCategory::Timeout);
    }
    let wire =
        windows_chat_turn::request(window, mode, values, anchor, remaining, std::process::id())
            .ok_or(FailureCategory::InvalidInput)?;
    owner.writer = Some(thread::spawn(move || {
        input.write_all(wire.as_bytes())?;
        input.write_all(b"\n")
    }));
    let result = receive()?;
    if windows_chat_turn::WindowsChatStage::parse(&result).is_none() {
        return Err(FailureCategory::Output);
    }
    loop {
        if Instant::now() >= deadline {
            return Err(FailureCategory::Timeout);
        }
        match owner.child.try_wait().map_err(|_| FailureCategory::Pipe)? {
            Some(status) if status.success() => break,
            Some(_) => return Err(FailureCategory::NonzeroExit),
            None => thread::sleep(
                Duration::from_millis(20).min(deadline.saturating_duration_since(Instant::now())),
            ),
        }
    }
    owner
        .writer
        .take()
        .ok_or(FailureCategory::Pipe)?
        .join()
        .map_err(|_| FailureCategory::Pipe)?
        .map_err(|_| FailureCategory::Pipe)?;
    Ok(result)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn readiness_transport_rejects_trailing_data_and_expires_without_replay() {
        use std::os::unix::fs::PermissionsExt as _;
        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join("helper");
        let window = Window {
            id: 1,
            pid: 2,
            bounds: xa11y::Rect {
                x: 10,
                y: 20,
                width: 300,
                height: 400,
            },
            name: String::new(),
            layer: 0,
        };
        let scripted = |suffix: &str, budget| {
            std::fs::write(&executable,format!("#!/usr/bin/env python3\nimport sys\nprint('ready 1000',flush=True)\nwire=sys.stdin.buffer.read()\nassert wire.count(b'\\n')==1\nassert len(wire)<8192\nassert len(wire.split())==13\nassert int(wire.split()[7])<=16000\n{suffix}\n")).unwrap();
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
            run(
                &executable,
                &window,
                "input-replace-owned",
                ["prompt", "", "sentinel"],
                Instant::now() + budget,
            )
        };
        assert_eq!(
            scripted("print('turn sent',flush=True)", Duration::from_secs(2))
                .unwrap()
                .as_str(),
            "turn sent\n"
        );
        assert_eq!(
            scripted(
                "print('turn sent\\nPRIVATE',flush=True)",
                Duration::from_secs(2)
            )
            .unwrap_err(),
            FailureCategory::Output
        );
        assert_eq!(
            scripted(
                "import time; time.sleep(0.2); print('turn sent',flush=True)",
                Duration::from_millis(80)
            )
            .unwrap_err(),
            FailureCategory::Timeout
        );
    }
}
