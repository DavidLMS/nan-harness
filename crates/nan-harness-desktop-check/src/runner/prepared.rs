//! Private installation receipts bridge credential-free preparation and GUI checks.

use super::*;
use serde::Serialize;

type Inventory = Vec<(
    DesktopHarnessKind,
    Result<Option<Installation>, DiscoveryError>,
)>;

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
    apps: Vec<PreparedApp>,
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
}

pub(super) struct Prepared {
    pub inventory: Inventory,
    pub nanh: (PathBuf, BinaryIdentity),
    pub owner: Journal,
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
    let apps = apps.into_iter().collect::<BTreeSet<_>>();
    let inventory = apps
        .iter()
        .map(|&app| (app, catalog::discover(app)))
        .collect::<Vec<_>>();
    let existing = discover_nanh(args.nan_harness.as_deref()).await?;
    print_inventory(&inventory, existing.as_ref(), false, true);
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
    let outcome = prepare_receipt(inventory, existing, &mut journal).await;
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
    Ok(0)
}

async fn prepare_receipt(
    inventory: Inventory,
    existing: Option<(PathBuf, BinaryIdentity)>,
    journal: &mut Journal,
) -> Result<Receipt, String> {
    let (nanh, nanh_identity) = match existing {
        Some(existing) => existing,
        None => install_nanh(journal).await?,
    };
    let mut apps = Vec::new();
    for (app, found) in inventory {
        let installed = match found {
            Ok(Some(installed)) => Some(installed),
            Err(DiscoveryError::Unsupported) => None,
            Err(error) => return Err(error.to_string()),
            Ok(None) => match install::install(app, journal).await {
                Ok(installed) => Some(installed),
                Err(
                    install::InstallError::Unavailable
                    | install::InstallError::ExternalInstallation,
                ) => None,
                Err(error) => return Err(error.to_string()),
            },
        };
        apps.push(PreparedApp {
            app,
            executable: installed
                .as_ref()
                .map(|installed| Executable::record(&installed.executable))
                .transpose()?,
            app_version: installed
                .as_ref()
                .and_then(|installed| installed.app_version.clone()),
            runtime_version: installed.and_then(|installed| installed.runtime_version),
        });
    }
    Ok(Receipt {
        schema_version: 1,
        run_id: journal.run_id().into(),
        platform: Platform::current(),
        architecture: Architecture::current(),
        checker: Executable::record(
            &std::env::current_exe().map_err(|_| "cannot identify checker")?,
        )?,
        nanh: Executable::record(&nanh)?,
        nanh_identity,
        apps,
    })
}

pub(super) fn load(path: &Path, apps: &[DesktopHarnessKind]) -> Result<Prepared, String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(65537).read_to_end(&mut bytes))
        .map_err(|_| "cannot read private preparation receipt")?;
    if bytes.len() > 65536 {
        return Err("preparation receipt is too large".into());
    }
    let receipt: Receipt =
        serde_json::from_slice(&bytes).map_err(|_| "invalid preparation receipt")?;
    if receipt.schema_version != 1
        || receipt.platform != Platform::current()
        || receipt.architecture != Architecture::current()
        || receipt.apps.len() != apps.len()
        || receipt
            .apps
            .iter()
            .map(|app| app.app)
            .collect::<BTreeSet<_>>()
            != apps.iter().copied().collect()
    {
        return Err("preparation does not match the selected apps and platform".into());
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
    let mut inventory = Vec::new();
    for app in receipt.apps {
        let Some(executable) = app.executable else {
            if app.app_version.is_some() || app.runtime_version.is_some() {
                return Err("unavailable prepared application has an inconsistent identity".into());
            }
            inventory.push((app.app, Err(DiscoveryError::Unsupported)));
            continue;
        };
        executable.verify()?;
        inventory.push((
            app.app,
            Ok(Some(Installation {
                executable: executable.path,
                app_version: app.app_version,
                runtime_version: app.runtime_version,
            })),
        ));
    }
    Ok(Prepared {
        inventory,
        nanh: (receipt.nanh.path, receipt.nanh_identity),
        owner,
    })
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
        assert!(load(&path, &[]).is_err());
        std::fs::write(&path, vec![b' '; 65537]).unwrap();
        assert!(load(&path, &[]).is_err());
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}
