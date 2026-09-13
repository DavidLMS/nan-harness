use super::{DiscoveryError, Installation};
use crate::catalog::frozen::inspect::pe;
use nan_harness_core::DesktopHarnessKind;
use semver::Version;
use std::fs;
use std::io::{ErrorKind, Read as _, Seek as _, SeekFrom};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub(super) mod asar;
pub(super) mod diagnostic;

pub(super) fn measure(
    kind: DesktopHarnessKind,
    executable: &Path,
) -> Result<Installation, DiscoveryError> {
    let bundle = executable
        .ancestors()
        .find(|path| path.extension().is_some_and(|extension| extension == "app"));
    let mut app_version = None;
    if cfg!(target_os = "macos")
        && let Some(bundle) = bundle
    {
        let mut command = Command::new("/usr/bin/plutil");
        command
            .args(["-extract", "CFBundleShortVersionString", "raw", "-o", "-"])
            .arg(bundle.join("Contents/Info.plist"));
        app_version = parse_version(&command_output_for(
            &mut command,
            kind,
            diagnostic::Source::AppVersionCommand,
        )?);
    }
    let parent = executable.parent().ok_or(DiscoveryError::Incomplete)?;
    let resources = bundle.map_or_else(
        || parent.join("resources"),
        |root| root.join("Contents/Resources"),
    );
    if app_version.is_none() {
        app_version = package_version(&resources.join("app/package.json"), kind)?;
    }
    if app_version.is_none() {
        app_version = asar::version(&resources.join("app.asar")).inspect_err(|_| {
            diagnostic::emit(
                kind,
                diagnostic::Source::AsarMetadata,
                diagnostic::Failure::MetadataUnreadable,
                None,
                None,
            );
        })?;
    }
    // The Windows CLI has its own 0.1.0 executable resource version. Its
    // documented --version output identifies the installed Zed application.
    if app_version.is_none() && kind == DesktopHarnessKind::Zed {
        app_version = parse_version(&command_output_for(
            Command::new(executable).arg("--version"),
            kind,
            diagnostic::Source::AppVersionCommand,
        )?);
    }
    if app_version.is_none() && cfg!(windows) && kind != DesktopHarnessKind::Zed {
        // AppX paths can use the Win32 extended-length prefix, which older
        // PowerShell FileVersionInfo handling does not consistently accept.
        // Read the bounded PE resource first; retain the shell fallback for
        // binaries whose product version is not stored in RT_VERSION.
        app_version = pe::product_version(executable).and_then(|text| parse_version(&text));
    }
    if app_version.is_none() && cfg!(windows) && kind != DesktopHarnessKind::Zed {
        let mut command = Command::new("powershell.exe");
        // Rust canonical paths use the Win32 extended-length prefix. Read file
        // version data directly, without PowerShell's filesystem-provider path rules.
        command.args(["-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference = 'Stop'; [System.Diagnostics.FileVersionInfo]::GetVersionInfo($env:NAN_CHECK_VERSION_PATH).ProductVersion"])
            .env("NAN_CHECK_VERSION_PATH", executable);
        app_version = parse_version(&command_output_for(
            &mut command,
            kind,
            diagnostic::Source::AppVersionCommand,
        )?);
    }
    // Electron executables are never invoked for inventory: --version may open a GUI.
    let runtime_version = if kind == DesktopHarnessKind::ChatGpt {
        let runtime = resources.join(if cfg!(windows) { "codex.exe" } else { "codex" });
        match fs::metadata(&runtime) {
            Ok(metadata) if metadata.is_file() => parse_version(&command_output_for(
                Command::new(runtime).arg("--version"),
                kind,
                diagnostic::Source::RuntimeVersionCommand,
            )?),
            Ok(_) => {
                diagnostic::emit(
                    kind,
                    diagnostic::Source::RuntimeMetadata,
                    diagnostic::Failure::InvalidMetadata,
                    None,
                    None,
                );
                return Err(DiscoveryError::Incomplete);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                diagnostic::emit(
                    kind,
                    diagnostic::Source::RuntimeMetadata,
                    diagnostic::Failure::Read,
                    None,
                    Some(&error),
                );
                return Err(DiscoveryError::VersionResource);
            }
        }
    } else {
        None
    };
    Ok(Installation {
        executable: executable.to_path_buf(),
        app_version,
        runtime_version,
    })
}

fn package_version(
    path: &Path,
    app: DesktopHarnessKind,
) -> Result<Option<Version>, DiscoveryError> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            diagnostic::emit(
                app,
                diagnostic::Source::PackageMetadata,
                diagnostic::Failure::Read,
                None,
                Some(&error),
            );
            return Err(DiscoveryError::VersionResource);
        }
    };
    let mut bytes = Vec::new();
    file.take(65_537).read_to_end(&mut bytes).map_err(|error| {
        diagnostic::emit(
            app,
            diagnostic::Source::PackageMetadata,
            diagnostic::Failure::Read,
            None,
            Some(&error),
        );
        DiscoveryError::VersionResource
    })?;
    if bytes.len() > 65_536 {
        diagnostic::emit(
            app,
            diagnostic::Source::PackageMetadata,
            diagnostic::Failure::Oversize,
            None,
            None,
        );
        return Err(DiscoveryError::VersionResource);
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| {
        diagnostic::emit(
            app,
            diagnostic::Source::PackageMetadata,
            diagnostic::Failure::InvalidMetadata,
            None,
            None,
        );
        DiscoveryError::VersionResource
    })?;
    Ok(value
        .get("version")
        .and_then(serde_json::Value::as_str)
        .and_then(parse_version))
}

pub(super) fn parse_version(text: &str) -> Option<Version> {
    text.split_whitespace()
        .find_map(|part| Version::parse(part.trim_start_matches('v')).ok())
}

pub(super) fn command_output(command: &mut Command) -> Result<String, DiscoveryError> {
    command_output_within(command, Duration::from_secs(5))
}

/// Run the bounded Windows package inventory command and retain its closed
/// failure fact before mapping it to the discovery boundary.
pub(super) fn invalid_windows_package_metadata(app: DesktopHarnessKind) {
    diagnostic::emit(
        app,
        diagnostic::Source::WindowsPackageEnumeration,
        diagnostic::Failure::InvalidMetadata,
        None,
        None,
    );
}

pub(super) fn windows_package_output(
    command: &mut Command,
    app: DesktopHarnessKind,
) -> Result<String, DiscoveryError> {
    command_output_detailed(command, Duration::from_secs(5)).map_err(|error| {
        diagnostic::emit_command(app, diagnostic::Source::WindowsPackageEnumeration, &error);
        DiscoveryError::RootEnumeration
    })
}

fn command_output_for(
    command: &mut Command,
    app: DesktopHarnessKind,
    source: diagnostic::Source,
) -> Result<String, DiscoveryError> {
    command_output_detailed(command, Duration::from_secs(5)).map_err(|error| {
        let runtime_read_access = (cfg!(windows)
            && app == DesktopHarnessKind::ChatGpt
            && source == diagnostic::Source::RuntimeVersionCommand
            && error.failure == diagnostic::Failure::Spawn
            && error.os_error == Some(5))
        .then(|| runtime_read_access(Path::new(command.get_program())));
        let strict_runtime_context = cfg!(windows)
            && app == DesktopHarnessKind::ChatGpt
            && source == diagnostic::Source::RuntimeVersionCommand
            && error.failure == diagnostic::Failure::Spawn
            && error.os_error == Some(5);
        let runtime_path = Path::new(command.get_program());
        diagnostic::emit_command_with_runtime_observations(
            app,
            source,
            &error,
            runtime_read_access,
            strict_runtime_context.then(|| runtime_file(runtime_path)),
            strict_runtime_context.then(|| runtime_image_header(runtime_path)),
        );
        DiscoveryError::VersionResource
    })
}

fn runtime_read_access(path: &Path) -> diagnostic::RuntimeReadAccess {
    match fs::File::open(path) {
        Ok(_) => diagnostic::RuntimeReadAccess::Readable,
        Err(error) if error.raw_os_error() == Some(5) => {
            diagnostic::RuntimeReadAccess::AccessDenied
        }
        Err(_) => diagnostic::RuntimeReadAccess::OtherError,
    }
}

fn runtime_file(path: &Path) -> diagnostic::RuntimeFile {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return diagnostic::RuntimeFile::Missing;
        }
        Err(_) => return diagnostic::RuntimeFile::QueryUnknown,
    };
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        if metadata.file_attributes() & 0x400 != 0 {
            return diagnostic::RuntimeFile::SymlinkOrReparse;
        }
    }
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        diagnostic::RuntimeFile::SymlinkOrReparse
    } else if file_type.is_file() {
        diagnostic::RuntimeFile::RegularFile
    } else if file_type.is_dir() {
        diagnostic::RuntimeFile::Directory
    } else {
        diagnostic::RuntimeFile::Other
    }
}

const MAX_RUNTIME_PE_OFFSET: u32 = 1024 * 1024;
const PE_SIGNATURE: [u8; 4] = *b"PE\0\0";
const X64_MACHINE: u16 = 0x8664;
const ARM64_MACHINE: u16 = 0xaa64;

fn runtime_image_header(path: &Path) -> diagnostic::RuntimeImageHeader {
    if runtime_file(path) != diagnostic::RuntimeFile::RegularFile {
        return diagnostic::RuntimeImageHeader::QueryUnknown;
    }
    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return diagnostic::RuntimeImageHeader::QueryUnknown,
    };
    let mut dos = [0; 64];
    match file.read_exact(&mut dos) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::UnexpectedEof => {
            return diagnostic::RuntimeImageHeader::InvalidHeader;
        }
        Err(_) => return diagnostic::RuntimeImageHeader::QueryUnknown,
    }
    if dos[..2] != *b"MZ" {
        return diagnostic::RuntimeImageHeader::InvalidHeader;
    }
    let offset = u32::from_le_bytes(dos[60..64].try_into().expect("fixed DOS header"));
    if offset > MAX_RUNTIME_PE_OFFSET {
        return diagnostic::RuntimeImageHeader::InvalidHeader;
    }
    if file.seek(SeekFrom::Start(u64::from(offset))).is_err() {
        return diagnostic::RuntimeImageHeader::QueryUnknown;
    }
    let mut pe = [0; 6];
    match file.read_exact(&mut pe) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::UnexpectedEof => {
            return diagnostic::RuntimeImageHeader::InvalidHeader;
        }
        Err(_) => return diagnostic::RuntimeImageHeader::QueryUnknown,
    }
    if pe[..4] != PE_SIGNATURE {
        return diagnostic::RuntimeImageHeader::InvalidHeader;
    }
    classify_runtime_machine(u16::from_le_bytes([pe[4], pe[5]]), host_runtime_machine())
}

fn host_runtime_machine() -> Option<u16> {
    #[cfg(all(windows, target_arch = "x86_64"))]
    {
        return Some(X64_MACHINE);
    }
    #[cfg(all(windows, target_arch = "aarch64"))]
    {
        return Some(ARM64_MACHINE);
    }
    None
}

fn classify_runtime_machine(
    machine: u16,
    host_machine: Option<u16>,
) -> diagnostic::RuntimeImageHeader {
    if machine != X64_MACHINE && machine != ARM64_MACHINE {
        return diagnostic::RuntimeImageHeader::UnsupportedMachine;
    }
    match host_machine {
        Some(host) if host == machine => diagnostic::RuntimeImageHeader::MachineMatchesHost,
        Some(_) => diagnostic::RuntimeImageHeader::MachineDiffers,
        None => diagnostic::RuntimeImageHeader::QueryUnknown,
    }
}

fn parse_runtime_image_fixture(
    bytes: &[u8],
    host_machine: Option<u16>,
) -> diagnostic::RuntimeImageHeader {
    if bytes.len() < 64 || bytes[..2] != *b"MZ" {
        return diagnostic::RuntimeImageHeader::InvalidHeader;
    }
    let offset = u32::from_le_bytes(bytes[60..64].try_into().expect("fixed DOS header"));
    if offset > MAX_RUNTIME_PE_OFFSET {
        return diagnostic::RuntimeImageHeader::InvalidHeader;
    }
    let end = offset as usize + 6;
    if end > bytes.len() || bytes[offset as usize..end - 2] != PE_SIGNATURE {
        return diagnostic::RuntimeImageHeader::InvalidHeader;
    }
    classify_runtime_machine(
        u16::from_le_bytes([bytes[end - 2], bytes[end - 1]]),
        host_machine,
    )
}

pub(super) fn command_output_within(
    command: &mut Command,
    limit: Duration,
) -> Result<String, DiscoveryError> {
    command_output_detailed(command, limit).map_err(|_| DiscoveryError::VersionResource)
}

fn command_output_detailed(
    command: &mut Command,
    limit: Duration,
) -> Result<String, diagnostic::CommandFailure> {
    use diagnostic::{CommandFailure, Failure};
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env_remove("NAN_API_KEY")
        .spawn()
        .map_err(|error| CommandFailure::io(Failure::Spawn, &error))?;
    let deadline = Instant::now() + limit;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            other => {
                let failure = match other {
                    Err(error) => CommandFailure::io(Failure::Wait, &error),
                    Ok(_) => CommandFailure::new(Failure::Timeout),
                };
                let _ = child.kill();
                let _ = child.wait();
                return Err(failure);
            }
        }
    };
    if !status.success() {
        return Err(CommandFailure {
            exit_code: status.code(),
            ..CommandFailure::new(Failure::NonzeroExit)
        });
    }
    let mut bytes = Vec::new();
    child
        .stdout
        .take()
        .ok_or_else(|| CommandFailure::new(Failure::Pipe))?
        .take(65_537)
        .read_to_end(&mut bytes)
        .map_err(|error| CommandFailure::io(Failure::Read, &error))?;
    if bytes.len() > 65_536 {
        return Err(CommandFailure::new(Failure::Oversize));
    }
    String::from_utf8(bytes).map_err(|_| CommandFailure::new(Failure::Encoding))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_exact_not_guessed_from_four_component_numbers() {
        assert_eq!(
            parse_version("codex-cli 0.151.0-alpha.7.2"),
            Version::parse("0.151.0-alpha.7.2").ok()
        );
        assert_eq!(parse_version("1.2.3.4"), None);
        assert_eq!(parse_version("unknown"), None);
    }

    #[test]
    fn runtime_read_access_is_bounded_and_does_not_read_content() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("codex.exe");
        fs::write(&path, b"synthetic executable").unwrap();
        assert_eq!(
            runtime_read_access(&path),
            diagnostic::RuntimeReadAccess::Readable
        );
        assert_eq!(
            runtime_read_access(&root.path().join("missing.exe")),
            diagnostic::RuntimeReadAccess::OtherError
        );
    }

    fn pe_fixture(offset: u32, machine: u16) -> Vec<u8> {
        let mut bytes = vec![0; offset as usize + 6];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[60..64].copy_from_slice(&offset.to_le_bytes());
        bytes[offset as usize..offset as usize + 4].copy_from_slice(&PE_SIGNATURE);
        bytes[offset as usize + 4..offset as usize + 6].copy_from_slice(&machine.to_le_bytes());
        bytes
    }

    #[test]
    fn runtime_file_facts_do_not_infer_from_openability() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("runtime.exe");
        let directory = root.path().join("runtime-dir");
        fs::write(&file, b"not an image").unwrap();
        fs::create_dir(&directory).unwrap();
        assert_eq!(runtime_file(&file), diagnostic::RuntimeFile::RegularFile);
        assert_eq!(runtime_file(&directory), diagnostic::RuntimeFile::Directory);
        assert_eq!(
            runtime_file(&root.path().join("missing.exe")),
            diagnostic::RuntimeFile::Missing
        );
        assert_eq!(
            runtime_image_header(&file),
            diagnostic::RuntimeImageHeader::InvalidHeader
        );
        assert_eq!(
            runtime_image_header(&directory),
            diagnostic::RuntimeImageHeader::QueryUnknown
        );
    }

    #[test]
    fn runtime_image_fixture_classifies_bounded_header_states() {
        assert_eq!(
            parse_runtime_image_fixture(&pe_fixture(64, X64_MACHINE), Some(X64_MACHINE)),
            diagnostic::RuntimeImageHeader::MachineMatchesHost
        );
        assert_eq!(
            parse_runtime_image_fixture(&pe_fixture(64, X64_MACHINE), Some(ARM64_MACHINE)),
            diagnostic::RuntimeImageHeader::MachineDiffers
        );
        assert_eq!(
            parse_runtime_image_fixture(&pe_fixture(64, 0x1234), Some(X64_MACHINE)),
            diagnostic::RuntimeImageHeader::UnsupportedMachine
        );
        assert_eq!(
            parse_runtime_image_fixture(&pe_fixture(64, X64_MACHINE), None),
            diagnostic::RuntimeImageHeader::QueryUnknown
        );
        assert_eq!(
            parse_runtime_image_fixture(b"MZ", Some(X64_MACHINE)),
            diagnostic::RuntimeImageHeader::InvalidHeader
        );
        let mut malformed = pe_fixture(64, X64_MACHINE);
        malformed[64..68].copy_from_slice(b"NOPE");
        assert_eq!(
            parse_runtime_image_fixture(&malformed, Some(X64_MACHINE)),
            diagnostic::RuntimeImageHeader::InvalidHeader
        );
        assert_eq!(
            parse_runtime_image_fixture(
                &pe_fixture(MAX_RUNTIME_PE_OFFSET + 1, X64_MACHINE),
                Some(X64_MACHINE)
            ),
            diagnostic::RuntimeImageHeader::InvalidHeader
        );
        let mut truncated = pe_fixture(64, X64_MACHINE);
        truncated.truncate(68);
        assert_eq!(
            parse_runtime_image_fixture(&truncated, Some(X64_MACHINE)),
            diagnostic::RuntimeImageHeader::InvalidHeader
        );
    }

    #[test]
    #[cfg(windows)]
    fn windows_inventory_accepts_a_canonical_executable_without_version_resources() {
        let executable = fs::canonicalize(std::env::current_exe().unwrap()).unwrap();
        let installation = measure(DesktopHarnessKind::Pen, &executable).unwrap();
        assert_eq!(installation.executable, executable);
    }

    #[test]
    fn absent_package_metadata_is_not_an_inventory_failure() {
        let root = tempfile::tempdir().expect("fixture");
        assert_eq!(
            package_version(
                &root.path().join("missing.json"),
                DesktopHarnessKind::ChatGpt
            ),
            Ok(None)
        );
    }

    #[cfg(unix)]
    #[test]
    fn synthetic_commands_cover_nonzero_missing_timeout_and_encoding() {
        use super::diagnostic::{CommandFailure, Failure};
        let limit = Duration::from_secs(2);
        let exit =
            command_output_detailed(Command::new("sh").args(["-c", "exit 7"]), limit).unwrap_err();
        assert_eq!(
            exit,
            CommandFailure {
                failure: Failure::NonzeroExit,
                exit_code: Some(7),
                os_error: None
            }
        );
        let missing =
            command_output_detailed(&mut Command::new("/definitely/missing"), limit).unwrap_err();
        assert_eq!(missing.failure, Failure::Spawn);
        assert!(missing.os_error.is_some());
        assert_eq!(missing.exit_code, None);
        assert_eq!(
            command_output_detailed(Command::new("sleep").arg("1"), Duration::from_millis(10)),
            Err(CommandFailure::new(Failure::Timeout))
        );
        assert_eq!(
            command_output_detailed(Command::new("sh").args(["-c", "printf '\\377'"]), limit),
            Err(CommandFailure::new(Failure::Encoding))
        );
    }

    #[test]
    fn malformed_and_oversized_package_metadata_is_rejected() {
        let root = tempfile::tempdir().expect("fixture");
        let path = root.path().join("package.json");
        fs::write(&path, b"not-json").expect("fixture");
        assert!(package_version(&path, DesktopHarnessKind::ChatGpt).is_err());
        fs::write(&path, vec![b'x'; 65_537]).expect("fixture");
        assert!(package_version(&path, DesktopHarnessKind::ChatGpt).is_err());
    }
}
