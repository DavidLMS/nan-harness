use super::error::InstallError;
use super::installer::run_command;
use nan_harness_core::HarnessKind;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

mod binding;

const REPOSITORY: &str = "https://github.com/zai-org/ZCode";
const REVISION: &str = "29628c9acdb81b703bbd4080c207a0e7ce5e276e";
const CLI: &str = "apps/zcode-cli/packages/cli/dist/zcode.cjs";
const RECEIPT: &str = ".nanh-source-revision";
const BINDING: &str = "nanh-zcode-config-v1";

pub(super) fn build_description() -> String {
    format!(
        "git clone {REPOSITORY}; git -C ZCode checkout --detach {REVISION}; pnpm --dir ZCode --filter '@zcode/cli...' install --ignore-scripts --frozen-lockfile; pnpm --dir ZCode --filter '@zcode/cli...' build"
    )
}

pub(super) fn check_prerequisites() -> Result<(), InstallError> {
    super::runtime::check_required_runtime(HarnessKind::ZCode)?;
    for program in ["git", "pnpm"] {
        let output = run_command(OsStr::new(program), &["--version"], Command::output)
            .map_err(|_| InstallError::SourcePrerequisite { program })?;
        if !output.status.success()
            || (program == "pnpm" && output.stdout.trim_ascii() != b"10.33.2")
        {
            return Err(InstallError::SourcePrerequisite { program });
        }
    }
    Ok(())
}

pub(super) fn install() -> Result<(), InstallError> {
    check_prerequisites()?;
    let home = env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| prepare(io::Error::other("an absolute user home is required")))?;
    install_into(&home)
}

fn install_into(home: &Path) -> Result<(), InstallError> {
    let root = home.join(".local/share/nan-harness/zcode");
    let bin = home.join(".local/bin");
    reject_symlink(&root).map_err(prepare)?;
    fs::create_dir_all(&root).map_err(prepare)?;
    fs::create_dir_all(&bin).map_err(prepare)?;
    let command = bin.join(if cfg!(windows) { "zcode.cmd" } else { "zcode" });
    let launcher = launcher(cfg!(windows));
    check_launcher(&command, &launcher).map_err(prepare)?;
    // The lock and revision receipt prevent a partial build from becoming an installation.
    let lock = root.join(".install-lock");
    reject_symlink(&lock).map_err(prepare)?;
    let lock_file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock)
        .map_err(prepare)?;
    lock_file
        .try_lock()
        .map_err(|error| prepare(io::Error::other(error)))?;
    let result = prepare_source(&root).and_then(|()| publish_launcher(&command, &launcher));
    drop(lock_file);
    result
}

fn prepare_source(root: &Path) -> Result<(), InstallError> {
    let installed = root.join(REVISION);
    reject_symlink(&installed).map_err(prepare)?;
    if installed.exists() {
        let revision = fs::read_to_string(installed.join(RECEIPT)).map_err(prepare)?;
        if revision != format!("{REVISION}\n{BINDING}") {
            return Err(prepare(io::Error::other(
                "source installation ownership changed",
            )));
        }
        return verify(&installed);
    }
    let temporary = tempfile::tempdir_in(root).map_err(prepare)?;
    let source = temporary.path().join("source");
    execute(
        "git",
        &["clone", "--no-checkout", "--filter=blob:none", REPOSITORY],
        temporary.path(),
        Some(&source),
    )?;
    execute("git", &["checkout", "--detach", REVISION], &source, None)?;
    execute(
        "pnpm",
        &[
            "--filter",
            "@zcode/cli...",
            "install",
            "--ignore-scripts",
            "--frozen-lockfile",
        ],
        &source,
        None,
    )?;
    let entries = [
        (
            "main.ts",
            binding::entrypoint as fn(&str) -> io::Result<String>,
        ),
        ("prompt-command.ts", binding::headless_entrypoint),
    ]
    .into_iter()
    .map(|(name, bind)| {
        let path = source.join("apps/zcode-cli/packages/cli/src").join(name);
        let original = fs::read_to_string(&path).map_err(prepare)?;
        let bound = bind(&original).map_err(prepare)?;
        Ok((path, original, bound))
    })
    .collect::<Result<Vec<_>, InstallError>>()?;
    for (path, _, bound) in &entries {
        fs::write(path, bound).map_err(prepare)?;
    }
    let build = execute(
        "pnpm",
        &["--filter", "@zcode/cli...", "build"],
        &source,
        None,
    );
    for (path, original, _) in entries {
        fs::write(path, original).map_err(prepare)?;
    }
    build?;
    verify(&source)?;
    fs::write(source.join(RECEIPT), format!("{REVISION}\n{BINDING}")).map_err(prepare)?;
    fs::rename(source, installed).map_err(prepare)
}

fn execute(
    program: &'static str,
    arguments: &[&str],
    directory: &Path,
    destination: Option<&Path>,
) -> Result<(), InstallError> {
    let status = run_command(OsStr::new(program), arguments, |command| {
        command
            .current_dir(directory)
            .stdin(Stdio::null())
            .env_remove("NAN_API_KEY")
            .env_remove("ZCODE_NAN_API_KEY");
        if let Some(destination) = destination {
            command.arg(destination);
        }
        command.status()
    })
    .map_err(|source| InstallError::CommandStart {
        harness: HarnessKind::ZCode,
        program,
        source,
    })?;
    if status.success() {
        Ok(())
    } else {
        Err(InstallError::CommandFailed {
            harness: HarnessKind::ZCode,
            program,
            exit_code: status.code(),
        })
    }
}

fn verify(source: &Path) -> Result<(), InstallError> {
    for (argument, expected) in [("version", "0.16.9"), ("--nanh-source-info", BINDING)] {
        let output = Command::new("node")
            .arg(source.join(CLI))
            .arg(argument)
            .stdin(Stdio::null())
            .env_remove("NAN_API_KEY")
            .env_remove("ZCODE_NAN_API_KEY")
            .output()
            .map_err(prepare)?;
        if !output.status.success() || output.stdout.trim_ascii() != expected.as_bytes() {
            return Err(prepare(io::Error::other(
                "the source-built CLI failed its compatibility check",
            )));
        }
    }
    Ok(())
}

fn launcher(windows: bool) -> String {
    if windows {
        format!(
            "@echo off\r\nrem nan-harness ZCode source launcher\r\nnode \"%~dp0..\\share\\nan-harness\\zcode\\{REVISION}\\{}\" %*\r\n",
            CLI.replace('/', "\\")
        )
    } else {
        format!(
            "#!/bin/sh\n# nan-harness ZCode source launcher\nexec node \"$(dirname \"$0\")/../share/nan-harness/zcode/{REVISION}/{CLI}\" \"$@\"\n"
        )
    }
}

fn check_launcher(path: &Path, expected: &str) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && fs::read_to_string(path)? == expected => Ok(()),
        Ok(_) => Err(io::Error::other(
            "the existing zcode command is not owned by nan-harness",
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn publish_launcher(path: &Path, contents: &str) -> Result<(), InstallError> {
    if path.exists() {
        return check_launcher(path, contents).map_err(prepare);
    }
    let parent = path
        .parent()
        .ok_or_else(|| prepare(io::Error::other("launcher parent is required")))?;
    let mut pending = tempfile::NamedTempFile::new_in(parent).map_err(prepare)?;
    pending.write_all(contents.as_bytes()).map_err(prepare)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        pending
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o755))
            .map_err(prepare)?;
    }
    pending
        .persist_noclobber(path)
        .map_err(|error| prepare(error.error))?;
    Ok(())
}

fn prepare(source: io::Error) -> InstallError {
    InstallError::PrepareInstaller {
        harness: HarnessKind::ZCode,
        source,
    }
}

fn reject_symlink(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(io::Error::other(
            "source installation must not follow a symbolic link",
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests;
