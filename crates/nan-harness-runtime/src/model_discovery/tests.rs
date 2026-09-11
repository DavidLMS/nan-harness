use super::*;
use nan_harness_core::coding_models_from_provider_ids;
use std::fs;

fn models(id: &str) -> Vec<CodingModelProfile> {
    coding_models_from_provider_ids([id.to_owned()])
}

#[test]
fn persisted_catalog_survives_failures_and_has_no_expiry() {
    let directory = tempfile::tempdir().unwrap();
    let cache = ModelCache::open(directory.path(), "https://provider.test/v1", "test-key").unwrap();
    cache.save(&models("qwen3.6"), 1).unwrap();
    let reopened =
        ModelCache::open(directory.path(), "https://provider.test/v1/", "test-key").unwrap();
    for reason in [
        ModelFallbackReason::Transport,
        ModelFallbackReason::Timeout,
        ModelFallbackReason::HttpStatus(503),
        ModelFallbackReason::InvalidResponse,
        ModelFallbackReason::NoModels,
    ] {
        let discovery = resolve(Some(&reopened), Err("live failure"), |_| Some(reason)).unwrap();
        assert_eq!(discovery.models[0].id, "qwen3.6");
        assert_eq!(
            discovery.source,
            ModelDiscoverySource::Cache {
                fetched_at_unix_seconds: 1,
                reason
            }
        );
        assert!(
            discovery
                .notice(1_000_000)
                .unwrap()
                .contains("999999 seconds")
        );
    }
    let live = resolve(
        Some(&cache),
        Ok::<_, ()>(models("deepseek-v4-flash")),
        |()| None,
    )
    .unwrap();
    assert_eq!(live.source, ModelDiscoverySource::Live);
    assert!(live.notice(0).is_none());
    assert_eq!(reopened.load().unwrap().0[0].id, "deepseek-v4-flash");
}

#[test]
fn credentials_full_base_url_and_local_salt_isolate_catalogs() {
    let directory = tempfile::tempdir().unwrap();
    let cache = ModelCache::open(directory.path(), "https://provider.test/v1", "test-key").unwrap();
    cache.save(&models("qwen3.6"), 1).unwrap();
    for (url, key) in [
        ("https://provider.test/v2", "test-key"),
        ("https://other.test/v1", "test-key"),
        ("https://provider.test/v1", "other-key"),
    ] {
        assert!(
            ModelCache::open(directory.path(), url, key)
                .unwrap()
                .load()
                .is_none()
        );
    }
    let other = tempfile::tempdir().unwrap();
    let other_cache =
        ModelCache::open(other.path(), "https://provider.test/v1", "test-key").unwrap();
    other_cache.save(&models("qwen3.6"), 1).unwrap();
    let name = |root: &std::path::Path| {
        fs::read_dir(root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .find(|name| name.to_string_lossy().ends_with(".json"))
            .unwrap()
    };
    assert_ne!(name(directory.path()), name(other.path()));
}

#[test]
fn missing_invalid_and_ineligible_caches_preserve_original_error() {
    let directory = tempfile::tempdir().unwrap();
    let cache = ModelCache::open(directory.path(), "https://provider.test/v1", "test-key").unwrap();
    assert_eq!(
        resolve(Some(&cache), Err(42), |_| Some(
            ModelFallbackReason::Transport
        ))
        .unwrap_err(),
        42
    );
    cache.save(&models("qwen3.6"), 1).unwrap();
    for status in [400, 401, 403, 404, 409, 422] {
        assert_eq!(
            resolve(Some(&cache), Err(status), |s| {
                ModelFallbackReason::from_status(*s)
            })
            .unwrap_err(),
            status
        );
    }
    for status in [408, 429, 500, 503, 599] {
        assert!(
            resolve(Some(&cache), Err(status), |s| {
                ModelFallbackReason::from_status(*s)
            })
            .is_ok()
        );
    }
    let path = fs::read_dir(directory.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|s| s == "json"))
        .unwrap();
    for payload in [
        b"broken".as_slice(),
        br#"{"schema_version":2,"fetched_at_unix_seconds":1,"ids":["qwen3.6"]}"#,
        br#"{"schema_version":1,"fetched_at_unix_seconds":1,"ids":[]}"#,
    ] {
        fs::write(&path, payload).unwrap();
        assert_eq!(
            resolve(Some(&cache), Err(42), |_| Some(
                ModelFallbackReason::InvalidResponse
            ))
            .unwrap_err(),
            42
        );
    }
    fs::write(&path, vec![b' '; 1024 * 1024 + 1]).unwrap();
    assert!(cache.load().is_none());
}

#[test]
fn unwritable_cache_does_not_block_live_result_or_replace_good_catalog_with_empty() {
    let directory = tempfile::tempdir().unwrap();
    let cache = ModelCache::open(directory.path(), "https://provider.test/v1", "test-key").unwrap();
    cache.save(&models("qwen3.6"), 1).unwrap();
    cache.save(&[], 2).unwrap();
    assert_eq!(cache.load().unwrap().1, 1);
    fs::remove_dir_all(directory.path()).unwrap();
    let discovery = resolve(Some(&cache), Ok::<_, ()>(models("qwen3.6")), |()| None).unwrap();
    assert_eq!(discovery.source, ModelDiscoverySource::Live);
    assert_eq!(
        resolve(None, Err(42), |_| Some(ModelFallbackReason::Timeout)).unwrap_err(),
        42
    );
}

#[test]
fn concurrent_first_use_and_publication_keep_complete_private_catalogs() {
    let directory = tempfile::tempdir().unwrap();
    let barrier = &std::sync::Barrier::new(8);
    std::thread::scope(|scope| {
        for index in 0..8 {
            let root = directory.path();
            scope.spawn(move || {
                barrier.wait();
                let cache = ModelCache::open(root, "https://provider.test/v1", "test-key").unwrap();
                cache
                    .save(
                        &models(if index % 2 == 0 {
                            "qwen3.6"
                        } else {
                            "deepseek-v4-flash"
                        }),
                        index,
                    )
                    .unwrap();
                assert_eq!(cache.load().unwrap().0.len(), 1);
            });
        }
    });
    let entries = fs::read_dir(directory.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 2);
    for path in entries {
        let bytes = fs::read(&path).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("test-key"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(directory.path()).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
    }
}
