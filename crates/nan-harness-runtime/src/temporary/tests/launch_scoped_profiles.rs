use crate::temporary::lifecycle::{SCOPED_FILES_LOCK_NAME, ScopedFileLifecycleEvent};
use crate::temporary::{TemporaryError, TemporaryWorkspace};
use nan_harness_core::launch_plan::{
    ArtifactLifecycle, LaunchScopedFile, TemporaryArtifactMode, USER_HOME_PLACEHOLDER,
};
use std::fs::{self, File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

mod process_fixture;
mod recovery;

#[test]
fn launch_scoped_profiles_are_private_and_removed_on_drop() {
    let home = tempfile::tempdir().expect("temporary home should exist");
    let codex_home = home.path().join(".codex");
    fs::create_dir_all(&codex_home).expect("Codex home should exist");
    fs::write(codex_home.join("config.toml"), "notify = [\"true\"]\n")
        .expect("base config should exist");
    let files = [codex_profile("launch_01scopedfile")];

    let workspace = TemporaryWorkspace::materialize_with_home_and_scoped(
        &[],
        &[],
        &files,
        home.path(),
        |_, content| Ok(content.to_owned()),
    )
    .expect("profile should materialize");
    let profile = workspace
        .path("codex-profile")
        .expect("profile path should exist")
        .to_path_buf();
    let lock = profile.with_file_name(format!(
        "{}.lock",
        profile
            .file_name()
            .expect("profile name should exist")
            .to_string_lossy()
    ));
    let directory_lock = codex_home.join(SCOPED_FILES_LOCK_NAME);

    assert_eq!(
        fs::read_to_string(&profile).expect("profile should be readable"),
        "model = \"qwen3.6\"\n"
    );
    assert!(lock.exists());
    assert!(directory_lock.exists());
    assert_eq!(
        fs::read_to_string(codex_home.join("config.toml"))
            .expect("base config should remain readable"),
        "notify = [\"true\"]\n"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&profile)
                .expect("profile metadata should exist")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(&directory_lock)
                .expect("directory lock metadata should exist")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    #[cfg(windows)]
    nan_harness_test_support::windows_acl::assert_private_file(&profile)
        .expect("launch-scoped profile should have a private protected DACL");
    #[cfg(windows)]
    nan_harness_test_support::windows_acl::assert_private_file(&directory_lock)
        .expect("directory lock should have a private protected DACL");

    drop(workspace);
    assert!(!profile.exists());
    assert!(!lock.exists());
    assert!(directory_lock.exists());
}

#[test]
fn three_overlapping_launches_preserve_each_other() {
    let home = tempfile::tempdir().expect("temporary home should exist");
    let codex_home = home.path().join(".codex");
    fs::create_dir_all(&codex_home).expect("Codex home should exist");
    let stale = codex_home.join("nan-harness-launch_01staleprofile.config.toml");
    let stale_lock = codex_home.join("nan-harness-launch_01staleprofile.config.toml.lock");
    fs::write(&stale, "stale").expect("stale profile should exist");
    fs::write(&stale_lock, "").expect("stale lock should exist");

    let launch_ids = [
        "launch_01firstactive",
        "launch_01secondactive",
        "launch_01thirdactive",
    ];
    let (ready_tx, ready_rx) = mpsc::channel();
    let mut releases = Vec::new();
    let mut workers = Vec::new();
    for launch_id in launch_ids {
        let ready_tx = ready_tx.clone();
        let (release_tx, release_rx) = mpsc::sync_channel(0);
        let child_home = home.path().to_path_buf();
        workers.push(thread::spawn(move || {
            let files = [codex_profile(launch_id)];
            TemporaryWorkspace::materialize_with_home_and_scoped_observing(
                &[],
                &[],
                &files,
                &child_home,
                |_, content| Ok(content.to_owned()),
                |event| {
                    if event == ScopedFileLifecycleEvent::BeforeDirectoryLock {
                        ready_tx.send(()).expect("launch should report readiness");
                        release_rx
                            .recv_timeout(Duration::from_secs(5))
                            .expect("launch should be released");
                    }
                },
            )
            .expect("overlapping profile should materialize")
        }));
        releases.push(release_tx);
    }
    for _ in launch_ids {
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("every launch should reach the start barrier");
    }
    for release in releases {
        release.send(()).expect("launch should start");
    }
    let mut workspaces = workers
        .drain(..)
        .map(|worker| worker.join().expect("launch thread should finish"))
        .collect::<Vec<_>>();
    let profiles = workspaces
        .iter()
        .map(|workspace| {
            workspace
                .path("codex-profile")
                .expect("profile should exist")
                .to_path_buf()
        })
        .collect::<Vec<_>>();

    assert!(!stale.exists());
    assert!(!stale_lock.exists());
    assert!(profiles.iter().all(|profile| profile.exists()));

    drop(workspaces.remove(1));
    assert!(profiles[0].exists());
    assert!(!profiles[1].exists());
    assert!(profiles[2].exists());
    drop(workspaces);
    assert!(profiles.iter().all(|profile| !profile.exists()));
}

#[test]
fn launch_scoped_publication_holds_the_directory_lock() {
    let home = tempfile::tempdir().expect("temporary home should exist");
    let codex_home = home.path().join(".codex");
    fs::create_dir_all(&codex_home).expect("Codex home should exist");
    let (published_tx, published_rx) = mpsc::sync_channel(0);
    let (resume_tx, resume_rx) = mpsc::sync_channel(0);
    let child_home = home.path().to_path_buf();

    let creator = thread::spawn(move || {
        let files = [codex_profile("launch_01pausedcreator")];
        TemporaryWorkspace::materialize_with_home_and_scoped_observing(
            &[],
            &[],
            &files,
            &child_home,
            |_, content| Ok(content.to_owned()),
            |event| {
                if event == ScopedFileLifecycleEvent::SessionLockPublished {
                    published_tx
                        .send(())
                        .expect("publication checkpoint should be observed");
                    resume_rx
                        .recv_timeout(Duration::from_secs(5))
                        .expect("creator should be resumed");
                }
            },
        )
    });

    published_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("creator should publish its session lock");
    let directory_lock_is_held = OpenOptions::new()
        .read(true)
        .write(true)
        .open(codex_home.join(SCOPED_FILES_LOCK_NAME))
        .is_ok_and(|file| match file.try_lock() {
            Err(TryLockError::WouldBlock) => true,
            Ok(()) => {
                File::unlock(&file).expect("test probe should unlock");
                false
            }
            Err(TryLockError::Error(_)) => false,
        });
    let (attempted_tx, attempted_rx) = mpsc::sync_channel(0);
    let cleaner_home = home.path().to_path_buf();
    let cleaner = thread::spawn(move || {
        let files = [codex_profile("launch_01overlappingcleaner")];
        TemporaryWorkspace::materialize_with_home_and_scoped_observing(
            &[],
            &[],
            &files,
            &cleaner_home,
            |_, content| Ok(content.to_owned()),
            |event| {
                if event == ScopedFileLifecycleEvent::BeforeDirectoryLock {
                    attempted_tx
                        .send(())
                        .expect("cleanup attempt should be observed");
                }
            },
        )
    });
    attempted_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("cleanup should overlap paused publication");
    resume_tx.send(()).expect("creator should resume");
    let workspace = creator
        .join()
        .expect("creator thread should finish")
        .expect("profile should materialize");
    let cleaner_workspace = cleaner
        .join()
        .expect("cleaner thread should finish")
        .expect("overlapping profile should materialize");
    let first_profile = workspace
        .path("codex-profile")
        .expect("creator profile should exist");

    assert!(
        directory_lock_is_held,
        "publication must exclude concurrent orphan reclamation"
    );
    assert!(first_profile.exists());
    let third = materialize_profile(home.path(), "launch_01postoverlap");
    assert!(first_profile.exists());
    drop(third);
    drop(cleaner_workspace);
    drop(workspace);
}

#[test]
fn failed_profile_create_preserves_the_colliding_file() {
    let home = tempfile::tempdir().expect("temporary home should exist");
    let codex_home = home.path().join(".codex");
    fs::create_dir_all(&codex_home).expect("Codex home should exist");
    let files = [codex_profile("launch_01collision")];
    let profile = codex_home.join(&files[0].file_name);

    let result = TemporaryWorkspace::materialize_with_home_and_scoped_observing(
        &[],
        &[],
        &files,
        home.path(),
        |_, content| Ok(content.to_owned()),
        |event| {
            if event == ScopedFileLifecycleEvent::BeforeProfileCreate {
                fs::write(&profile, "pre-existing").expect("colliding profile should be published");
            }
        },
    );

    assert!(
        result.is_err(),
        "exclusive creation should reject a collision"
    );
    assert_eq!(
        fs::read_to_string(&profile).expect("colliding profile should be preserved"),
        "pre-existing"
    );
    assert!(!session_lock_path(&profile).exists());
}

#[cfg(unix)]
#[test]
fn failed_profile_create_preserves_a_colliding_symlink_and_target() {
    use std::os::unix::fs::symlink;
    let home = tempfile::tempdir().expect("temporary home should exist");
    let codex_home = home.path().join(".codex");
    fs::create_dir_all(&codex_home).expect("Codex home should exist");
    let files = [codex_profile("launch_01symlinkcollision")];
    let profile = codex_home.join(&files[0].file_name);
    let target = home.path().join("user-owned-target");
    fs::write(&target, "user-owned").expect("symlink target should exist");
    let result = TemporaryWorkspace::materialize_with_home_and_scoped_observing(
        &[],
        &[],
        &files,
        home.path(),
        |_, content| Ok(content.to_owned()),
        |event| {
            if event == ScopedFileLifecycleEvent::BeforeProfileCreate {
                symlink(&target, &profile).expect("colliding symlink should be published");
            }
        },
    );

    assert!(
        result.is_err(),
        "exclusive creation should reject a symlink"
    );
    assert!(
        fs::symlink_metadata(&profile)
            .expect("colliding symlink should remain")
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        fs::read_to_string(&target).expect("symlink target should remain readable"),
        "user-owned"
    );
    assert!(!session_lock_path(&profile).exists());
}

#[test]
fn stable_directory_lock_is_reused_without_truncation() {
    let home = tempfile::tempdir().expect("temporary home should exist");
    let first = materialize_profile(home.path(), "launch_01firstlockuser");
    let lock_path = home.path().join(".codex").join(SCOPED_FILES_LOCK_NAME);
    drop(first);
    fs::write(&lock_path, "existing-lock-marker").expect("lock marker should be written");
    let second = materialize_profile(home.path(), "launch_01secondlockuser");
    assert_eq!(
        fs::read_to_string(&lock_path).expect("stable lock should remain readable"),
        "existing-lock-marker"
    );
    drop(second);
    assert!(lock_path.exists());
}

#[test]
fn nonregular_directory_lock_fails_before_orphan_cleanup() {
    let home = tempfile::tempdir().expect("temporary home should exist");
    let codex_home = home.path().join(".codex");
    fs::create_dir_all(&codex_home).expect("Codex home should exist");
    fs::create_dir(codex_home.join(SCOPED_FILES_LOCK_NAME))
        .expect("nonregular coordination entry should exist");
    let stale = codex_home.join("nan-harness-launch_01preserved.config.toml");
    fs::write(&stale, "preserve me").expect("candidate profile should exist");
    let error = materialize_profile_error(home.path(), "launch_01blocked");
    // Platforms can reject directory collisions before our regular-file check.
    assert!(
        matches!(error, TemporaryError::Materialize { .. }),
        "{error:?}"
    );
    assert!(codex_home.join(SCOPED_FILES_LOCK_NAME).is_dir());
    let blocked_profile = profile_path(&codex_home, "launch_01blocked");
    assert!(!blocked_profile.exists());
    assert!(!session_lock_path(&blocked_profile).exists());
    assert_eq!(
        fs::read_to_string(&stale).expect("cleanup must not start without coordination"),
        "preserve me"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let lock_path = codex_home.join(SCOPED_FILES_LOCK_NAME);
        fs::remove_dir(&lock_path).expect("directory fixture should be removable");
        fs::write(&lock_path, "").expect("file fixture should exist");
        fs::set_permissions(&lock_path, fs::Permissions::from_mode(0o000))
            .expect("file fixture should be inaccessible");
        let error = materialize_profile_error(home.path(), "launch_01denied");
        assert!(matches!(
            error,
            TemporaryError::Materialize { source, .. }
                if source.kind() == std::io::ErrorKind::PermissionDenied
        ));
        assert!(stale.exists());
        fs::set_permissions(lock_path, fs::Permissions::from_mode(0o600))
            .expect("file fixture permissions should be restored");
    }
}

#[test]
fn directory_lock_timeout_preserves_profiles() {
    let home = tempfile::tempdir().expect("temporary home should exist");
    drop(materialize_profile(home.path(), "launch_01initializer"));
    let codex_home = home.path().join(".codex");
    let lock_path = codex_home.join(SCOPED_FILES_LOCK_NAME);
    let lock_file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lock_path)
        .expect("coordination lock should open");
    File::lock(&lock_file).expect("coordination lock should be held by the fixture");
    let stale = codex_home.join("nan-harness-launch_01timeout-stale.config.toml");
    fs::write(&stale, "preserve me").expect("candidate profile should exist");

    let started = Instant::now();
    let error = materialize_profile_error(home.path(), "launch_01timedout");
    let elapsed = started.elapsed();
    assert!(matches!(
        error,
        TemporaryError::Materialize { source, .. }
            if source.kind() == std::io::ErrorKind::TimedOut
    ));
    assert!(elapsed >= Duration::from_secs(5));
    assert_eq!(
        fs::read_to_string(stale).expect("timed-out cleanup must preserve profiles"),
        "preserve me"
    );
    File::unlock(&lock_file).expect("fixture should release coordination lock");
}

#[test]
fn killed_owner_artifacts_are_reclaimed_by_a_later_launch() {
    if process_fixture::is_child() {
        process_fixture::run_child();
    }

    let home = tempfile::tempdir().expect("temporary home should exist");
    let codex_home = home.path().join(".codex");
    fs::create_dir_all(&codex_home).expect("Codex home should exist");
    let base = codex_home.join("config.toml");
    let unrelated = codex_home.join("user-owned.txt");
    fs::write(&base, "notify = [\"true\"]\n").expect("base config should exist");
    fs::write(&unrelated, "keep me").expect("unrelated file should exist");
    let ready = home.path().join("killed-owner-ready");
    let orphan = profile_path(&codex_home, "launch_01killedowner");
    let mut child = process_fixture::KillOnDrop::spawn(home.path(), &ready);
    process_fixture::wait_until_ready(child.child_mut(), &ready);
    assert!(orphan.exists());
    assert!(session_lock_path(&orphan).exists());

    child.terminate();
    let recovery = materialize_profile(home.path(), "launch_01recovery");
    assert!(!orphan.exists());
    assert!(!session_lock_path(&orphan).exists());
    assert_eq!(
        fs::read_to_string(base).expect("base config should remain readable"),
        "notify = [\"true\"]\n"
    );
    assert_eq!(
        fs::read_to_string(unrelated).expect("unrelated file should remain readable"),
        "keep me"
    );
    drop(recovery);
}

fn materialize_profile(home: &Path, launch_id: &str) -> TemporaryWorkspace {
    let files = [codex_profile(launch_id)];
    TemporaryWorkspace::materialize_with_home_and_scoped(&[], &[], &files, home, |_, content| {
        Ok(content.to_owned())
    })
    .expect("profile should materialize")
}

fn materialize_profile_error(home: &Path, launch_id: &str) -> TemporaryError {
    let files = [codex_profile(launch_id)];
    match TemporaryWorkspace::materialize_with_home_and_scoped(
        &[],
        &[],
        &files,
        home,
        |_, content| Ok(content.to_owned()),
    ) {
        Ok(_) => panic!("profile materialization should fail"),
        Err(error) => error,
    }
}

fn profile_path(codex_home: &Path, launch_id: &str) -> PathBuf {
    codex_home.join(codex_profile(launch_id).file_name)
}

fn session_lock_path(profile: &Path) -> PathBuf {
    profile.with_file_name(format!(
        "{}.lock",
        profile
            .file_name()
            .expect("profile name should exist")
            .to_string_lossy()
    ))
}

fn codex_profile(launch_id: &str) -> LaunchScopedFile {
    LaunchScopedFile {
        id: "codex-profile".to_owned(),
        directory: format!("{USER_HOME_PLACEHOLDER}/.codex"),
        file_name: format!("nan-harness-{launch_id}.config.toml"),
        ownership_prefix: "nan-harness-launch_".to_owned(),
        mode: TemporaryArtifactMode::OwnerFile,
        content_template: "model = \"qwen3.6\"\n".to_owned(),
        lifecycle: ArtifactLifecycle::Launch,
    }
}
