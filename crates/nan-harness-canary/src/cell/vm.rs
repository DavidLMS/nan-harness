use super::{remote::run_host_command, spec::GuestNetwork, workspace::CellWorkspace};
use std::fs;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::{Child, Command};

pub(crate) struct VmLease {
    pub(crate) name: String,
    pub(crate) created: bool,
    run_process: Option<Child>,
    cleaned: bool,
}

impl VmLease {
    pub(crate) fn new(name: String) -> Self {
        Self {
            name,
            created: false,
            run_process: None,
            cleaned: false,
        }
    }

    pub(crate) async fn start(
        &mut self,
        workspace: &CellWorkspace,
        network: GuestNetwork,
        timeout: Duration,
    ) -> Result<(), String> {
        let input = format!("nan-input:{}:ro", workspace.input.display());
        let output = format!("nan-output:{}", workspace.output.display());
        let mut command = Command::new("tart");
        command.args(["run", "--no-graphics"]);
        if matches!(network, GuestNetwork::Softnet) {
            command.arg("--net-softnet");
        }
        let stdout = fs::File::create(workspace.log_path("start-vm", 1))
            .map_err(|error| format!("could not create the Tart private log: {error}"))?;
        let stderr = stdout
            .try_clone()
            .map_err(|error| format!("could not clone the Tart private log: {error}"))?;
        command
            .args([
                &format!("--dir={input}"),
                &format!("--dir={output}"),
                self.name.as_str(),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .kill_on_drop(true);
        let child = command
            .spawn()
            .map_err(|error| format!("could not start tart: {error}"))?;
        self.run_process = Some(child);
        tokio::time::sleep(timeout.min(Duration::from_millis(500))).await;
        if self
            .run_process
            .as_mut()
            .expect("run process should exist")
            .try_wait()
            .map_err(|error| format!("could not inspect tart: {error}"))?
            .is_some()
        {
            return Err("tart exited before the VM became reachable".to_owned());
        }
        Ok(())
    }

    pub(crate) async fn cleanup(&mut self) -> Result<(), String> {
        if self.cleaned {
            return Ok(());
        }
        let mut failures = Vec::new();
        if self.created
            && let Err(error) = run_host_command(
                "tart",
                &["stop", self.name.as_str()],
                Duration::from_secs(30),
            )
            .await
        {
            failures.push(error.detail);
        }
        if let Some(mut child) = self.run_process.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        if self.created
            && let Err(error) = run_host_command(
                "tart",
                &["delete", self.name.as_str()],
                Duration::from_secs(30),
            )
            .await
        {
            failures.push(error.detail);
        }
        if failures.is_empty() {
            self.cleaned = true;
            Ok(())
        } else {
            Err(failures.join("; "))
        }
    }
}

impl Drop for VmLease {
    fn drop(&mut self) {
        if self.cleaned || !self.created {
            return;
        }
        let _ = std::process::Command::new("tart")
            .args(["stop", self.name.as_str()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = std::process::Command::new("tart")
            .args(["delete", self.name.as_str()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}
