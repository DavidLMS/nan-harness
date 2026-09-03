use std::path::Path;

#[cfg(unix)]
use std::io::Write as _;
#[cfg(unix)]
use std::{env, fs};
#[cfg(unix)]
use tempfile::Builder as TempFileBuilder;

#[cfg(unix)]
pub(super) fn replace_running_executable(candidate: &Path) -> Result<(), std::io::Error> {
    let executable = fs::canonicalize(env::current_exe()?)?;
    replace_executable(candidate, &executable)
}

#[cfg(unix)]
fn replace_executable(candidate: &Path, executable: &Path) -> Result<(), std::io::Error> {
    let permissions = fs::metadata(executable)?.permissions();
    let directory = executable
        .parent()
        .ok_or_else(|| std::io::Error::other("the running executable has no parent directory"))?;
    let prefix = executable
        .file_stem()
        .and_then(|value| value.to_str())
        .map_or_else(
            || ".nan-harness-update-".to_owned(),
            |stem| format!(".{stem}-update-"),
        );
    let mut staged = TempFileBuilder::new()
        .prefix(&prefix)
        .tempfile_in(directory)?;
    let mut source = fs::File::open(candidate)?;
    std::io::copy(&mut source, &mut staged)?;
    staged.flush()?;
    staged.as_file().set_permissions(permissions)?;
    staged.as_file().sync_all()?;
    staged
        .into_temp_path()
        .persist(executable)
        .map_err(|error| error.error)
}

#[cfg(windows)]
pub(super) fn replace_running_executable(candidate: &Path) -> Result<(), std::io::Error> {
    self_replace::self_replace(candidate)
}

#[cfg(all(test, unix))]
mod tests {
    use super::replace_executable;
    use std::os::unix::fs::PermissionsExt as _;

    #[test]
    fn replacement_preserves_executable_permissions() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let executable = directory.path().join("nan-harness");
        let candidate = directory.path().join("candidate");
        std::fs::write(&executable, b"old").expect("executable fixture should be written");
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o751))
            .expect("executable permissions should be set");
        std::fs::write(&candidate, b"new").expect("candidate fixture should be written");

        replace_executable(&candidate, &executable).expect("replacement should succeed");

        assert_eq!(
            std::fs::read(&executable).expect("replacement should be readable"),
            b"new"
        );
        assert_eq!(
            std::fs::metadata(&executable)
                .expect("replacement metadata should exist")
                .permissions()
                .mode()
                & 0o777,
            0o751
        );
    }
}
