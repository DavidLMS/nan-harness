use super::{
    canonical_origin, duration_millis, fingerprint, load_or_create_salt, load_or_create_salt_with,
    read_salt,
};
use nan_harness_private_fs::open_private_new;
use std::cell::Cell;
use std::io::{Error, ErrorKind, Write as _};
use std::path::Path;
use std::time::Duration;

#[test]
fn duration_millis_rounds_sub_milliseconds_and_saturates_u64() {
    assert_eq!(duration_millis(Duration::ZERO), 0);
    assert_eq!(duration_millis(Duration::from_nanos(999)), 0);
    assert_eq!(duration_millis(Duration::from_millis(1_500)), 1_500);
    assert_eq!(
        duration_millis(Duration::from_secs(u64::MAX / 1000 + 1)),
        u64::MAX
    );
}

#[test]
fn scope_fingerprint_is_bound_to_salt_origin_and_credential() {
    let salt = [7_u8; 32];
    let scope = fingerprint(&salt, "https://api.example.com", "test-secret");

    assert_ne!(
        scope,
        fingerprint(&[8_u8; 32], "https://api.example.com", "test-secret")
    );
    assert_ne!(
        scope,
        fingerprint(&salt, "https://other.example.com", "test-secret")
    );
    assert_ne!(
        scope,
        fingerprint(&salt, "https://api.example.com", "other-secret")
    );
}

#[test]
fn canonical_origin_is_stable_for_the_same_host() {
    assert_eq!(
        canonical_origin("https://API.example.COM:443/v1?attempt=one"),
        Some("https://api.example.com".to_owned())
    );
    assert_eq!(
        canonical_origin("https://api.example.com/v1?attempt=two"),
        Some("https://api.example.com".to_owned())
    );
}

#[test]
fn read_salt_rejects_short_and_long_files_as_invalid_data() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let short_path = temporary.path().join("short.salt");
    let long_path = temporary.path().join("long.salt");
    write_private_salt(&short_path, &[9_u8; 31]);
    write_private_salt(&long_path, &[9_u8; 33]);

    let short = read_salt(&short_path).expect_err("a short salt should be rejected");
    let long = read_salt(&long_path).expect_err("a long salt should be rejected");

    assert_eq!(short.kind(), ErrorKind::InvalidData);
    assert_eq!(long.kind(), ErrorKind::InvalidData);
}

#[test]
fn read_salt_preserves_an_exact_32_byte_salt() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let expected: Vec<u8> = (0..32).map(|byte| byte * 7).collect();
    let path = temporary.path().join("scope.salt");
    write_private_salt(&path, &expected);

    let loaded = read_salt(&path).expect("an exact salt should be readable");

    assert_eq!(loaded, expected);
}

#[test]
fn load_or_create_salt_reuses_an_existing_published_salt() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let expected = vec![11_u8; 32];
    let path = temporary.path().join("scope.salt");
    write_private_salt(&path, &expected);
    let published_before = std::fs::metadata(&path)
        .expect("published salt metadata should exist")
        .modified()
        .expect("published salt modification time should exist");

    let loaded = load_or_create_salt(temporary.path()).expect("published salt should be reused");
    let published_after = std::fs::metadata(&path)
        .expect("published salt metadata should remain")
        .modified()
        .expect("published salt modification time should remain");

    assert_eq!(loaded, expected);
    assert_eq!(published_before, published_after);
    let published = std::fs::read(&path).expect("published salt should remain readable");
    assert_eq!(published, expected);
}

#[test]
fn a_non_retryable_read_failure_is_preserved_without_creating_or_rereading() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = temporary.path().join("scope.salt");
    let reads = ScriptedReads::new(&[ErrorKind::PermissionDenied, ErrorKind::BrokenPipe]);

    let error = load_or_create_salt_with(&path, &|path| reads.next(path))
        .expect_err("a non-retryable read failure should be reported");

    assert_eq!(
        error.kind(),
        ErrorKind::PermissionDenied,
        "the original read failure must be preserved instead of a later one"
    );
    assert_eq!(reads.performed(), 1, "the failed read must not be retried");
    assert!(
        !path.exists(),
        "a salt must not be published after a read failure that is not a missing file"
    );
}

/// Answers each read with the next scripted failure, so that entering the publication
/// wait after a non-retryable read is visible as a different reported error.
struct ScriptedReads {
    failures: Vec<ErrorKind>,
    performed: Cell<usize>,
}

impl ScriptedReads {
    fn new(failures: &[ErrorKind]) -> Self {
        Self {
            failures: failures.to_vec(),
            performed: Cell::new(0),
        }
    }

    /// Answers one read with its scripted failure, repeating the last one afterwards.
    fn next(&self, path: &Path) -> Result<Vec<u8>, Error> {
        let performed = self.performed.get();
        self.performed.set(performed + 1);
        let kind = self.failures[performed.min(self.failures.len() - 1)];
        Err(Error::new(
            kind,
            format!("scripted read failure for {}", path.display()),
        ))
    }

    fn performed(&self) -> usize {
        self.performed.get()
    }
}

fn write_private_salt(path: &Path, bytes: &[u8]) {
    let mut file = open_private_new(path).expect("private salt fixture should be created");
    file.write_all(bytes)
        .expect("salt fixture should be written");
    file.sync_all().expect("salt fixture should be durable");
}
