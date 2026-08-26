#[cfg(unix)]
use nan_harness_private_fs::open_private_truncate;
use nan_harness_private_fs::{PrivatePathKind, open_private_new, restrict_path};
use std::fs;
use std::io::ErrorKind;
use tempfile::tempdir;

#[cfg(windows)]
use nan_harness_private_fs::restrict_file;
#[cfg(windows)]
use std::fs::File;
#[cfg(windows)]
use std::path::Path;

#[cfg(unix)]
#[test]
fn unix_file_becomes_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempdir().expect("temporary directory should be created");
    let path = directory.path().join("private-file");
    fs::write(&path, b"payload").expect("test file should be created");

    restrict_path(&path, PrivatePathKind::File).expect("file permissions should be restricted");

    let mode = fs::metadata(&path)
        .expect("test file metadata should be readable")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}

#[cfg(unix)]
#[test]
fn unix_directory_becomes_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let parent = tempdir().expect("temporary directory should be created");
    let path = parent.path().join("private-directory");
    fs::create_dir(&path).expect("test directory should be created");

    restrict_path(&path, PrivatePathKind::Directory)
        .expect("directory permissions should be restricted");

    let mode = fs::metadata(&path)
        .expect("test directory metadata should be readable")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o700);
}

#[cfg(unix)]
#[test]
fn open_private_new_hardens_before_returning() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempdir().expect("temporary directory should be created");
    let path = directory.path().join("private-file");
    let _file = open_private_new(&path).expect("private file should be created");

    let mode = fs::metadata(&path)
        .expect("test file metadata should be readable")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}

#[cfg(unix)]
#[test]
fn open_private_truncate_clears_contents_and_hardens_before_returning() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempdir().expect("temporary directory should be created");
    let path = directory.path().join("private-file");
    fs::write(&path, b"stale payload").expect("existing file should be created");

    let _file = open_private_truncate(&path).expect("private file should be truncated");
    assert!(
        fs::read(&path)
            .expect("truncated file should be readable")
            .is_empty()
    );

    let mode = fs::metadata(&path)
        .expect("test file metadata should be readable")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
fn open_private_new_refuses_existing_path() {
    let directory = tempdir().expect("temporary directory should be created");
    let path = directory.path().join("existing");
    fs::write(&path, b"original").expect("existing file should be created");

    let error = open_private_new(&path).expect_err("existing path must not be replaced");
    assert_eq!(error.kind(), ErrorKind::AlreadyExists);
    assert_eq!(
        fs::read(&path).expect("existing file should remain readable"),
        b"original"
    );
}

#[test]
fn open_private_new_reports_open_failure_without_creating_a_file() {
    let directory = tempdir().expect("temporary directory should be created");
    let path = directory.path().join("missing-parent").join("file");

    let error = open_private_new(&path).expect_err("missing parent must fail the open");
    assert_eq!(error.kind(), ErrorKind::NotFound);
    assert!(!path.exists());
}

#[cfg(windows)]
fn make_permissive(path: &Path, directory: bool) {
    let inheritance = if directory {
        r"$inheritance = [System.Security.AccessControl.InheritanceFlags]([int][System.Security.AccessControl.InheritanceFlags]::ContainerInherit -bor [int][System.Security.AccessControl.InheritanceFlags]::ObjectInherit)
$rule = [System.Security.AccessControl.FileSystemAccessRule]::new(
    $everyone,
    [System.Security.AccessControl.FileSystemRights]::FullControl,
    $inheritance,
    [System.Security.AccessControl.PropagationFlags]::None,
    [System.Security.AccessControl.AccessControlType]::Allow)"
    } else {
        r"$rule = [System.Security.AccessControl.FileSystemAccessRule]::new(
    $everyone,
    [System.Security.AccessControl.FileSystemRights]::FullControl,
    [System.Security.AccessControl.AccessControlType]::Allow)"
    };
    let script = format!(
        r"$path = $args[0]
$acl = Get-Acl -LiteralPath $path
$everyone = [System.Security.Principal.SecurityIdentifier]::new('S-1-1-0')
{inheritance}
$acl.SetAccessRule($rule)
Set-Acl -LiteralPath $path -AclObject $acl
"
    );
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .arg(path.as_os_str())
        .output()
        .expect("PowerShell should run");
    assert!(
        output.status.success(),
        "permissive ACL setup failed (status: {}):\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(windows)]
#[test]
fn windows_file_has_only_owner_and_system() {
    use nan_harness_test_support::windows_acl;

    let directory = tempdir().expect("temporary directory should be created");
    let path = directory.path().join("private-file");
    let mut file = open_private_new(&path).expect("test file should be created");
    make_permissive(&path, false);

    restrict_file(&mut file).expect("file DACL should be restricted");
    windows_acl::assert_private_file(&path).expect("file DACL should be exact");
}

#[cfg(windows)]
#[test]
fn windows_directory_and_descendants_have_only_owner_and_system() {
    use nan_harness_test_support::windows_acl;

    let parent = tempdir().expect("temporary directory should be created");
    let path = parent.path().join("private-directory");
    fs::create_dir(&path).expect("test directory should be created");
    make_permissive(&path, true);

    restrict_path(&path, PrivatePathKind::Directory).expect("directory DACL should be restricted");
    windows_acl::assert_private_directory(&path).expect("directory DACL should be exact");

    let child_file = path.join("child-file");
    File::create(&child_file).expect("child file should be created");
    let child_directory = path.join("child-directory");
    fs::create_dir(&child_directory).expect("child directory should be created");
    windows_acl::assert_private_descendant(&child_file)
        .expect("child file should inherit only private entries");
    windows_acl::assert_private_descendant(&child_directory)
        .expect("child directory should inherit only private entries");
}

#[cfg(windows)]
#[test]
fn windows_missing_path_fails_loudly() {
    let directory = tempdir().expect("temporary directory should be created");
    let path = directory.path().join("missing");

    restrict_path(&path, PrivatePathKind::File)
        .expect_err("missing path must not be reported as hardened");
}
