use super::*;
use crate::catalog::versions::asar;
use std::collections::BTreeMap;
use std::io::Write as _;

const NATIVE: [(Platform, Architecture); 3] = [
    (Platform::Linux, Architecture::X86_64),
    (Platform::Macos, Architecture::Aarch64),
    (Platform::Windows, Architecture::X86_64),
];
const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
const TAG_OBJECT: &str = "fedcba9876543210fedcba9876543210fedcba98";

/// Official endpoints served from memory; every request is counted.
#[derive(Default)]
struct Upstream {
    documents: BTreeMap<String, Vec<u8>>,
    artifacts: BTreeMap<String, Vec<u8>>,
    requests: BTreeMap<String, usize>,
}

impl Fetch for Upstream {
    async fn metadata(&mut self, url: &str, limit: u64) -> Result<Vec<u8>, ()> {
        *self.requests.entry(url.into()).or_default() += 1;
        let body = self.documents.get(url).cloned().ok_or(())?;
        if body.len() as u64 > limit {
            return Err(());
        }
        Ok(body)
    }

    async fn artifact(&mut self, url: &str, destination: &Path) -> Result<(), ()> {
        *self.requests.entry(url.into()).or_default() += 1;
        let bytes = self.artifacts.get(url).ok_or(())?;
        let mut file = nan_harness_private_fs::open_private_new(destination).map_err(|_| ())?;
        file.write_all(bytes).map_err(|_| ())
    }

    fn inspect(
        &mut self,
        app: DesktopHarnessKind,
        format: PackageFormat,
        path: &Path,
    ) -> Result<Version, InspectError> {
        // Disk images need the platform mount tool; model its bounded Info.plist read.
        if format == PackageFormat::Dmg {
            let text = std::fs::read_to_string(path).map_err(|_| InspectError::Unresolved)?;
            return match text.strip_prefix("CFBundleShortVersionString=") {
                Some("mounted") => Err(InspectError::CleanupUncertain),
                Some(version) => exact_version(version).ok_or(InspectError::Unresolved),
                None => Err(InspectError::Unresolved),
            };
        }
        inspect::artifact_version(app, format, path)
    }
}

fn apt_index(package: &str, versions: &[&str], architecture: &str) -> Vec<u8> {
    versions
        .iter()
        .map(|version| {
            format!(
                "Package: {package}\nVersion: {version}\nArchitecture: {architecture}\nFilename: pool/main/{}/{package}/{package}_{version}_{architecture}.deb\nSHA256: {}\nDescription: synthetic\n continuation\n",
                &package[..1],
                "A".repeat(64)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
        .into_bytes()
}

fn appcast(versions: &[&str]) -> Vec<u8> {
    use std::fmt::Write as _;
    let items = versions
        .iter()
        .fold(String::new(), |mut items, version| {
            write!(items,
                r#"<item><title>{version}</title><sparkle:shortVersionString>{version}</sparkle:shortVersionString><sparkle:hardwareRequirements>arm64</sparkle:hardwareRequirements><enclosure url="https://persistent.oaistatic.com/codex-app-prod/ChatGPT-darwin-arm64-{version}.zip" length="1" /><sparkle:deltas><enclosure url="https://persistent.oaistatic.com/codex-app-prod/delta" /></sparkle:deltas></item>"#
            ).unwrap();
            items
        });
    format!(r#"<?xml version="1.0"?><rss><channel>{items}</channel></rss>"#).into_bytes()
}

fn github_release(tag: &str, assets: &[&str]) -> Vec<u8> {
    let assets = assets
        .iter()
        .map(|name| {
            serde_json::json!({
                "name": name,
                "browser_download_url": format!("https://github.com/zed-industries/zed/releases/download/{tag}/{name}"),
                "digest": format!("sha256:{}", "b".repeat(64)),
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_vec(&serde_json::json!({"tag_name": tag, "draft": false, "prerelease": false, "assets": assets})).unwrap()
}

/// A minimal stored or deflated ZIP with one `app/resources/app.asar` entry.
fn msix(version: &str, deflate: bool) -> Vec<u8> {
    let name = b"app/resources/app.asar";
    let content = asar::fixture(version);
    let data = if deflate {
        let mut encoder =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&content).unwrap();
        encoder.finish().unwrap()
    } else {
        content.clone()
    };
    let method: u16 = if deflate { 8 } else { 0 };
    let sizes = |bytes: &mut Vec<u8>| {
        bytes.extend(0u32.to_le_bytes());
        bytes.extend(u32::try_from(data.len()).unwrap().to_le_bytes());
        bytes.extend(u32::try_from(content.len()).unwrap().to_le_bytes());
    };
    let mut zip = Vec::new();
    zip.extend(0x0403_4b50u32.to_le_bytes());
    zip.extend([20, 0, 0, 0]);
    zip.extend(method.to_le_bytes());
    zip.extend([0, 0, 0, 0]);
    sizes(&mut zip);
    zip.extend(u16::try_from(name.len()).unwrap().to_le_bytes());
    zip.extend(0u16.to_le_bytes());
    zip.extend(name);
    zip.extend(&data);
    let directory = zip.len();
    zip.extend(0x0201_4b50u32.to_le_bytes());
    zip.extend([20, 0, 20, 0, 0, 0]);
    zip.extend(method.to_le_bytes());
    zip.extend([0, 0, 0, 0]);
    sizes(&mut zip);
    zip.extend(u16::try_from(name.len()).unwrap().to_le_bytes());
    zip.extend([0u8; 12]);
    zip.extend(0u32.to_le_bytes());
    zip.extend(name);
    let size = zip.len() - directory;
    zip.extend(0x0605_4b50u32.to_le_bytes());
    zip.extend([0, 0, 0, 0, 1, 0, 1, 0]);
    zip.extend(u32::try_from(size).unwrap().to_le_bytes());
    zip.extend(u32::try_from(directory).unwrap().to_le_bytes());
    zip.extend(0u16.to_le_bytes());
    zip
}

fn pen_tarball(version: &str) -> Vec<u8> {
    let content = asar::fixture(version);
    let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
        Vec::new(),
        flate2::Compression::fast(),
    ));
    let mut header = tar::Header::new_gnu();
    header.set_size(content.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder
        .append_data(
            &mut header,
            "Pen-linux-x64/resources/app.asar",
            content.as_slice(),
        )
        .unwrap();
    builder.into_inner().unwrap().finish().unwrap()
}

/// Every official source for the three native targets, at synthetic latest versions.
fn upstream() -> Upstream {
    let mut up = Upstream::default();
    let mut doc = |url: &str, body: Vec<u8>| {
        up.documents.insert(url.into(), body);
    };
    doc(
        "https://persistent.oaistatic.com/codex-app-prod/linux/deb/dists/stable/main/binary-amd64/Packages",
        apt_index("chatgpt", &["26.99.1", "26.908.40834", "26.100.0"], "amd64"),
    );
    doc(
        "https://downloads.claude.ai/claude-desktop/apt/stable/dists/stable/main/binary-amd64/Packages",
        apt_index("claude-desktop", &["1.17180.0", "1.17282.0"], "amd64"),
    );
    doc(
        "https://persistent.oaistatic.com/codex-app-prod/appcast.xml",
        appcast(&["26.908.40834", "26.907.1"]),
    );
    doc(
        "https://downloads.claude.ai/releases/darwin/universal/RELEASES.json",
        serde_json::to_vec(&serde_json::json!({"currentRelease":"1.52386.4","releases":[{"version":"1.52386.4","updateTo":{"version":"1.52386.4","url":format!("https://downloads.claude.ai/releases/darwin/universal/1.52386.4/Claude-{}.zip", "c".repeat(40))}}]})).unwrap(),
    );
    doc(
        "https://api.github.com/repos/zed-industries/zed/releases/latest",
        github_release(
            "v1.19.2",
            &[
                "Zed-aarch64.dmg",
                "zed-linux-x86_64.tar.gz",
                "Zed-x86_64.exe",
                "zed-remote-server-linux-x86_64.gz",
            ],
        ),
    );
    doc(
        "https://api.github.com/repos/NousResearch/hermes-agent/releases/latest",
        br#"{"tag_name":"v2026.9.11","draft":false,"prerelease":false}"#.to_vec(),
    );
    doc(
        "https://api.github.com/repos/NousResearch/hermes-agent/git/ref/tags/v2026.9.11",
        format!(r#"{{"object":{{"type":"tag","sha":"{TAG_OBJECT}"}}}}"#).into_bytes(),
    );
    doc(
        &format!("https://api.github.com/repos/NousResearch/hermes-agent/git/tags/{TAG_OBJECT}"),
        format!(r#"{{"object":{{"type":"commit","sha":"{COMMIT}"}}}}"#).into_bytes(),
    );
    doc(
        &format!(
            "https://raw.githubusercontent.com/NousResearch/hermes-agent/{COMMIT}/apps/desktop/package.json"
        ),
        br#"{"name":"hermes","version":"0.17.2"}"#.to_vec(),
    );
    let mut artifact = |url: &str, bytes: Vec<u8>| {
        up.artifacts.insert(url.into(), bytes);
    };
    artifact(
        "https://persistent.oaistatic.com/codex-app-prod/ChatGPT-darwin-arm64-26.908.40834.zip",
        b"chatgpt zip".to_vec(),
    );
    artifact(
        &format!(
            "https://downloads.claude.ai/releases/darwin/universal/1.52386.4/Claude-{}.zip",
            "c".repeat(40)
        ),
        b"claude zip".to_vec(),
    );
    artifact(
        "https://persistent.oaistatic.com/codex-app-prod/ChatGPT-x64.msix",
        msix("26.908.40834", true),
    );
    artifact(
        "https://claude.ai/api/desktop/win32/x64/msix",
        msix("1.52386.4", false),
    );
    artifact(
        "https://www.pen.dev/download/Pen-mac-arm64.dmg",
        b"CFBundleShortVersionString=1.4.0".to_vec(),
    );
    artifact(
        "https://www.pen.dev/download/Pen-linux-x64.tar.gz",
        pen_tarball("1.4.0"),
    );
    artifact(
        "https://www.pen.dev/download/Pen-win-x64.exe",
        inspect::pe::fixture("1.4.0"),
    );
    up
}

async fn resolve_cell(
    up: &mut Upstream,
    platform: Platform,
    architecture: Architecture,
) -> (Manifest, tempfile::TempDir) {
    let artifacts = tempfile::tempdir().unwrap();
    let manifest = resolve(
        &DesktopHarnessKind::ALL,
        platform,
        architecture,
        "qwen3.6",
        artifacts.path(),
        up,
    )
    .await
    .unwrap();
    (manifest, artifacts)
}

fn frozen(manifest: &Manifest, app: DesktopHarnessKind) -> &Release {
    match manifest.entry(app) {
        Some(Entry::Frozen(release)) => release,
        other => panic!("{app} was not frozen: {other:?}"),
    }
}

#[tokio::test]
async fn all_fifteen_native_pairs_freeze_exact_official_releases() {
    let expected = [
        ("26.908.40834", "1.17282.0", "0.17.2", "1.4.0", "1.19.2"),
        ("26.908.40834", "1.52386.4", "0.17.2", "1.4.0", "1.19.2"),
        ("26.908.40834", "1.52386.4", "0.17.2", "1.4.0", "1.19.2"),
    ];
    for ((platform, architecture), versions) in NATIVE.into_iter().zip(expected) {
        let mut up = upstream();
        let (manifest, artifacts) = resolve_cell(&mut up, platform, architecture).await;
        manifest
            .verify(&DesktopHarnessKind::ALL, platform, architecture, "qwen3.6")
            .unwrap();
        let actual = (
            frozen(&manifest, DesktopHarnessKind::ChatGpt)
                .version
                .as_str(),
            frozen(&manifest, DesktopHarnessKind::Claude)
                .version
                .as_str(),
            frozen(&manifest, DesktopHarnessKind::Hermes)
                .version
                .as_str(),
            frozen(&manifest, DesktopHarnessKind::Pen).version.as_str(),
            frozen(&manifest, DesktopHarnessKind::Zed).version.as_str(),
        );
        assert_eq!(actual, versions, "{platform:?}");
        for entry in &manifest.apps {
            let Entry::Frozen(release) = entry else {
                unreachable!()
            };
            assert!(release.url.starts_with("https://"));
            assert!(release.runtime_version.is_none());
            match release.format {
                PackageFormat::Source => {
                    assert_eq!(release.revision.as_deref(), Some(COMMIT));
                    assert!(release.digest.is_none());
                }
                _ => assert!(release.digest.as_deref().is_some_and(sha256_digest)),
            }
            if release.staged {
                let staged = staged_path(artifacts.path(), release).unwrap();
                let digest = crate::install::sha256_file(&staged).unwrap();
                assert_eq!(
                    release.digest.as_deref(),
                    Some(format!("sha256:{digest}").as_str())
                );
            }
        }
        // Resolution reads each official source exactly once.
        assert!(
            up.requests.values().all(|count| *count == 1),
            "{:?}",
            up.requests
        );
        let encoded = serde_json::to_vec(&manifest).unwrap();
        let decoded: Manifest = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, manifest);
    }
}

#[tokio::test]
async fn one_failed_source_does_not_erase_independent_apps() {
    let mut up = upstream();
    up.documents
        .remove("https://api.github.com/repos/zed-industries/zed/releases/latest");
    let (manifest, _artifacts) = resolve_cell(&mut up, Platform::Linux, Architecture::X86_64).await;
    assert_eq!(
        manifest.entry(DesktopHarnessKind::Zed),
        Some(&Entry::Blocked(Blocker {
            app: DesktopHarnessKind::Zed,
            reason: BlockReason::ResolutionFailed,
            evidence: "https://github.com/zed-industries/zed/releases/latest".into(),
        }))
    );
    assert_eq!(frozen(&manifest, DesktopHarnessKind::Pen).version, "1.4.0");
    manifest
        .verify(
            &DesktopHarnessKind::ALL,
            Platform::Linux,
            Architecture::X86_64,
            "qwen3.6",
        )
        .unwrap();
}

#[tokio::test]
async fn ambiguous_or_four_component_versions_are_never_frozen() {
    let mut up = upstream();
    up.artifacts.insert(
        "https://www.pen.dev/download/Pen-win-x64.exe".into(),
        inspect::pe::fixture("1.4.0.0"),
    );
    up.artifacts.insert(
        "https://persistent.oaistatic.com/codex-app-prod/ChatGPT-x64.msix".into(),
        msix("26.908.40834.0", false),
    );
    up.documents.insert(
        "https://api.github.com/repos/zed-industries/zed/releases/latest".into(),
        github_release("v1.19.2-pre", &["Zed-x86_64.exe"]),
    );
    let artifacts = tempfile::tempdir().unwrap();
    let manifest = resolve(
        &DesktopHarnessKind::ALL,
        Platform::Windows,
        Architecture::X86_64,
        "qwen3.6",
        artifacts.path(),
        &mut up,
    )
    .await
    .unwrap();
    for app in [
        DesktopHarnessKind::ChatGpt,
        DesktopHarnessKind::Pen,
        DesktopHarnessKind::Zed,
    ] {
        assert!(matches!(
            manifest.entry(app),
            Some(Entry::Blocked(Blocker {
                reason: BlockReason::ResolutionFailed,
                ..
            }))
        ));
    }
    assert_eq!(
        frozen(&manifest, DesktopHarnessKind::Claude).version,
        "1.52386.4"
    );
    // Unresolved staged bytes are removed rather than left for installation.
    let left = std::fs::read_dir(artifacts.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(left.len(), 1, "{left:?}");
    assert!(left[0].starts_with("claude-desktop-"));
    for text in [
        "1.2.3.4",
        "01.2.3",
        "1.2",
        "v1.2.3",
        "1.2.3-rc.1",
        "1.2.3+build",
    ] {
        assert!(exact_version(text).is_none(), "{text}");
    }
}

#[tokio::test]
async fn cleanup_uncertainty_stops_successor_apps() {
    let mut up = upstream();
    up.artifacts.insert(
        "https://www.pen.dev/download/Pen-mac-arm64.dmg".into(),
        b"CFBundleShortVersionString=mounted".to_vec(),
    );
    let artifacts = tempfile::tempdir().unwrap();
    let result = resolve(
        &DesktopHarnessKind::ALL,
        Platform::Macos,
        Architecture::Aarch64,
        "qwen3.6",
        artifacts.path(),
        &mut up,
    )
    .await;
    assert_eq!(result, Err(ResolveError::CleanupUncertain));
    assert!(
        !up.requests
            .contains_key("https://api.github.com/repos/zed-industries/zed/releases/latest")
    );
}

#[tokio::test]
async fn drifted_or_tampered_manifests_are_rejected() {
    let mut up = upstream();
    let (manifest, _artifacts) = resolve_cell(&mut up, Platform::Linux, Architecture::X86_64).await;
    let verify = |manifest: &Manifest| {
        manifest.verify(
            &DesktopHarnessKind::ALL,
            Platform::Linux,
            Architecture::X86_64,
            "qwen3.6",
        )
    };
    let edit = |change: &dyn Fn(&mut Release)| {
        let mut copy = manifest.clone();
        for entry in &mut copy.apps {
            if let Entry::Frozen(release) = entry {
                change(release);
            }
        }
        copy
    };
    let tampered: [&dyn Fn(&mut Release); 9] = [
        &|release| release.url = release.url.replace("https://", "https://mirror.example/"),
        &|release| release.version = format!("{}.0", release.version),
        &|release| release.channel = "github-release:community/wrapper".into(),
        &|release| release.digest = None,
        &|release| release.staged = !release.staged,
        &|release| release.installer = Installer::External,
        &|release| release.format = PackageFormat::Zip,
        &|release| release.revision = Some("a".repeat(40)),
        &|release| release.runtime_version = Some("0.1".into()),
    ];
    for change in tampered {
        assert!(verify(&edit(change)).is_err());
    }
    assert!(
        verify(&edit(&|release| {
            release.runtime_version = Some("0.154.0-alpha.6.2".into());
        }))
        .is_ok()
    );
    // A version change alone breaks the exact versioned URL for immutable channels.
    let mut drift = manifest.clone();
    if let Some(Entry::Frozen(release)) = drift
        .apps
        .iter_mut()
        .find(|entry| entry.app() == DesktopHarnessKind::Zed)
    {
        release.version = "1.19.3".into();
    }
    assert_eq!(verify(&drift), Err(ManifestError::Untrusted));
    for (apps, model, platform) in [
        (&DesktopHarnessKind::ALL[..4], "qwen3.6", Platform::Linux),
        (&DesktopHarnessKind::ALL[..], "other-model", Platform::Linux),
        (&DesktopHarnessKind::ALL[..], "qwen3.6", Platform::Windows),
    ] {
        assert!(
            manifest
                .verify(apps, platform, Architecture::X86_64, model)
                .is_err()
        );
    }
    let mut blocked = manifest.clone();
    blocked.apps[0] = Entry::Blocked(Blocker {
        app: DesktopHarnessKind::ChatGpt,
        reason: BlockReason::UpstreamUnsupported,
        evidence: "https://learn.chatgpt.com/docs/app".into(),
    });
    assert_eq!(verify(&blocked), Err(ManifestError::Untrusted));
    let unknown = serde_json::to_string(&manifest).unwrap().replacen(
        "\"staged\"",
        "\"extra\":1,\"staged\"",
        1,
    );
    assert!(serde_json::from_str::<Manifest>(&unknown).is_err());
}

#[tokio::test]
async fn unsupported_official_pairs_are_closed_blockers_without_versions() {
    let mut up = upstream();
    let artifacts = tempfile::tempdir().unwrap();
    let manifest = resolve(
        &[DesktopHarnessKind::ChatGpt, DesktopHarnessKind::Pen],
        Platform::Windows,
        Architecture::Aarch64,
        "model",
        artifacts.path(),
        &mut up,
    )
    .await
    .unwrap();
    assert!(
        manifest
            .apps
            .iter()
            .all(|entry| matches!(entry, Entry::Blocked(_)))
    );
    assert!(up.requests.is_empty());
    let encoded = serde_json::to_value(&manifest).unwrap();
    assert!(
        encoded["apps"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry.get("version").is_none())
    );
    let macos = resolve(
        &[DesktopHarnessKind::ChatGpt],
        Platform::Macos,
        Architecture::X86_64,
        "model",
        artifacts.path(),
        &mut up,
    )
    .await
    .unwrap();
    assert_eq!(
        macos.apps,
        [Entry::Blocked(Blocker {
            app: DesktopHarnessKind::ChatGpt,
            reason: BlockReason::UpstreamUnsupported,
            evidence: "https://learn.chatgpt.com/docs/app".into(),
        })]
    );
    assert!(
        resolve(
            &[],
            Platform::Linux,
            Architecture::X86_64,
            "bad model",
            artifacts.path(),
            &mut up
        )
        .await
        .is_err()
    );
}

#[test]
fn metadata_parsers_reject_ambiguous_or_untrusted_records() {
    let prefix = "https://persistent.oaistatic.com/codex-app-prod/ChatGPT-darwin-arm64-";
    let duplicated = appcast(&["26.908.40834", "26.908.40834"]);
    assert!(
        resolve::newest_sparkle(std::str::from_utf8(&duplicated).unwrap(), prefix, "arm64")
            .is_none()
    );
    let foreign = String::from_utf8(appcast(&["26.908.40834"]))
        .unwrap()
        .replace(prefix, "https://example.com/ChatGPT-");
    assert!(resolve::newest_sparkle(&foreign, prefix, "arm64").is_none());
    let intel = String::from_utf8(appcast(&["26.908.40834"]))
        .unwrap()
        .replace(">arm64<", ">x86_64<");
    assert!(resolve::newest_sparkle(&intel, prefix, "arm64").is_none());

    let index = String::from_utf8(apt_index("claude-desktop", &["1.0.0"], "amd64")).unwrap();
    for invalid in [
        format!("{index}\n{index}"),
        index.replace(&"A".repeat(64), "bad"),
        index.replace("pool/main/c/", "../"),
        index.replace("Version: 1.0.0", "Version: 1.0.0.1"),
    ] {
        assert!(resolve::newest_apt(&invalid, "claude-desktop", "amd64").is_none());
    }
    assert!(resolve::newest_apt(&index, "claude-desktop", "arm64").is_none());

    let release = github_release("v1.19.2", &["Zed-x86_64.exe", "Zed-x86_64.exe"]);
    assert!(resolve::github_asset(&release, "zed-industries/zed", "Zed-x86_64.exe").is_none());
    let prerelease = String::from_utf8(github_release("v1.19.2", &["Zed-x86_64.exe"]))
        .unwrap()
        .replace("\"prerelease\":false", "\"prerelease\":true");
    assert!(
        resolve::github_asset(
            prerelease.as_bytes(),
            "zed-industries/zed",
            "Zed-x86_64.exe"
        )
        .is_none()
    );

    let archive = "https://downloads.claude.ai/releases/darwin/universal/";
    let mismatch = serde_json::to_vec(&serde_json::json!({"currentRelease":"1.2.3","releases":[{"version":"1.2.3","updateTo":{"version":"1.2.4","url":format!("{archive}1.2.3/Claude-{}.zip", "c".repeat(40))}}]})).unwrap();
    assert!(resolve::squirrel_mac(&mismatch, archive).is_none());
}

#[test]
fn staged_artifact_metadata_is_bounded_and_single() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("package");
    std::fs::write(&path, msix("2.0.1", true)).unwrap();
    assert_eq!(
        inspect::msix_package_version(&path).as_deref(),
        Some("2.0.1")
    );
    let mut traversal = msix("2.0.1", false);
    let position = traversal
        .windows(3)
        .rposition(|window| window == b"app")
        .unwrap();
    traversal[position..position + 3].copy_from_slice(b"../");
    std::fs::write(&path, traversal).unwrap();
    assert!(inspect::msix_package_version(&path).is_none());
    std::fs::write(&path, b"not a zip").unwrap();
    assert!(inspect::msix_package_version(&path).is_none());
    assert_eq!(
        inspect::tar_package_version(pen_tarball("3.1.4").as_slice()).as_deref(),
        Some("3.1.4")
    );
    std::fs::write(&path, inspect::pe::fixture("9.8.7")).unwrap();
    assert_eq!(
        inspect::pe::product_version(&path).as_deref(),
        Some("9.8.7")
    );
    let mut truncated = inspect::pe::fixture("9.8.7");
    truncated.truncate(0x220);
    std::fs::write(&path, truncated).unwrap();
    assert!(inspect::pe::product_version(&path).is_none());
}
