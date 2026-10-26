//! Hosted-only Windows launcher; all application output remains discarded.
use std::{
    env,
    ffi::OsString,
    fs::OpenOptions,
    io::{self, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(Clone, Copy)]
enum App {
    Hermes,
    Codex,
    Claude,
    Pen,
}
impl App {
    fn from_argument(value: &std::ffi::OsStr) -> Option<Self> {
        match value.to_str()? {
            "hermes-desktop" => Some(Self::Hermes),
            "chatgpt-desktop" => Some(Self::Codex),
            "claude-desktop" => Some(Self::Claude),
            "pen-desktop" => Some(Self::Pen),
            _ => None,
        }
    }
    const fn name(self) -> &'static str {
        match self {
            Self::Hermes => "hermes-desktop",
            Self::Codex => "chatgpt-desktop",
            Self::Claude => "claude-desktop",
            Self::Pen => "pen-desktop",
        }
    }
}

fn observed_app(args: &[OsString]) -> Option<App> {
    if !args.iter().any(|arg| arg == "--provider-base-url") {
        return None;
    }
    args.first().and_then(|arg| App::from_argument(arg))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    RealBinary,
    HostPolicy,
    PortBind,
    PortAddress,
    ChildSpawn,
    FactsRoot,
    ConnectionCreate,
    ConnectionWrite,
    ConnectionSync,
    ChildWait,
    ChildExit,
}
impl Stage {
    const fn name(self) -> &'static str {
        match self {
            Self::RealBinary => "real-binary",
            Self::HostPolicy => "host-policy",
            Self::PortBind => "port-bind",
            Self::PortAddress => "port-address",
            Self::ChildSpawn => "child-spawn",
            Self::FactsRoot => "facts-root",
            Self::ConnectionCreate => "connection-create",
            Self::ConnectionWrite => "connection-write",
            Self::ConnectionSync => "connection-sync",
            Self::ChildWait => "child-wait",
            Self::ChildExit => "child-exit",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cause {
    NotFound,
    PermissionDenied,
    AlreadyExists,
    InvalidInput,
    Interrupted,
    TimedOut,
    AddressInUse,
    AddressNotAvailable,
    Other,
    ChildNonzero,
}
impl Cause {
    fn from_io(kind: io::ErrorKind) -> Self {
        match kind {
            io::ErrorKind::NotFound => Self::NotFound,
            io::ErrorKind::PermissionDenied => Self::PermissionDenied,
            io::ErrorKind::AlreadyExists => Self::AlreadyExists,
            io::ErrorKind::InvalidInput => Self::InvalidInput,
            io::ErrorKind::Interrupted => Self::Interrupted,
            io::ErrorKind::TimedOut => Self::TimedOut,
            io::ErrorKind::AddrInUse => Self::AddressInUse,
            io::ErrorKind::AddrNotAvailable => Self::AddressNotAvailable,
            _ => Self::Other,
        }
    }
    const fn name(self) -> &'static str {
        match self {
            Self::NotFound => "not-found",
            Self::PermissionDenied => "permission-denied",
            Self::AlreadyExists => "already-exists",
            Self::InvalidInput => "invalid-input",
            Self::Interrupted => "interrupted",
            Self::TimedOut => "timed-out",
            Self::AddressInUse => "address-in-use",
            Self::AddressNotAvailable => "address-not-available",
            Self::Other => "other",
            Self::ChildNonzero => "child-nonzero",
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Failure {
    stage: Stage,
    cause: Cause,
    child_started: bool,
    child_exit_code: Option<i32>,
    os_code: Option<i32>,
}
impl Failure {
    const fn condition(stage: Stage, cause: Cause, child_started: bool) -> Self {
        Self {
            stage,
            cause,
            child_started,
            child_exit_code: None,
            os_code: None,
        }
    }
    fn io(stage: Stage, error: &io::Error, child_started: bool) -> Self {
        Self {
            cause: Cause::from_io(error.kind()),
            os_code: error.raw_os_error(),
            ..Self::condition(stage, Cause::Other, child_started)
        }
    }
    const fn child_exit(code: i32) -> Self {
        Self {
            child_exit_code: Some(code),
            ..Self::condition(Stage::ChildExit, Cause::ChildNonzero, true)
        }
    }
    fn payload(self, app: App) -> String {
        let number = |value: Option<i32>| value.map_or_else(|| "null".into(), |v| v.to_string());
        // All strings come from closed enums; no argument, error or path is serialized.
        format!(
            "{{\"schemaVersion\":1,\"mechanism\":\"windows-launcher\",\"diagnosticsOnly\":true,\"app\":\"{}\",\"stage\":\"{}\",\"cause\":\"{}\",\"childStarted\":{},\"childExitCode\":{},\"osCode\":{}}}\n",
            app.name(),
            self.stage.name(),
            self.cause.name(),
            self.child_started,
            number(self.child_exit_code),
            number(self.os_code)
        )
    }
}

fn diagnostic_directory(directory: &Path) -> bool {
    if !directory.is_absolute() {
        return false;
    }
    let Ok(metadata) = directory.symlink_metadata() else {
        return false;
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return false;
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return false;
        }
    }
    // The checker restricts the original containing directory before launch.
    true
}

fn write_diagnostic(directory: &Path, app: App, failure: Failure, nonce: u128) -> io::Result<()> {
    if !diagnostic_directory(directory) {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let path = directory.join(format!(
        "windows-launcher-{}-{nonce}.json",
        std::process::id()
    ));
    write_new_diagnostic(&path, &failure.payload(app))
}

fn write_new_diagnostic(path: &Path, payload: &str) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(payload.as_bytes())?;
    file.sync_all()
}

fn emit_failure(app: App, failure: Failure) {
    let Some(directory) = env::var_os("FEASIBILITY_FACTS") else {
        return;
    };
    let Ok(nonce) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) else {
        return;
    };
    // Failure to record observations never changes the original result.
    let _ = write_diagnostic(Path::new(&directory), app, failure, nonce.as_nanos());
}

fn write_connection(directory: &Path, port: u16, child: u32) -> Result<(), Failure> {
    if !directory.is_absolute() || !directory.is_dir() {
        return Err(Failure::condition(
            Stage::FactsRoot,
            Cause::InvalidInput,
            true,
        ));
    }
    let path = directory.join(format!("connection-{}.json", std::process::id()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|error| Failure::io(Stage::ConnectionCreate, &error, true))?;
    // The containing directory has already been restricted by the checker.
    writeln!(
        file,
        "{{\"schemaVersion\":1,\"port\":{port},\"launcherPid\":{child}}}"
    )
    .map_err(|error| Failure::io(Stage::ConnectionWrite, &error, true))?;
    file.sync_all()
        .map_err(|error| Failure::io(Stage::ConnectionSync, &error, true))
}

fn command_arguments(mut args: Vec<OsString>, port: u16) -> Vec<OsString> {
    if !args.iter().any(|arg| arg == "--") {
        args.push("--".into());
    }
    args.push(format!("--remote-debugging-port={port}").into());
    args.push("--remote-debugging-address=127.0.0.1".into());
    args
}

fn run() -> Result<i32, Failure> {
    let real = PathBuf::from(
        env::var_os("FEASIBILITY_REAL_NANH")
            .ok_or_else(|| Failure::condition(Stage::RealBinary, Cause::NotFound, false))?,
    );
    if !real.is_absolute() || !real.is_file() {
        return Err(Failure::condition(
            Stage::RealBinary,
            Cause::InvalidInput,
            false,
        ));
    }
    let args: Vec<_> = env::args_os().skip(1).collect();
    let observe = observed_app(&args).is_some();
    let hermes = args.first().is_some_and(|arg| arg == "hermes-desktop");
    let baseline =
        !hermes && env::var("NANH_DESKTOP_QUALIFICATION_MODE").as_deref() == Ok("startup-baseline");
    if observe
        && (env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
            || env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted"))
    {
        return Err(Failure::condition(
            Stage::HostPolicy,
            Cause::PermissionDenied,
            false,
        ));
    }
    let port = if observe && !baseline {
        Some(
            TcpListener::bind(("127.0.0.1", 0))
                .map_err(|error| Failure::io(Stage::PortBind, &error, false))?
                .local_addr()
                .map_err(|error| Failure::io(Stage::PortAddress, &error, false))?
                .port(),
        )
    } else {
        None
    };
    let mut command = Command::new(real);
    command.args(if hermes {
        port.map_or_else(
            || args.clone(),
            |port| command_arguments(args.clone(), port),
        )
    } else {
        args
    });
    if !hermes && let Some(port) = port {
        command.env("NANH_DESKTOP_QUALIFICATION_CDP_PORT", port.to_string());
    }
    let mut child = command
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
        .map_err(|error| Failure::io(Stage::ChildSpawn, &error, false))?;
    let record = if let Some(port) = port {
        (|| {
            let directory = PathBuf::from(
                env::var_os("FEASIBILITY_FACTS")
                    .ok_or_else(|| Failure::condition(Stage::FactsRoot, Cause::NotFound, true))?,
            );
            write_connection(&directory, port, child.id())
        })()
    } else {
        Ok(())
    };
    if let Err(failure) = record {
        let _ = child.kill();
        let _ = child.wait();
        return Err(failure);
    }
    child
        .wait()
        .map(|status| status.code().unwrap_or(1))
        .map_err(|error| Failure::io(Stage::ChildWait, &error, true))
}

fn main() {
    let app = observed_app(&env::args_os().skip(1).collect::<Vec<_>>());
    let (code, failure) = match run() {
        Ok(0) => (0, None),
        Ok(code) => (code, Some(Failure::child_exit(code))),
        Err(failure) => (1, Some(failure)),
    };
    if let (Some(app), Some(failure)) = (app, failure) {
        emit_failure(app, failure);
    }
    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;
    struct OwnedDirectory(PathBuf);
    impl OwnedDirectory {
        fn new() -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = env::temp_dir().join(format!(
                "nanh-shim-fixture-{}-{nonce}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                std::fs::DirBuilder::new()
                    .mode(0o700)
                    .create(&path)
                    .unwrap();
            }
            #[cfg(not(unix))]
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for OwnedDirectory {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn connection_collision_retains_the_original_and_projects_its_actual_io_cause() {
        let directory = OwnedDirectory::new();
        let path = directory
            .0
            .join(format!("connection-{}.json", std::process::id()));
        std::fs::write(&path, b"PRIVATE_EXISTING_CONNECTION").unwrap();
        let failure = write_connection(&directory.0, 43210, 17).unwrap_err();
        assert_eq!(failure.stage, Stage::ConnectionCreate);
        assert_eq!(failure.cause, Cause::AlreadyExists);
        assert!(failure.child_started);
        assert_eq!(failure.child_exit_code, None);
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"PRIVATE_EXISTING_CONNECTION"
        );
        let payload = failure.payload(App::Codex);
        assert!(payload.contains("\"stage\":\"connection-create\""));
        assert!(payload.contains("\"cause\":\"already-exists\""));
        assert!(!payload.contains("PRIVATE"));
        assert!(!payload.contains(&directory.0.to_string_lossy().to_string()));
    }

    #[test]
    fn closed_failure_payload_distinguishes_wrapper_failure_from_child_nonzero() {
        let error = io::Error::new(io::ErrorKind::PermissionDenied, "PRIVATE_ERROR_AND_PATH");
        let failure = Failure::io(Stage::ChildSpawn, &error, false);
        assert_eq!(
            failure.payload(App::Codex),
            "{\"schemaVersion\":1,\"mechanism\":\"windows-launcher\",\"diagnosticsOnly\":true,\"app\":\"chatgpt-desktop\",\"stage\":\"child-spawn\",\"cause\":\"permission-denied\",\"childStarted\":false,\"childExitCode\":null,\"osCode\":null}\n"
        );
        assert_eq!(
            Failure::child_exit(1).payload(App::Claude),
            "{\"schemaVersion\":1,\"mechanism\":\"windows-launcher\",\"diagnosticsOnly\":true,\"app\":\"claude-desktop\",\"stage\":\"child-exit\",\"cause\":\"child-nonzero\",\"childStarted\":true,\"childExitCode\":1,\"osCode\":null}\n"
        );
        if let Some(path) = env::var_os("NANH_SHIM_TEST_RECORD") {
            write_new_diagnostic(Path::new(&path), &failure.payload(App::Codex)).unwrap();
        }
        let native = io::Error::from_raw_os_error(32);
        assert_eq!(
            Failure::io(Stage::ChildSpawn, &native, false).os_code,
            Some(32)
        );
        assert!(!failure.payload(App::Pen).contains("PRIVATE"));
        assert!(App::from_argument(std::ffi::OsStr::new("PRIVATE_APP")).is_none());
        assert!(observed_app(&["chatgpt-desktop".into(), "--restore".into()]).is_none());
        assert!(observed_app(&["PRIVATE_APP".into(), "--provider-base-url".into()]).is_none());
        for kind in [io::ErrorKind::Unsupported, io::ErrorKind::UnexpectedEof] {
            assert_eq!(Cause::from_io(kind), Cause::Other);
        }
    }

    #[test]
    fn diagnostic_write_is_private_exclusive_and_has_no_invalid_root_fallback() {
        let directory = OwnedDirectory::new();
        let failure = Failure::condition(Stage::FactsRoot, Cause::InvalidInput, true);
        write_diagnostic(&directory.0, App::Hermes, failure, 7).unwrap();
        let path = std::fs::read_dir(&directory.0)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let original = std::fs::read(&path).unwrap();
        assert_eq!(
            write_new_diagnostic(&path, &failure.payload(App::Codex))
                .unwrap_err()
                .kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(std::fs::read(&path).unwrap(), original);
        // Distinct clock values are explicit here; the fixture never races the
        // platform clock's resolution to prove that one reused PID can record both.
        write_diagnostic(&directory.0, App::Codex, failure, 8).unwrap();
        assert_eq!(std::fs::read_dir(&directory.0).unwrap().count(), 2);
        for root in [PathBuf::from("relative"), directory.0.join("absent"), path] {
            assert!(write_diagnostic(&root, App::Codex, failure, 9).is_err());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{PermissionsExt, symlink};
            assert_eq!(
                std::fs::metadata(&directory.0)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
            for entry in std::fs::read_dir(&directory.0).unwrap() {
                assert_eq!(
                    entry.unwrap().metadata().unwrap().permissions().mode() & 0o777,
                    0o600
                );
            }
            let link = directory.0.join("link");
            symlink(&directory.0, &link).unwrap();
            assert!(write_diagnostic(&link, App::Codex, failure, 9).is_err());
        }
    }

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
