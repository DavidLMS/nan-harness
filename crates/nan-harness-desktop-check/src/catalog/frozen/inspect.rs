//! Read a version from frozen artifact bytes without executing or installing them.

pub(crate) mod pe;
pub(crate) mod zip;

use super::{PackageFormat, exact_version};
use crate::catalog::versions::asar;
use nan_harness_core::DesktopHarnessKind;
use semver::Version;
use std::fs::File;
use std::path::Path;

const MAX_TAR_ENTRIES: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InspectError {
    /// No single exact version was present in bounded package metadata.
    Unresolved,
    /// A read-only image may remain mounted.
    CleanupUncertain,
}

pub(crate) fn artifact_version(
    kind: DesktopHarnessKind,
    format: PackageFormat,
    path: &Path,
) -> Result<Version, InspectError> {
    let raw = match format {
        PackageFormat::Msix => msix_package_version(path),
        PackageFormat::TarGz => {
            tar_package_version(File::open(path).map_err(|_| InspectError::Unresolved)?)
        }
        PackageFormat::WindowsSetup => pe::product_version(path),
        PackageFormat::Dmg => dmg_bundle_version(kind, path)?,
        PackageFormat::Zip | PackageFormat::Deb | PackageFormat::Source => None,
    };
    raw.as_deref()
        .and_then(exact_version)
        .ok_or(InspectError::Unresolved)
}

/// Electron's package.json inside the MSIX `resources/app.asar`; the four-component
/// `AppxManifest` identity is deliberately not converted into an application version.
pub(crate) fn msix_package_version(path: &Path) -> Option<String> {
    let entries = zip::entries(path).ok()?;
    zip::validate(&entries).ok()?;
    let mut candidates = entries.iter().filter(|entry| is_app_asar(&entry.name));
    let entry = candidates.next()?;
    if candidates.next().is_some() {
        return None;
    }
    let mut reader = zip::open(path, entry).ok()?;
    let package = asar::package_json(&mut reader, entry.uncompressed).ok()??;
    asar::package_version(&package)
}

fn is_app_asar(name: &str) -> bool {
    let components = name.split('/').collect::<Vec<_>>();
    components.len() <= 4 && components.ends_with(&["resources", "app.asar"])
}

/// Streams a gzip tarball once, requiring exactly one application ASAR.
pub(crate) fn tar_package_version(input: impl std::io::Read) -> Option<String> {
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(input));
    let mut found = None;
    for (index, entry) in archive.entries().ok()?.enumerate() {
        if index >= MAX_TAR_ENTRIES {
            return None;
        }
        let mut entry = entry.ok()?;
        let path = entry.path().ok()?.to_string_lossy().into_owned();
        if !is_app_asar(path.trim_start_matches("./")) {
            continue;
        }
        if found.is_some() || !entry.header().entry_type().is_file() {
            return None;
        }
        let length = entry.size();
        found = Some(asar::package_json(&mut entry, length).ok()??);
    }
    asar::package_version(&found?)
}

#[cfg(target_os = "macos")]
fn dmg_bundle_version(
    kind: DesktopHarnessKind,
    path: &Path,
) -> Result<Option<String>, InspectError> {
    use std::process::Command;
    use std::time::Duration;
    let mount = path.with_extension("mount");
    nan_harness_private_fs::create_private_dir(&mount).map_err(|_| InspectError::Unresolved)?;
    let attached = crate::catalog::versions::command_output_within(
        Command::new("/usr/bin/hdiutil")
            .args([
                "attach",
                "-readonly",
                "-nobrowse",
                "-noautoopen",
                "-mountpoint",
            ])
            .arg(&mount)
            .arg(path),
        Duration::from_mins(3),
    );
    if attached.is_err() {
        // A timed-out attach may still complete; do not treat the image as released.
        return detach(&mount).map(|()| None);
    }
    let name = crate::catalog::app_bundle_name(kind);
    let version = crate::catalog::versions::command_output(
        Command::new("/usr/bin/plutil")
            .args(["-extract", "CFBundleShortVersionString", "raw", "-o", "-"])
            .arg(mount.join(format!("{name}.app/Contents/Info.plist"))),
    )
    .ok()
    .map(|text| text.trim().to_owned());
    detach(&mount)?;
    Ok(version)
}

#[cfg(target_os = "macos")]
fn detach(mount: &Path) -> Result<(), InspectError> {
    let detached = crate::catalog::versions::command_output_within(
        std::process::Command::new("/usr/bin/hdiutil")
            .arg("detach")
            .arg(mount),
        std::time::Duration::from_mins(1),
    );
    match detached {
        Ok(_) => std::fs::remove_dir(mount).map_err(|_| InspectError::CleanupUncertain),
        // An attach that never happened leaves an empty private directory.
        Err(_) if std::fs::remove_dir(mount).is_ok() => Ok(()),
        Err(_) => Err(InspectError::CleanupUncertain),
    }
}

#[cfg(not(target_os = "macos"))]
fn dmg_bundle_version(_: DesktopHarnessKind, _: &Path) -> Result<Option<String>, InspectError> {
    Ok(None)
}
