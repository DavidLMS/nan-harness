#[cfg(unix)]
use nan_harness_private_fs::open_private_truncate;
use nan_harness_private_fs::{
    PrivateFileReadStatus, PrivatePathKind, create_private_dir, create_private_dir_all,
    open_private_new, open_private_read, restrict_path,
};
use std::fs;
use std::io::ErrorKind;
use tempfile::tempdir;

#[cfg(windows)]
use nan_harness_private_fs::restrict_file;
#[cfg(windows)]
use std::fs::File;

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
fn unix_private_directories_are_owner_only_from_creation() {
    use std::os::unix::fs::PermissionsExt;

    let parent = tempdir().expect("temporary directory should be created");
    let direct = parent.path().join("direct");
    create_private_dir(&direct).expect("private directory should be created");
    let nested = parent.path().join("nested/child");
    create_private_dir_all(&nested).expect("private directory tree should be created");

    for path in [direct, parent.path().join("nested"), nested] {
        let mode = fs::metadata(&path)
            .expect("private directory metadata should be readable")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700, "{} should be owner-only", path.display());
    }
}

#[cfg(unix)]
#[test]
fn create_private_dir_all_leaves_preexisting_directory_mode_unchanged() {
    use std::os::unix::fs::PermissionsExt;

    let parent = tempdir().expect("temporary directory should be created");
    let path = parent.path().join("existing");
    fs::create_dir(&path).expect("test directory should be created");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
        .expect("test directory should be made permissive");

    create_private_dir_all(&path).expect("existing directory should be accepted");

    let mode = fs::metadata(&path)
        .expect("test directory metadata should be readable")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o755);
}

#[test]
fn private_directory_creation_rejects_existing_files() {
    let parent = tempdir().expect("temporary directory should be created");
    let path = parent.path().join("existing-file");
    fs::write(&path, b"payload").expect("test file should be created");

    let direct_error =
        create_private_dir(&path).expect_err("direct creation must reject an existing file");
    assert_eq!(direct_error.kind(), ErrorKind::AlreadyExists);
    let nested_error = create_private_dir_all(&path.join("child"))
        .expect_err("recursive creation must reject a file component");
    assert_eq!(nested_error.kind(), ErrorKind::NotADirectory);
    assert_eq!(
        fs::read(&path).expect("file should remain readable"),
        b"payload"
    );
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

#[cfg(unix)]
#[test]
fn open_private_read_repairs_permissive_file_and_reads_same_handle() {
    use std::io::Read as _;
    use std::os::unix::fs::PermissionsExt;

    let directory = tempdir().expect("temporary directory should be created");
    let path = directory.path().join("private-file");
    fs::write(&path, b"payload").expect("test file should be created");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644))
        .expect("test file should be made permissive");

    let (mut file, status) = open_private_read(&path).expect("test file should open privately");
    let mut contents = Vec::new();
    file.read_to_end(&mut contents)
        .expect("opened file should remain readable");

    assert_eq!(status, PrivateFileReadStatus::Repaired);
    assert_eq!(contents, b"payload");
    assert_eq!(
        fs::metadata(&path)
            .expect("repaired file metadata should be readable")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[cfg(unix)]
#[test]
fn open_private_read_preserves_an_already_private_mode() {
    use std::io::Read as _;
    use std::os::unix::fs::PermissionsExt;

    let directory = tempdir().expect("temporary directory should be created");
    let path = directory.path().join("private-file");
    fs::write(&path, b"payload").expect("test file should be created");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o400))
        .expect("test file should be owner-only");

    let (mut file, status) = open_private_read(&path).expect("test file should open privately");
    let mut contents = Vec::new();
    file.read_to_end(&mut contents)
        .expect("opened file should remain readable");

    assert_eq!(status, PrivateFileReadStatus::AlreadyPrivate);
    assert_eq!(contents, b"payload");
    assert_eq!(
        fs::metadata(&path)
            .expect("private file metadata should be readable")
            .permissions()
            .mode()
            & 0o777,
        0o400
    );
}

#[cfg(unix)]
#[test]
fn open_private_read_repairs_a_symlink_target_on_the_open_handle() {
    use std::io::Read as _;
    use std::os::unix::fs::{PermissionsExt, symlink};

    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("target-file");
    let link = directory.path().join("linked-file");
    fs::write(&target, b"linked payload").expect("target file should be created");
    fs::set_permissions(&target, fs::Permissions::from_mode(0o644))
        .expect("target file should be made permissive");
    symlink(&target, &link).expect("test symlink should be created");

    let (mut file, status) =
        open_private_read(&link).expect("symlink target should open privately");
    let mut contents = Vec::new();
    file.read_to_end(&mut contents)
        .expect("opened target should remain readable");

    assert_eq!(status, PrivateFileReadStatus::Repaired);
    assert_eq!(contents, b"linked payload");
    assert_eq!(
        fs::metadata(&target)
            .expect("target metadata should be readable")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert!(
        fs::symlink_metadata(&link)
            .expect("symlink metadata should be readable")
            .file_type()
            .is_symlink()
    );
}

#[test]
fn open_private_read_reports_a_missing_file() {
    let directory = tempdir().expect("temporary directory should be created");
    let path = directory.path().join("missing");

    let error = open_private_read(&path).expect_err("missing file must fail closed");
    assert_eq!(error.kind(), ErrorKind::NotFound);
}

#[cfg(unix)]
#[test]
fn open_private_read_rejects_a_directory_without_changing_its_mode() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempdir().expect("temporary directory should be created");
    let path = directory.path().join("not-a-file");
    fs::create_dir(&path).expect("test directory should be created");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
        .expect("test directory should be made permissive");

    let error = open_private_read(&path).expect_err("directory must fail the private file open");

    assert_eq!(error.kind(), ErrorKind::InvalidInput);
    assert_eq!(
        fs::metadata(&path)
            .expect("directory metadata should remain readable")
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
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
#[test]
fn windows_file_has_only_owner_and_system() {
    use nan_harness_test_support::windows_acl;

    let directory = tempdir().expect("temporary directory should be created");
    let path = directory.path().join("private-file");
    let mut file = open_private_new(&path).expect("test file should be created");
    windows_acl::make_permissive_file(&path).expect("file ACL should be made permissive");

    restrict_file(&mut file).expect("file DACL should be restricted");
    windows_acl::assert_private_file(&path).expect("file DACL should be exact");
}

#[cfg(windows)]
#[test]
fn windows_open_private_read_repairs_and_reads_the_same_file() {
    use nan_harness_test_support::windows_acl;
    use std::io::Read as _;

    let directory = tempdir().expect("temporary directory should be created");
    let path = directory.path().join("private-file");
    fs::write(&path, b"payload").expect("test file should be created");
    windows_acl::make_permissive_file(&path).expect("file ACL should be made permissive");

    let (mut file, status) = open_private_read(&path).expect("test file should open privately");
    let mut contents = Vec::new();
    file.read_to_end(&mut contents)
        .expect("opened file should remain readable");

    assert_eq!(status, PrivateFileReadStatus::Repaired);
    assert_eq!(contents, b"payload");
    windows_acl::assert_private_file(&path).expect("repaired file DACL should be exact");
}

#[cfg(windows)]
#[test]
fn windows_directory_and_descendants_have_only_owner_and_system() {
    use nan_harness_test_support::windows_acl;

    let parent = tempdir().expect("temporary directory should be created");
    let path = parent.path().join("private-directory");
    fs::create_dir(&path).expect("test directory should be created");
    windows_acl::make_permissive_directory(&path).expect("directory ACL should be made permissive");

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
fn windows_private_directory_creation_applies_exact_dacls() {
    use nan_harness_test_support::windows_acl;

    let parent = tempdir().expect("temporary directory should be created");
    let direct = parent.path().join("direct");
    create_private_dir(&direct).expect("private directory should be created");
    windows_acl::assert_private_directory(&direct).expect("direct directory DACL should be exact");

    let nested = parent.path().join("nested/child");
    create_private_dir_all(&nested).expect("private directory tree should be created");
    windows_acl::assert_private_directory(&parent.path().join("nested"))
        .expect("private ancestor DACL should be exact");
    windows_acl::assert_private_directory(&nested).expect("nested directory DACL should be exact");
}

#[cfg(windows)]
#[test]
fn windows_recursive_creation_leaves_preexisting_dacl_unchanged() {
    use nan_harness_test_support::windows_acl;

    let parent = tempdir().expect("temporary directory should be created");
    let path = parent.path().join("existing");
    fs::create_dir(&path).expect("test directory should be created");
    windows_acl::make_permissive_directory(&path).expect("test DACL should be made permissive");

    create_private_dir_all(&path).expect("existing directory should be accepted");

    windows_acl::assert_private_directory(&path)
        .expect_err("preexisting directory DACL should remain permissive");
}

#[cfg(windows)]
#[test]
fn windows_missing_path_fails_loudly() {
    let directory = tempdir().expect("temporary directory should be created");
    let path = directory.path().join("missing");

    restrict_path(&path, PrivatePathKind::File)
        .expect_err("missing path must not be reported as hardened");
}
