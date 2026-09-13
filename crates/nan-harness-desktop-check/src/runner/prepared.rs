//! Private installation receipts bridge credential-free preparation and GUI checks.

use super::*;
use crate::catalog::frozen::{self, BlockReason, Entry, Installer, Manifest};
use serde::Serialize;

pub(super) type Inventory = Vec<(DesktopHarnessKind, Result<Option<Installation>, Reason>)>;

const RECEIPT_SCHEMA: u8 = 2;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Receipt {
    schema_version: u8,
    run_id: String,
    platform: Platform,
    architecture: Architecture,
    checker: Executable,
    nanh: Executable,
    nanh_identity: BinaryIdentity,
    /// Binds the exact frozen manifest bytes and model; absent for discovery-only preparation.
    frozen: Option<FrozenBinding>,
    apps: Vec<PreparedApp>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FrozenBinding {
    sha256: String,
    model: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Executable {
    path: PathBuf,
    sha256: String,
}

impl Executable {
    fn record(path: &Path) -> Result<Self, String> {
        let path = std::fs::canonicalize(path).map_err(|_| "prepared executable cannot be read")?;
        let sha256 = crate::probe::binary_digest(&path)
            .map_err(|_| "prepared executable cannot be identified")?;
        Ok(Self { path, sha256 })
    }

    fn verify(&self) -> Result<(), String> {
        if !self.path.is_absolute() || Self::record(&self.path)?.sha256 != self.sha256 {
            return Err("a prepared executable changed; prepare again without credentials".into());
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PreparedApp {
    app: DesktopHarnessKind,
    executable: Option<Executable>,
    app_version: Option<Version>,
    runtime_version: Option<Version>,
    /// Closed reason when this app cannot run; independent apps keep their evidence.
    blocked: Option<Reason>,
}

pub(super) struct Prepared {
    pub inventory: Inventory,
    pub nanh: (PathBuf, BinaryIdentity),
    pub owner: Journal,
}

pub(super) fn discovery_reason(error: DiscoveryError) -> Reason {
    match error {
        DiscoveryError::Ambiguous => Reason::InstallationAmbiguous,
        DiscoveryError::Unsupported => Reason::InstallationUnavailable,
        DiscoveryError::Unreadable
        | DiscoveryError::Incomplete
        | DiscoveryError::RootEnumeration
        | DiscoveryError::CandidateMetadata
        | DiscoveryError::CandidateCanonicalization
        | DiscoveryError::CandidateRead
        | DiscoveryError::VersionResource
        | DiscoveryError::Architecture => Reason::InstallationUnreadable,
    }
}

fn discovery_diagnostic(app: DesktopHarnessKind, error: DiscoveryError, reason: Reason) {
    catalog::diagnostic::emit(catalog::diagnostic::Event {
        schema_version: 1,
        app,
        stage: catalog::diagnostic::stage(error),
        error_category: catalog::diagnostic::category(error),
        reason,
        os_error: None,
    });
}

/// Outcome of preparing one app; `Abort` means cleanup state is uncertain.
enum Prepare {
    Ready(Installation),
    Blocked(Reason),
    Abort,
}

pub(crate) async fn prepare(args: RunArgs) -> Result<i32, String> {
    if std::env::var_os("NAN_API_KEY").is_some() {
        return Err("remove NAN_API_KEY before preparing installations".into());
    }
    if args.prepared.is_some() || args.mode != ExecutionMode::Auto {
        return Err("prepare does not accept --prepared or --mode".into());
    }
    if args.launch_wrapper.is_some() {
        return Err("prepare never launches applications; omit --launch-wrapper".into());
    }
    if args.artifacts.is_some() && args.frozen.is_none() {
        return Err("--artifacts requires --frozen".into());
    }
    let output = args
        .output
        .as_ref()
        .ok_or("prepare requires --output for its private receipt")?;
    if output.exists() {
        return Err("preparation output already exists".into());
    }
    let apps = if args.apps.is_empty() {
        DesktopHarnessKind::ALL.to_vec()
    } else {
        args.apps.clone()
    };
    let apps = apps
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let frozen = args
        .frozen
        .as_deref()
        .map(|path| Manifest::read(path, &apps, &args.model))
        .transpose()
        .map_err(|error| error.to_string())?;
    let discovered = apps
        .iter()
        .map(|&app| {
            let found = catalog::discover(app).map_err(|error| {
                let reason = discovery_reason(error);
                discovery_diagnostic(app, error, reason);
                reason
            });
            (app, found)
        })
        .collect::<Vec<_>>();
    let existing = discover_nanh(args.nan_harness.as_deref()).await?;
    print_inventory(&discovered, existing.as_ref(), false, true);
    if !args.yes
        && (args.non_interactive || !confirm("Prepare these installations without opening them?")?)
    {
        return Ok(0);
    }
    let mut journal = Journal::create(&state_directory()?).map_err(|error| error.to_string())?;
    eprintln!(
        "Recover prepared installations with: nanh-desktop-check cleanup {}",
        journal.run_id()
    );
    let context = Context {
        frozen: frozen.as_ref(),
        artifacts: args.artifacts.as_deref(),
        model: &args.model,
    };
    let outcome = prepare_receipt(discovered, existing, &context, &mut journal).await;
    let receipt = match outcome {
        Ok(receipt) => receipt,
        Err(error) => {
            if journal.cleanup(false).is_err() {
                eprintln!("Cleanup needs attention; keep the private recovery receipt.");
            }
            return Err(error);
        }
    };
    let bytes = serde_json::to_vec(&receipt).map_err(|_| "cannot encode preparation receipt")?;
    open_private_new(output)
        .and_then(|mut file| file.write_all(&bytes).and_then(|()| file.sync_all()))
        .map_err(|_| "cannot save preparation receipt; recovery state retained")?;
    println!("Private preparation receipt: {}", output.display());
    // Blocked apps are recorded, not fatal: the run reports them beside independent evidence.
    Ok(0)
}

struct Context<'a> {
    frozen: Option<&'a (Manifest, String)>,
    artifacts: Option<&'a Path>,
    model: &'a str,
}

async fn prepare_receipt(
    discovered: Inventory,
    existing: Option<(PathBuf, BinaryIdentity)>,
    context: &Context<'_>,
    journal: &mut Journal,
) -> Result<Receipt, String> {
    let (nanh, nanh_identity) = match existing {
        Some(existing) => existing,
        None => install_nanh(journal).await?,
    };
    let mut apps = Vec::new();
    for (app, found) in discovered {
        let outcome = match context.frozen {
            Some((manifest, _)) => {
                prepare_frozen(app, found, manifest, context.artifacts, journal).await
            }
            None => prepare_latest(app, found, journal).await,
        };
        let prepared = match outcome {
            Prepare::Ready(installed) => PreparedApp {
                app,
                executable: Some(Executable::record(&installed.executable)?),
                app_version: installed.app_version,
                runtime_version: installed.runtime_version,
                blocked: None,
            },
            Prepare::Blocked(reason) => {
                eprintln!("  {app}: not prepared ({})", guidance(reason));
                PreparedApp {
                    app,
                    executable: None,
                    app_version: None,
                    runtime_version: None,
                    blocked: Some(reason),
                }
            }
            Prepare::Abort => {
                return Err(
                    "an installer volume may still be mounted; later apps were not prepared".into(),
                );
            }
        };
        apps.push(prepared);
    }
    Ok(Receipt {
        schema_version: RECEIPT_SCHEMA,
        run_id: journal.run_id().into(),
        platform: Platform::current(),
        architecture: Architecture::current(),
        checker: Executable::record(
            &std::env::current_exe().map_err(|_| "cannot identify checker")?,
        )?,
        nanh: Executable::record(&nanh)?,
        nanh_identity,
        frozen: context.frozen.map(|(_, sha256)| FrozenBinding {
            sha256: sha256.clone(),
            model: context.model.into(),
        }),
        apps,
    })
}

fn install_reason(error: &install::InstallError) -> Prepare {
    use install::InstallError as E;
    Prepare::Blocked(match error {
        E::MountPending => return Prepare::Abort,
        E::ExternalInstallation | E::Unavailable => Reason::InstallationUnavailable,
        E::VersionMismatch => Reason::UnsupportedVersion,
        E::Discovery(DiscoveryError::Ambiguous) => Reason::InstallationAmbiguous,
        _ => Reason::InstallationFailed,
    })
}

fn install_diagnostic(app: DesktopHarnessKind, error: &install::InstallError, reason: Reason) {
    use install::InstallError as E;
    let (stage, error_category) = match error {
        E::VersionMismatch => (
            catalog::diagnostic::Stage::VersionResource,
            catalog::diagnostic::ErrorCategory::VersionMismatch,
        ),
        E::Discovery(discovery) => (
            catalog::diagnostic::stage(*discovery),
            catalog::diagnostic::category(*discovery),
        ),
        E::Unavailable | E::ExternalInstallation => (
            catalog::diagnostic::Stage::FrozenResolution,
            catalog::diagnostic::ErrorCategory::Unsupported,
        ),
        _ => (
            catalog::diagnostic::Stage::Installation,
            catalog::diagnostic::ErrorCategory::InstallationFailed,
        ),
    };
    catalog::diagnostic::emit(catalog::diagnostic::Event {
        schema_version: 1,
        app,
        stage,
        error_category,
        reason,
        os_error: None,
    });
}

fn frozen_blocker_diagnostic(app: DesktopHarnessKind, blocker: BlockReason) {
    let (error_category, reason) = match blocker {
        BlockReason::ResolutionFailed => (
            catalog::diagnostic::ErrorCategory::ResolutionFailed,
            Reason::VersionUnknown,
        ),
        BlockReason::UpstreamUnsupported => (
            catalog::diagnostic::ErrorCategory::UpstreamUnsupported,
            Reason::InstallationUnavailable,
        ),
        BlockReason::UnqualifiedPlatform => (
            catalog::diagnostic::ErrorCategory::UnqualifiedPlatform,
            Reason::InstallationUnavailable,
        ),
    };
    catalog::diagnostic::emit(catalog::diagnostic::Event {
        schema_version: 1,
        app,
        stage: catalog::diagnostic::Stage::FrozenResolution,
        error_category,
        reason,
        os_error: None,
    });
}

/// Legacy local preparation: resolve and freeze internally, then install once.
async fn prepare_latest(
    app: DesktopHarnessKind,
    found: Result<Option<Installation>, Reason>,
    journal: &mut Journal,
) -> Prepare {
    match found {
        Ok(Some(installed)) => Prepare::Ready(installed),
        Err(reason) => Prepare::Blocked(reason),
        Ok(None) => match install::install(app, journal).await {
            Ok(installed) => Prepare::Ready(installed),
            Err(error) => {
                let outcome = install_reason(&error);
                let reason = match outcome {
                    Prepare::Blocked(reason) => reason,
                    Prepare::Ready(_) | Prepare::Abort => Reason::InstallationFailed,
                };
                install_diagnostic(app, &error, reason);
                outcome
            }
        },
    }
}

/// Install or accept only the exact frozen release; never rediscover latest.
async fn prepare_frozen(
    app: DesktopHarnessKind,
    found: Result<Option<Installation>, Reason>,
    manifest: &Manifest,
    artifacts: Option<&Path>,
    journal: &mut Journal,
) -> Prepare {
    let release = match manifest.entry(app) {
        Some(Entry::Frozen(release)) => release,
        Some(Entry::Blocked(blocker)) => {
            frozen_blocker_diagnostic(app, blocker.reason);
            return Prepare::Blocked(match blocker.reason {
                BlockReason::ResolutionFailed => Reason::VersionUnknown,
                BlockReason::UpstreamUnsupported | BlockReason::UnqualifiedPlatform => {
                    Reason::InstallationUnavailable
                }
            });
        }
        None => return Prepare::Blocked(Reason::InstallationUnavailable),
    };
    let installed = match (found, release.installer) {
        (Err(reason), _) => return Prepare::Blocked(reason),
        // An external step must already have installed exactly this release.
        (Ok(None), Installer::External) => return Prepare::Blocked(Reason::InstallationFailed),
        (Ok(Some(installed)), _) => installed,
        (Ok(None), Installer::Checker) => {
            match install::install_frozen(release, artifacts, journal).await {
                Ok(installed) => installed,
                Err(error) => {
                    let outcome = install_reason(&error);
                    let reason = match outcome {
                        Prepare::Blocked(reason) => reason,
                        Prepare::Ready(_) | Prepare::Abort => Reason::InstallationFailed,
                    };
                    install_diagnostic(app, &error, reason);
                    return outcome;
                }
            }
        }
    };
    if install::verify_version(release, &installed).is_ok() {
        Prepare::Ready(installed)
    } else {
        catalog::diagnostic::emit(catalog::diagnostic::Event {
            schema_version: 1,
            app,
            stage: catalog::diagnostic::Stage::VersionResource,
            error_category: catalog::diagnostic::ErrorCategory::VersionMismatch,
            reason: Reason::UnsupportedVersion,
            os_error: None,
        });
        Prepare::Blocked(Reason::UnsupportedVersion)
    }
}

pub(super) fn load(
    path: &Path,
    apps: &[DesktopHarnessKind],
    model: &str,
) -> Result<Prepared, String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(65537).read_to_end(&mut bytes))
        .map_err(|_| "cannot read private preparation receipt")?;
    if bytes.len() > 65536 {
        return Err("preparation receipt is too large".into());
    }
    let receipt: Receipt =
        serde_json::from_slice(&bytes).map_err(|_| "invalid preparation receipt")?;
    if receipt.schema_version != RECEIPT_SCHEMA
        || receipt.platform != Platform::current()
        || receipt.architecture != Architecture::current()
    {
        return Err("preparation does not match the selected apps and platform".into());
    }
    if let Some(binding) = &receipt.frozen
        && (binding.model != model || !frozen::sha256_hex(&binding.sha256))
    {
        return Err("the model differs from the frozen preparation; no substitute is used".into());
    }
    let owner =
        Journal::open(&state_directory()?, &receipt.run_id).map_err(|error| error.to_string())?;
    let current = std::env::current_exe().map_err(|_| "cannot identify checker")?;
    if Executable::record(&current)?.sha256 != receipt.checker.sha256 {
        return Err("the checker changed after preparation".into());
    }
    receipt.checker.verify()?;
    receipt.nanh.verify()?;
    if receipt.nanh.sha256 != receipt.nanh_identity.sha256 {
        return Err("prepared nanh identity is inconsistent".into());
    }
    let inventory = selected_inventory(receipt.apps, apps, receipt.frozen.is_some())?;
    Ok(Prepared {
        inventory,
        nanh: (receipt.nanh.path, receipt.nanh_identity),
        owner,
    })
}

/// A later stage may select fewer prepared apps, but cannot add an identity or
/// bypass validation of the receipt's other entries and executable bindings.
fn selected_inventory(
    prepared: Vec<PreparedApp>,
    apps: &[DesktopHarnessKind],
    frozen: bool,
) -> Result<Inventory, String> {
    let available = prepared.iter().map(|app| app.app).collect::<BTreeSet<_>>();
    let selected = apps.iter().copied().collect::<BTreeSet<_>>();
    if selected.is_empty()
        || selected.len() != apps.len()
        || available.len() != prepared.len()
        || !selected.is_subset(&available)
    {
        return Err("preparation does not contain the exact selected app identities".into());
    }
    let mut inventory = Vec::new();
    for app in prepared {
        let kind = app.app;
        let found = prepared_app(app, frozen)?;
        if selected.contains(&kind) {
            inventory.push((kind, found));
        }
    }
    Ok(inventory)
}

fn prepared_app(
    app: PreparedApp,
    frozen: bool,
) -> Result<Result<Option<Installation>, Reason>, String> {
    const INCONSISTENT: &str = "prepared application identity is inconsistent";
    match (app.executable, app.blocked) {
        (None, Some(reason)) if app.app_version.is_none() && app.runtime_version.is_none() => {
            Ok(Err(reason))
        }
        (Some(executable), None) if !frozen || app.app_version.is_some() => {
            executable.verify()?;
            Ok(Ok(Some(Installation {
                executable: executable.path,
                app_version: app.app_version,
                runtime_version: app.runtime_version,
            })))
        }
        _ => Err(INCONSISTENT.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepared_executable_changes_are_rejected_before_execution() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("synthetic-executable");
        std::fs::write(&path, b"original synthetic executable").unwrap();
        let recorded = Executable::record(&path).unwrap();
        recorded.verify().unwrap();
        std::fs::write(&path, b"substituted synthetic executable").unwrap();
        assert!(recorded.verify().is_err());
    }

    #[test]
    fn missing_and_oversized_receipts_fail_without_installing() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("receipt.json");
        assert!(load(&path, &[], "qwen3.6").is_err());
        std::fs::write(&path, vec![b' '; 65537]).unwrap();
        assert!(load(&path, &[], "qwen3.6").is_err());
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    fn app(
        executable: Option<Executable>,
        version: Option<&str>,
        blocked: Option<Reason>,
    ) -> PreparedApp {
        PreparedApp {
            app: DesktopHarnessKind::Zed,
            executable,
            app_version: version.map(|version| version.parse().unwrap()),
            runtime_version: None,
            blocked,
        }
    }

    #[test]
    fn a_later_stage_can_select_only_verified_prepared_apps() {
        use DesktopHarnessKind::{ChatGpt, Zed};
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("synthetic-executable");
        std::fs::write(&path, b"synthetic").unwrap();
        let entries = || {
            let ready = app(
                Some(Executable::record(&path).unwrap()),
                Some("1.19.2"),
                None,
            );
            let mut blocked = app(None, None, Some(Reason::InstallationFailed));
            blocked.app = ChatGpt;
            vec![ready, blocked]
        };
        let selected = selected_inventory(entries(), &[Zed], true).unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].0, Zed);
        assert!(matches!(selected[0].1, Ok(Some(_))));
        for requested in [vec![], vec![Zed, Zed], vec![DesktopHarnessKind::Pen]] {
            assert!(selected_inventory(entries(), &requested, true).is_err());
        }
        let mut duplicate = entries();
        duplicate[1].app = Zed;
        assert!(selected_inventory(duplicate, &[Zed], true).is_err());
        let mut inconsistent = entries();
        inconsistent[1].app_version = Some("1.0.0".parse().unwrap());
        assert!(selected_inventory(inconsistent, &[Zed], true).is_err());
        let changed = entries();
        std::fs::write(&path, b"changed").unwrap();
        // Even an excluded executable remains part of the verified receipt.
        assert!(selected_inventory(changed, &[ChatGpt], true).is_err());
    }

    #[test]
    fn receipt_apps_are_either_exactly_ready_or_closed_blockers() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("synthetic-executable");
        std::fs::write(&path, b"synthetic").unwrap();
        let record = || Some(Executable::record(&path).unwrap());
        assert!(matches!(
            prepared_app(app(None, None, Some(Reason::UnsupportedVersion)), true),
            Ok(Err(Reason::UnsupportedVersion))
        ));
        assert!(matches!(
            prepared_app(app(record(), Some("1.19.2"), None), true),
            Ok(Ok(Some(_)))
        ));
        assert!(matches!(
            prepared_app(app(record(), None, None), false),
            Ok(Ok(Some(_)))
        ));
        for inconsistent in [
            app(record(), None, None),
            app(record(), Some("1.19.2"), Some(Reason::InstallationFailed)),
            app(None, Some("1.19.2"), Some(Reason::InstallationFailed)),
            app(None, None, None),
        ] {
            assert!(prepared_app(inconsistent, true).is_err());
        }
    }

    fn frozen_manifest(installer: Installer) -> Manifest {
        Manifest {
            schema_version: 1,
            suite: "desktop".into(),
            platform: Platform::current(),
            architecture: Architecture::current(),
            model: "qwen3.6".into(),
            apps: vec![Entry::Frozen(frozen::Release {
                app: DesktopHarnessKind::Zed,
                version: "1.19.2".into(),
                runtime_version: None,
                channel: String::new(),
                url: String::new(),
                format: frozen::PackageFormat::WindowsSetup,
                digest: None,
                revision: None,
                staged: false,
                installer,
            })],
        }
    }

    #[tokio::test]
    async fn external_installs_must_match_the_frozen_version_without_downloading() {
        let state = tempfile::tempdir().unwrap();
        let mut journal = Journal::create(state.path()).unwrap();
        let manifest = frozen_manifest(Installer::External);
        let installed = |version: &str| Installation {
            executable: PathBuf::from("/synthetic/zed"),
            app_version: Some(version.parse().unwrap()),
            runtime_version: None,
        };
        let app = DesktopHarnessKind::Zed;
        assert!(matches!(
            prepare_frozen(
                app,
                Ok(Some(installed("1.19.2"))),
                &manifest,
                None,
                &mut journal
            )
            .await,
            Prepare::Ready(_)
        ));
        assert!(matches!(
            prepare_frozen(
                app,
                Ok(Some(installed("1.19.3"))),
                &manifest,
                None,
                &mut journal
            )
            .await,
            Prepare::Blocked(Reason::UnsupportedVersion)
        ));
        assert!(matches!(
            prepare_frozen(app, Ok(None), &manifest, None, &mut journal).await,
            Prepare::Blocked(Reason::InstallationFailed)
        ));
        assert!(matches!(
            prepare_frozen(
                app,
                Err(Reason::InstallationAmbiguous),
                &manifest,
                None,
                &mut journal
            )
            .await,
            Prepare::Blocked(Reason::InstallationAmbiguous)
        ));
        let mut blocked = manifest.clone();
        blocked.apps[0] = Entry::Blocked(frozen::Blocker {
            app,
            reason: BlockReason::ResolutionFailed,
            evidence: String::new(),
        });
        assert!(matches!(
            prepare_frozen(app, Ok(None), &blocked, None, &mut journal).await,
            Prepare::Blocked(Reason::VersionUnknown)
        ));
        // No network or install resource was reserved for any of these outcomes.
        assert!(journal.pending_names().is_empty());
    }

    #[tokio::test]
    async fn checker_installs_need_their_staged_frozen_bytes() {
        let state = tempfile::tempdir().unwrap();
        let mut journal = Journal::create(state.path()).unwrap();
        let mut manifest = frozen_manifest(Installer::Checker);
        if let Entry::Frozen(release) = &mut manifest.apps[0] {
            release.format = frozen::PackageFormat::TarGz;
            release.staged = true;
            release.digest = Some(format!("sha256:{}", "0".repeat(64)));
        }
        assert!(matches!(
            prepare_frozen(
                DesktopHarnessKind::Zed,
                Ok(None),
                &manifest,
                None,
                &mut journal
            )
            .await,
            Prepare::Blocked(Reason::InstallationUnavailable)
        ));
    }
}
