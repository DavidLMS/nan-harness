use futures_util::FutureExt;
use std::fs::{File, OpenOptions};
use std::io;
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use tokio::process::Command;

pub(super) fn prepare(command: &mut Command) -> io::Result<(NamedPipeServer, NamedPipeServer)> {
    let (stdout, stdout_writer) = pipe()?;
    let (stderr, stderr_writer) = pipe()?;
    command.stdout(stdout_writer).stderr(stderr_writer);
    Ok((stdout, stderr))
}

fn pipe() -> io::Result<(NamedPipeServer, File)> {
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).map_err(io::Error::other)?;
    let name = format!(
        r"\\.\pipe\nan-harness-probe-{:032x}",
        u128::from_ne_bytes(random)
    );
    // Tokio's anonymous child pipes use blocking reader threads on Windows. Named overlapped
    // pipes keep cancellation independent of descendants holding writers, even if job cleanup
    // fails. Use a fresh unguessable, local-only, single-instance name and connect our writer
    // before spawning the child; no external process needs to discover the pipe name.
    let server = ServerOptions::new()
        .access_inbound(true)
        .access_outbound(false)
        .first_pipe_instance(true)
        .reject_remote_clients(true)
        .max_instances(1)
        .create(&name)?;
    let writer = OpenOptions::new().write(true).open(&name)?;
    // Our writer is already connected. Finish registration before it can exit in the child;
    // ConnectNamedPipe completes immediately with ERROR_PIPE_CONNECTED in this ordering.
    server
        .connect()
        .now_or_never()
        .ok_or_else(|| io::Error::other("probe pipe did not connect immediately"))??;
    Ok((server, writer))
}
