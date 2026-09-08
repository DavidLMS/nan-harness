use super::{debian_data, extract_tar, safe_relative, validate_link};
use crate::install::InstallError;
use std::fs;
use std::io::Cursor;
use std::path::Path;

fn tar_file(name: &str, contents: &[u8]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_size(contents.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();
    builder
        .append_data(&mut header, name, contents)
        .expect("tar fixture");
    builder.into_inner().expect("tar bytes")
}

#[test]
fn extracts_executable_without_writing_outside_the_private_root() {
    let root = tempfile::tempdir().expect("fixture");
    extract_tar(
        Cursor::new(tar_file("zed.app/bin/zed", b"synthetic")),
        root.path(),
    )
    .expect("safe extraction");
    assert_eq!(
        fs::read(root.path().join("zed.app/bin/zed")).expect("file"),
        b"synthetic"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(
            fs::metadata(root.path().join("zed.app/bin/zed"))
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
}

#[test]
fn archive_paths_and_link_targets_cannot_escape() {
    for path in ["../outside", "/absolute", "dir/../../outside"] {
        assert!(safe_relative(Path::new(path)).is_err());
    }
    assert!(validate_link(Path::new("dir/link"), Path::new("../../outside")).is_err());
    assert!(validate_link(Path::new("dir/link"), Path::new("/outside")).is_err());
    assert!(validate_link(Path::new("dir/link"), Path::new("../file")).is_ok());
}

#[test]
fn duplicate_entries_cannot_overwrite_extracted_content() {
    let mut bytes = tar_file("file", b"first");
    bytes.truncate(1024);
    bytes.extend(tar_file("file", b"second"));
    let root = tempfile::tempdir().expect("fixture");
    assert!(extract_tar(Cursor::new(bytes), root.path()).is_err());
    assert_eq!(fs::read(root.path().join("file")).expect("file"), b"first");
}

#[test]
fn debian_container_requires_one_supported_data_member() {
    let root = tempfile::tempdir().expect("fixture");
    let input = root.path().join("package.deb");
    let payload = tar_file("usr/lib/chatgpt/ChatGPT", b"fixture");
    let mut bytes = b"!<arch>\n".to_vec();
    let header = format!(
        "{:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n",
        "data.tar/",
        0,
        0,
        0,
        "100644",
        payload.len()
    );
    assert_eq!(header.len(), 60);
    bytes.extend(header.as_bytes());
    bytes.extend(&payload);
    fs::write(&input, &bytes).expect("deb fixture");
    let output = root.path().join("data");
    assert_eq!(debian_data(&input, &output).expect("deb data"), "data.tar");
    assert_eq!(fs::read(output).expect("data"), payload);
    fs::write(&input, b"not a deb archive").expect("bad fixture");
    assert!(matches!(
        debian_data(&input, &root.path().join("other")),
        Err(InstallError::Archive)
    ));
}

#[cfg(unix)]
#[test]
fn symlink_entries_cannot_be_used_as_extraction_parents() {
    let mut builder = tar::Builder::new(Vec::new());
    let mut link = tar::Header::new_gnu();
    link.set_entry_type(tar::EntryType::Symlink);
    link.set_size(0);
    link.set_mode(0o777);
    builder
        .append_link(&mut link, "alias", "real")
        .expect("link fixture");
    let mut file = tar::Header::new_gnu();
    file.set_size(4);
    file.set_mode(0o600);
    builder
        .append_data(&mut file, "alias/file", &b"test"[..])
        .expect("file fixture");
    let bytes = builder.into_inner().expect("tar bytes");
    let root = tempfile::tempdir().expect("fixture");
    assert!(extract_tar(Cursor::new(bytes), root.path()).is_err());
    assert!(!root.path().join("real/file").exists());
}
