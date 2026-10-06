use super::{InstallError, PathBuf, Read};

/// Raw iteration lets us bound metadata before tar allocates long-name buffers.
#[derive(Default)]
pub(super) struct Extensions {
    path: Option<PathBuf>,
    link: Option<PathBuf>,
}

impl Extensions {
    pub(super) fn consume<R: Read>(
        &mut self,
        entry: &mut tar::Entry<'_, R>,
    ) -> Result<bool, InstallError> {
        let kind = entry.header().entry_type();
        if kind.is_gnu_longname() || kind.is_gnu_longlink() {
            if entry.size() > 16_384 {
                return Err(InstallError::TooLarge);
            }
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;
            if bytes.last() == Some(&0) {
                bytes.pop();
            }
            let path = String::from_utf8(bytes).map_err(|_| InstallError::Archive)?;
            let slot = if kind.is_gnu_longname() {
                &mut self.path
            } else {
                &mut self.link
            };
            if slot.replace(PathBuf::from(path)).is_some() {
                return Err(InstallError::Archive);
            }
            return Ok(true);
        }
        if kind.is_pax_local_extensions() {
            if entry.size() > 65_536 {
                return Err(InstallError::TooLarge);
            }
            for extension in entry.pax_extensions()?.ok_or(InstallError::Archive)? {
                let extension = extension.map_err(|_| InstallError::Archive)?;
                let key = extension.key().map_err(|_| InstallError::Archive)?;
                if key == "size" || key.starts_with("GNU.sparse") {
                    return Err(InstallError::Archive);
                }
                let slot = match key {
                    "path" => &mut self.path,
                    "linkpath" => &mut self.link,
                    _ => continue,
                };
                let value = extension.value().map_err(|_| InstallError::Archive)?;
                if slot.replace(PathBuf::from(value)).is_some() {
                    return Err(InstallError::Archive);
                }
            }
            return Ok(true);
        }
        if kind.is_pax_global_extensions() {
            return Err(InstallError::Archive);
        }
        Ok(false)
    }

    pub(super) fn path<R: Read>(
        &mut self,
        entry: &tar::Entry<'_, R>,
    ) -> Result<PathBuf, InstallError> {
        if self.link.is_some() && !entry.header().entry_type().is_symlink() {
            return Err(InstallError::Archive);
        }
        self.path.take().map_or_else(
            || {
                entry
                    .path()
                    .map(std::borrow::Cow::into_owned)
                    .map_err(|_| InstallError::Archive)
            },
            Ok,
        )
    }

    pub(super) fn link<R: Read>(
        &mut self,
        entry: &tar::Entry<'_, R>,
    ) -> Result<PathBuf, InstallError> {
        if let Some(link) = self.link.take() {
            return Ok(link);
        }
        entry
            .link_name()
            .map_err(|_| InstallError::Archive)?
            .map(std::borrow::Cow::into_owned)
            .ok_or(InstallError::Archive)
    }

    pub(super) const fn is_empty(&self) -> bool {
        self.path.is_none() && self.link.is_none()
    }
}
