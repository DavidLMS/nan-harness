use super::{InstallError, MAX_ENTRIES, MAX_EXPANDED_BYTES, process};
use flate2::read::GzDecoder;
use nan_harness_private_fs::{create_private_dir, open_private_new};
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{Read, Seek as _, SeekFrom};
use std::path::{Component, Path, PathBuf};

mod extensions;

pub(super) fn extract_gzip(input: &Path, destination: &Path) -> Result<(), InstallError> {
    extract_tar(GzDecoder::new(File::open(input)?), destination)
}

pub(super) async fn extract_deb(
    input: &Path,
    destination: &Path,
    root: &Path,
) -> Result<(), InstallError> {
    let data = root.join("debian-data");
    let compression = debian_data(input, &data)?;
    match compression.as_str() {
        "data.tar.gz" => extract_gzip(&data, destination),
        "data.tar" => extract_tar(File::open(data)?, destination),
        "data.tar.xz" | "data.tar.zst" => {
            let tar = root.join("debian-data.tar");
            let command = if compression == "data.tar.xz" {
                "xz"
            } else {
                "zstd"
            };
            process::decompress(command, &data, &tar).await?;
            extract_tar(File::open(tar)?, destination)
        }
        _ => Err(InstallError::Archive),
    }
}

fn debian_data(input: &Path, output: &Path) -> Result<String, InstallError> {
    let mut file = File::open(input)?;
    let length = file.metadata()?.len();
    let mut magic = [0u8; 8];
    file.read_exact(&mut magic)?;
    if &magic != b"!<arch>\n" {
        return Err(InstallError::Archive);
    }
    let mut selected = None;
    while file.stream_position()? < length {
        let mut header = [0u8; 60];
        file.read_exact(&mut header)?;
        if &header[58..] != b"`\n" {
            return Err(InstallError::Archive);
        }
        let name = std::str::from_utf8(&header[..16])
            .map_err(|_| InstallError::Archive)?
            .trim()
            .trim_end_matches('/');
        let size = std::str::from_utf8(&header[48..58])
            .map_err(|_| InstallError::Archive)?
            .trim()
            .parse::<u64>()
            .map_err(|_| InstallError::Archive)?;
        let end = file
            .stream_position()?
            .checked_add(size)
            .ok_or(InstallError::TooLarge)?;
        if end > length {
            return Err(InstallError::Archive);
        }
        if name.starts_with("data.tar") {
            if selected.is_some()
                || !matches!(
                    name,
                    "data.tar" | "data.tar.gz" | "data.tar.xz" | "data.tar.zst"
                )
            {
                return Err(InstallError::Archive);
            }
            let mut destination = open_private_new(output)?;
            std::io::copy(&mut (&mut file).take(size), &mut destination)?;
            selected = Some(name.to_owned());
        }
        file.seek(SeekFrom::Start(end + size % 2))?;
    }
    selected.ok_or(InstallError::Archive)
}

fn extract_tar(reader: impl Read, root: &Path) -> Result<(), InstallError> {
    let mut archive = tar::Archive::new(reader);
    let mut total = 0u64;
    let mut seen = BTreeSet::new();
    let mut links = Vec::new();
    let mut extensions = extensions::Extensions::default();
    for (index, entry) in archive
        .entries()
        .map_err(|_| InstallError::Archive)?
        .raw(true)
        .enumerate()
    {
        if index >= MAX_ENTRIES {
            return Err(InstallError::TooLarge);
        }
        let mut entry = entry.map_err(|_| InstallError::Archive)?;
        if extensions.consume(&mut entry)? {
            continue;
        }
        let relative = safe_relative(&extensions.path(&entry)?)?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let target = root.join(&relative);
        let entry_type = entry.header().entry_type();
        if !seen.insert(relative.clone()) {
            return Err(InstallError::Archive);
        }
        ensure_parents(root, &relative)?;
        if entry_type.is_dir() {
            if !target.exists() {
                create_private_dir(&target)?;
            }
        } else if entry_type.is_file() {
            total = total
                .checked_add(entry.size())
                .ok_or(InstallError::TooLarge)?;
            if total > MAX_EXPANDED_BYTES {
                return Err(InstallError::TooLarge);
            }
            let mut output = open_private_new(&target)?;
            std::io::copy(&mut entry, &mut output)?;
            executable_mode(
                &target,
                entry.header().mode().map_err(|_| InstallError::Archive)?,
            )?;
        } else if entry_type.is_symlink() {
            let link = extensions.link(&entry)?;
            validate_link(&relative, &link)?;
            links.push((target, link));
        } else {
            return Err(InstallError::Archive);
        }
    }
    if !extensions.is_empty() {
        return Err(InstallError::Archive);
    }
    // No archive entry can traverse a link: links are installed only after all
    // regular files and directories, and their targets stay inside this tree.
    install_links(root, links)
}

pub(super) fn safe_relative(path: &Path) -> Result<PathBuf, InstallError> {
    let mut result = PathBuf::new();
    for part in path.components() {
        match part {
            Component::Normal(name) => result.push(name),
            Component::CurDir => {}
            _ => return Err(InstallError::Archive),
        }
    }
    Ok(result)
}

fn ensure_parents(root: &Path, relative: &Path) -> Result<(), InstallError> {
    let mut directory = root.to_path_buf();
    if let Some(parent) = relative.parent() {
        for part in parent.components() {
            directory.push(part);
            match fs::symlink_metadata(&directory) {
                Ok(metadata) if metadata.is_dir() => {}
                Ok(_) => return Err(InstallError::Archive),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    create_private_dir(&directory)?;
                }
                Err(error) => return Err(error.into()),
            }
        }
    }
    Ok(())
}

pub(super) fn validate_link(relative: &Path, link: &Path) -> Result<(), InstallError> {
    let mut depth = relative
        .parent()
        .map_or(0, |parent| parent.components().count());
    for part in link.components() {
        match part {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::ParentDir if depth > 0 => depth -= 1,
            _ => return Err(InstallError::Archive),
        }
    }
    Ok(())
}

pub(super) fn executable_mode(path: &Path, source_mode: u32) -> Result<(), InstallError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(
            path,
            fs::Permissions::from_mode(if source_mode & 0o111 != 0 {
                0o700
            } else {
                0o600
            }),
        )?;
    }
    #[cfg(not(unix))]
    let _ = (path, source_mode);
    Ok(())
}

pub(super) fn create_symlink(link: &Path, target: &Path) -> Result<(), InstallError> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(link, target)?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = (link, target);
        Err(InstallError::Archive)
    }
}

pub(super) fn install_links(
    root: &Path,
    links: Vec<(PathBuf, PathBuf)>,
) -> Result<(), InstallError> {
    for (target, link) in &links {
        create_symlink(link, target)?;
    }
    let root = fs::canonicalize(root)?;
    for (target, _) in links {
        let resolved = fs::canonicalize(target).map_err(|_| InstallError::Archive)?;
        if !resolved.starts_with(&root) {
            return Err(InstallError::Archive);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
