use nan_harness_private_fs::{create_private_dir, open_private_truncate};
use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

pub(super) fn stage(source: &Path, target: &Path, owned_files: &[PathBuf]) -> io::Result<()> {
    open_private_truncate(&target.join("config.yaml"))?.write_all(b"{}")?;
    for relative in owned_files {
        let mut directory = target.to_owned();
        for component in relative.parent().into_iter().flat_map(Path::components) {
            directory.push(component);
            owned_directory(&directory)?;
        }
        copy_private(&source.join(relative), &target.join(relative))?;
    }
    dependencies(&source.join("installs"), &target.join("installs"))
}

fn owned_directory(path: &Path) -> io::Result<()> {
    match create_private_dir(path) {
        Ok(()) => Ok(()),
        Err(error)
            if error.kind() == io::ErrorKind::AlreadyExists
                && fs::symlink_metadata(path)?.file_type().is_dir() =>
        {
            Ok(())
        }
        Err(error) => Err(error),
    }
}

fn copy_private(source: &Path, target: &Path) -> io::Result<()> {
    if target
        .symlink_metadata()
        .is_ok_and(|metadata| !metadata.file_type().is_file())
    {
        return Err(io::Error::other("NH-HERMES-PROFILE-PATH"));
    }
    let mut input = fs::File::open(source)?;
    let mut output = open_private_truncate(target)?;
    io::copy(&mut input, &mut output).map(|_| ())
}

pub(super) fn dependencies(source: &Path, target: &Path) -> io::Result<()> {
    if !source.is_dir() {
        return Ok(());
    }
    owned_directory(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.len() != 16 || !name.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            continue;
        }
        let input = entry.path();
        if !input.join("facts.json").is_file() {
            continue;
        }
        let output = target.join(name);
        owned_directory(&output)?;
        copy_private(&input.join("facts.json"), &output.join("facts.json"))?;
        link_dependency_directory(&input.join("environments"), &output.join("environments"))?;
        // Modern Hermes checks application currency through its independent PM
        // runtime. Without its selection, valid copied facts look stale and each
        // managed child starts a dependency update that fences the gateway.
        let runtime = input.join("pm-runtime");
        if runtime.join("selected.json").is_file() {
            let staged = output.join("pm-runtime");
            owned_directory(&staged)?;
            copy_private(
                &runtime.join("selected.json"),
                &staged.join("selected.json"),
            )?;
            link_dependency_directory(&runtime.join("generations"), &staged.join("generations"))?;
        }
    }
    Ok(())
}

fn link_dependency_directory(source: &Path, target: &Path) -> io::Result<()> {
    if source.is_dir() && !target.try_exists()? {
        let source = fs::canonicalize(source)?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(source, target)?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(source, target)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staged_pm_selection_borrows_generations_without_sharing_mutable_state() {
        let source = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        let install = Path::new("0123456789abcdef");
        let runtime = install.join("pm-runtime");
        let generation = runtime.join("generations/committed");
        fs::create_dir_all(source.path().join(&generation)).unwrap();
        fs::write(source.path().join(install).join("facts.json"), "{}").unwrap();
        let selection = r#"{"generation":"generations/committed","inputs":"synthetic"}"#;
        fs::write(
            source.path().join(&runtime).join("selected.json"),
            selection,
        )
        .unwrap();
        fs::write(
            source.path().join(&runtime).join(".prepare.lock"),
            "native lock",
        )
        .unwrap();
        fs::write(
            source.path().join(&generation).join("pm-runtime.json"),
            "{}",
        )
        .unwrap();

        dependencies(source.path(), target.path()).unwrap();
        let staged = target.path().join(&runtime);
        assert_eq!(
            fs::read_to_string(staged.join("selected.json")).unwrap(),
            selection
        );
        assert_eq!(
            fs::canonicalize(staged.join("generations")).unwrap(),
            fs::canonicalize(source.path().join(&runtime).join("generations")).unwrap()
        );
        assert!(!staged.join(".prepare.lock").exists());
        fs::write(staged.join("selected.json"), "private selection").unwrap();
        assert_eq!(
            fs::read_to_string(source.path().join(&runtime).join("selected.json")).unwrap(),
            selection
        );
        target.close().unwrap();
        assert!(
            source
                .path()
                .join(&generation)
                .join("pm-runtime.json")
                .is_file()
        );
    }

    #[test]
    fn staged_profile_copies_only_declared_files_and_private_dependency_facts() {
        let source = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        let plugin = PathBuf::from("plugins/model-providers/nan/__init__.py");
        fs::create_dir_all(source.path().join(plugin.parent().unwrap())).unwrap();
        fs::write(source.path().join(&plugin), "owned provider").unwrap();
        fs::write(
            source.path().join("config.yaml"),
            "synthetic native credential",
        )
        .unwrap();
        fs::write(source.path().join("state.db"), "native history").unwrap();
        fs::write(source.path().join("gateway.lock"), "native authority").unwrap();
        let metadata = Path::new("installs/0123456789abcdef/facts.json");
        fs::create_dir_all(source.path().join(metadata.parent().unwrap())).unwrap();
        fs::write(
            source.path().join(metadata),
            "synthetic dependency metadata",
        )
        .unwrap();
        fs::create_dir_all(source.path().join("installs/1111111111111111")).unwrap();
        stage(source.path(), target.path(), std::slice::from_ref(&plugin)).unwrap();
        assert_eq!(
            fs::read_to_string(target.path().join("config.yaml")).unwrap(),
            "{}"
        );
        assert_eq!(
            fs::read_to_string(target.path().join(&plugin)).unwrap(),
            "owned provider"
        );
        assert!(!target.path().join("state.db").exists());
        assert!(!target.path().join("gateway.lock").exists());
        let file = fs::File::open(target.path().join(metadata)).unwrap();
        #[cfg(windows)]
        nan_harness_private_fs::verify_private_file(&file).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(file.metadata().unwrap().permissions().mode() & 0o777, 0o600);
        }
        fs::write(target.path().join(metadata), "changed private copy").unwrap();
        assert_eq!(
            fs::read_to_string(source.path().join(metadata)).unwrap(),
            "synthetic dependency metadata"
        );
    }
}
