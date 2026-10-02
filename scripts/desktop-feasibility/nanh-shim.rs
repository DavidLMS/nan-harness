//! Hosted-only Windows launcher; all application output remains discarded.
use std::{
    env,
    ffi::OsString,
    fs::OpenOptions,
    io::Write,
    net::TcpListener,
    path::PathBuf,
    process::{Command, Stdio},
};

fn command_arguments(mut args: Vec<OsString>, port: u16) -> Vec<OsString> {
    if !args.iter().any(|arg| arg == "--") {
        args.push("--".into());
    }
    args.push(format!("--remote-debugging-port={port}").into());
    args.push("--remote-debugging-address=127.0.0.1".into());
    args
}

fn run() -> Result<i32, ()> {
    let real = PathBuf::from(env::var_os("FEASIBILITY_REAL_NANH").ok_or(())?);
    if !real.is_absolute() || !real.is_file() {
        return Err(());
    }
    let args: Vec<_> = env::args_os().skip(1).collect();
    let observe = args.first().is_some_and(|arg| arg == "hermes-desktop")
        && args.iter().any(|arg| arg == "--provider-base-url");
    let port = if observe {
        if env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
            || env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
        {
            return Err(());
        }
        Some(
            TcpListener::bind(("127.0.0.1", 0))
                .map_err(|_| ())?
                .local_addr()
                .map_err(|_| ())?
                .port(),
        )
    } else {
        None
    };
    let mut child = Command::new(real)
        .args(port.map_or_else(
            || args.clone(),
            |port| command_arguments(args.clone(), port),
        ))
        .stdin(Stdio::null())
        .stdout(if observe {
            Stdio::null()
        } else {
            Stdio::inherit()
        })
        .stderr(if observe {
            Stdio::null()
        } else {
            Stdio::inherit()
        })
        .spawn()
        .map_err(|_| ())?;
    let record = if let Some(port) = port {
        (|| {
            let directory = PathBuf::from(env::var_os("FEASIBILITY_FACTS").ok_or(())?);
            if !directory.is_absolute() || !directory.is_dir() {
                return Err(());
            }
            let path = directory.join(format!("connection-{}.json", std::process::id()));
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(path)
                .map_err(|_| ())?;
            // The containing directory has already been restricted by the checker.
            writeln!(
                file,
                "{{\"schemaVersion\":1,\"port\":{port},\"launcherPid\":{}}}",
                child.id()
            )
            .map_err(|_| ())?;
            file.sync_all().map_err(|_| ())
        })()
    } else {
        Ok(())
    };
    if record.is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return Err(());
    }
    child
        .wait()
        .map(|status| status.code().unwrap_or(1))
        .map_err(|_| ())
}

fn main() {
    std::process::exit(run().unwrap_or(1));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_debug_flags_preserve_literal_arguments_and_one_delimiter() {
        for args in [
            vec![
                "hermes-desktop",
                "--provider-base-url",
                "http://127.0.0.1:1234",
            ],
            vec!["hermes-desktop", "--", "C:\\private path\\app;literal"],
        ] {
            let original: Vec<OsString> = args.into_iter().map(Into::into).collect();
            let actual = command_arguments(original.clone(), 43210);
            assert!(actual.starts_with(&original));
            assert_eq!(actual.iter().filter(|arg| *arg == "--").count(), 1);
            assert_eq!(
                actual.last().unwrap(),
                "--remote-debugging-address=127.0.0.1"
            );
        }
    }
}
