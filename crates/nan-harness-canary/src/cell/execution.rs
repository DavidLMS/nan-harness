use super::{
    remote::{self, SSH_RETRY_DELAY},
    reporting::{RuntimeFailure, failed_check, passed_check},
    spec::{CellSpec, Step},
    vm::VmLease,
    workspace::CellWorkspace,
};
use crate::report::FailureClass;
use std::env;
use std::fmt::Write as _;
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

const MOUNT_ATTEMPTS: u8 = 3;
const PREPARED_IMAGE_ENVIRONMENT_VARIABLE: &str = "NAN_CANARY_PREPARED_IMAGE";

pub(crate) async fn execute_in_vm(
    spec: &CellSpec,
    workspace: &CellWorkspace,
    api_key: Option<&str>,
    deadline: Instant,
) -> Result<Vec<crate::report::CheckReport>, RuntimeFailure> {
    let mut checks = Vec::new();
    let vm_name = vm_name(&spec.id);
    let mut lease = VmLease::new(vm_name);

    clone_vm(spec, &mut lease, &mut checks, deadline).await?;
    let execution = async {
        let ip = start_vm(spec, workspace, &mut lease, &mut checks, deadline).await?;
        mount_workspace(spec, workspace, &ip, &mut checks, deadline).await?;
        run_steps(spec, workspace, api_key, &ip, &mut checks, deadline).await
    }
    .await;
    match execution {
        Ok(()) => {
            cleanup_vm(&mut lease, &mut checks).await?;
            Ok(checks)
        }
        Err(mut failure) => {
            let cleanup_started = Instant::now();
            match lease.cleanup().await {
                Ok(()) => {
                    failure
                        .checks
                        .push(passed_check("cleanup-vm", cleanup_started.elapsed(), 1));
                }
                Err(detail) => {
                    failure.checks.push(failed_check(
                        "cleanup-vm",
                        cleanup_started.elapsed(),
                        1,
                        &detail,
                    ));
                    failure.class = FailureClass::Infrastructure;
                    "vm-cleanup".clone_into(&mut failure.phase);
                    "the Tart VM could not be destroyed after a failed check"
                        .clone_into(&mut failure.summary);
                }
            }
            Err(failure)
        }
    }
}

async fn clone_vm(
    spec: &CellSpec,
    lease: &mut VmLease,
    checks: &mut Vec<crate::report::CheckReport>,
    deadline: Instant,
) -> Result<(), RuntimeFailure> {
    let clone_source = if let Some(prepared) = prepared_image_override() {
        async_available_local_image(prepared)
            .await
            .unwrap_or_else(|| spec.image.clone())
    } else {
        spec.image.clone()
    };
    match remote::run_host_command(
        "tart",
        &["clone", clone_source.as_str(), lease.name.as_str()],
        super::remaining(deadline, Duration::from_secs(spec.clone_timeout_seconds)),
    )
    .await
    {
        Ok(duration) => {
            lease.created = true;
            checks.push(passed_check("clone-vm", duration, 1));
        }
        Err(error) => {
            checks.push(failed_check("clone-vm", error.duration, 1, &error.detail));
            return Err(RuntimeFailure::new(
                FailureClass::Infrastructure,
                "vm-clone",
                "the Tart VM could not be cloned",
                checks.clone(),
            ));
        }
    }
    Ok(())
}

fn prepared_image_override() -> Option<String> {
    let prepared = env::var(PREPARED_IMAGE_ENVIRONMENT_VARIABLE).ok()?;
    valid_prepared_image_name(&prepared).then_some(prepared)
}

pub(crate) fn valid_prepared_image_name(prepared: &str) -> bool {
    !(prepared.len() > 128
        || !prepared.starts_with("nhc-suite-")
        || !prepared.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        }))
}

async fn async_available_local_image(prepared: String) -> Option<String> {
    let inventory = remote::command_text(
        "tart",
        &["list", "--source", "local", "--quiet"],
        Duration::from_secs(10),
    )
    .await
    .ok()?;
    inventory
        .lines()
        .any(|candidate| candidate == prepared)
        .then_some(prepared)
}

async fn start_vm(
    spec: &CellSpec,
    workspace: &CellWorkspace,
    lease: &mut VmLease,
    checks: &mut Vec<crate::report::CheckReport>,
    deadline: Instant,
) -> Result<String, RuntimeFailure> {
    let run_started = Instant::now();
    lease
        .start(
            workspace,
            spec.network,
            super::remaining(deadline, Duration::from_secs(spec.boot_timeout_seconds)),
        )
        .await
        .map_err(|detail| {
            checks.push(failed_check("start-vm", run_started.elapsed(), 1, &detail));
            RuntimeFailure::new(
                FailureClass::Infrastructure,
                "vm-start",
                "the Tart VM could not be started",
                checks.clone(),
            )
        })?;
    checks.push(passed_check("start-vm", run_started.elapsed(), 1));

    let boot_started = Instant::now();
    let ip = remote::wait_for_ssh(
        lease.name.as_str(),
        super::remaining(deadline, Duration::from_secs(spec.boot_timeout_seconds)),
    )
    .await
    .map_err(|detail| {
        checks.push(failed_check(
            "wait-for-ssh",
            boot_started.elapsed(),
            1,
            &detail,
        ));
        RuntimeFailure::new(
            FailureClass::Infrastructure,
            "vm-boot",
            "the Tart VM did not become reachable over SSH",
            checks.clone(),
        )
    })?;
    checks.push(passed_check("wait-for-ssh", boot_started.elapsed(), 1));
    Ok(ip)
}

async fn mount_workspace(
    spec: &CellSpec,
    workspace: &CellWorkspace,
    ip: &str,
    checks: &mut Vec<crate::report::CheckReport>,
    deadline: Instant,
) -> Result<(), RuntimeFailure> {
    let mount_started = Instant::now();
    let mut attempts = 0_u8;
    let mut last_failure = None;
    while attempts < MOUNT_ATTEMPTS {
        attempts += 1;
        match remote::run_remote_script(
            ip,
            spec.guest.mount_script(),
            &workspace.log_path("mount-workspace", attempts),
            super::remaining(
                deadline,
                Duration::from_secs(super::spec::DEFAULT_STEP_TIMEOUT_SECONDS),
            ),
        )
        .await
        {
            Ok(()) => {
                checks.push(passed_check(
                    "mount-workspace",
                    mount_started.elapsed(),
                    attempts,
                ));
                return Ok(());
            }
            Err(error) => {
                last_failure = Some(error);
                if attempts < MOUNT_ATTEMPTS {
                    tokio::time::sleep(SSH_RETRY_DELAY).await;
                }
            }
        }
    }
    let detail = last_failure.unwrap_or_else(|| "workspace mount failed".to_owned());
    checks.push(failed_check(
        "mount-workspace",
        mount_started.elapsed(),
        attempts,
        &detail,
    ));
    Err(RuntimeFailure::new(
        FailureClass::Infrastructure,
        "vm-mount",
        "the host workspace could not be mounted inside the VM",
        checks.clone(),
    ))
}

async fn run_steps(
    spec: &CellSpec,
    workspace: &CellWorkspace,
    api_key: Option<&str>,
    ip: &str,
    checks: &mut Vec<crate::report::CheckReport>,
    deadline: Instant,
) -> Result<(), RuntimeFailure> {
    for step in &spec.steps {
        let mut attempts = 0_u8;
        let mut last_failure = None;
        let step_started = Instant::now();
        while attempts < step.attempts {
            attempts += 1;
            let script = step_script(spec, step, api_key);
            let timeout = super::remaining(deadline, Duration::from_secs(step.timeout_seconds));
            match remote::run_remote_script(
                ip,
                &script,
                &workspace.log_path(&step.name, attempts),
                timeout,
            )
            .await
            {
                Ok(()) => {
                    checks.push(passed_check(&step.name, step_started.elapsed(), attempts));
                    last_failure = None;
                    break;
                }
                Err(error) => {
                    last_failure = Some(error);
                    if attempts < step.attempts {
                        tokio::time::sleep(SSH_RETRY_DELAY).await;
                    }
                }
            }
        }
        if let Some(detail) = last_failure {
            checks.push(failed_check(
                &step.name,
                step_started.elapsed(),
                attempts,
                &detail,
            ));
            return Err(RuntimeFailure::new(
                step.failure_class,
                step.name.clone(),
                format!("canary step '{}' failed", step.name),
                checks.clone(),
            ));
        }
    }
    Ok(())
}

async fn cleanup_vm(
    lease: &mut VmLease,
    checks: &mut Vec<crate::report::CheckReport>,
) -> Result<(), RuntimeFailure> {
    let cleanup_started = Instant::now();
    lease.cleanup().await.map_err(|detail| {
        checks.push(failed_check(
            "cleanup-vm",
            cleanup_started.elapsed(),
            1,
            &detail,
        ));
        RuntimeFailure::new(
            FailureClass::Infrastructure,
            "vm-cleanup",
            "the Tart VM could not be destroyed cleanly",
            checks.clone(),
        )
    })?;
    checks.push(passed_check("cleanup-vm", cleanup_started.elapsed(), 1));
    Ok(())
}

pub(crate) fn step_script(
    spec: &CellSpec,
    step: &Step,
    api_key: Option<&str>,
) -> Zeroizing<String> {
    let input = spec.guest.input_path();
    let output = spec.guest.output_path();
    let mut script = Zeroizing::new(format!(
        "set -euo pipefail\nexport NAN_CANARY_INPUT={}\nexport NAN_CANARY_OUTPUT={}\nexport NAN_CANARY_HARNESS={}\n",
        shell_quote(input),
        shell_quote(output),
        shell_quote(&spec.harness.to_string())
    ));
    if let Some(model) = &spec.model {
        writeln!(script, "export NAN_CANARY_MODEL={}", shell_quote(model))
            .expect("writing to a String cannot fail");
    }
    if step.requires_api_key {
        let api_key = api_key.expect("the API key is loaded before live steps");
        writeln!(script, "export NAN_API_KEY={}", shell_quote(api_key))
            .expect("writing to a String cannot fail");
    }
    script.push_str(
        &step
            .script
            .replace("{{input}}", input)
            .replace("{{output}}", output)
            .replace("{{harness}}", &spec.harness.to_string())
            .replace("{{model}}", spec.model.as_deref().unwrap_or_default()),
    );
    script.push('\n');
    script
}

pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn vm_name(cell_id: &str) -> String {
    let sanitized = cell_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    format!("nan-harness-canary-{sanitized}-{}", std::process::id())
}
