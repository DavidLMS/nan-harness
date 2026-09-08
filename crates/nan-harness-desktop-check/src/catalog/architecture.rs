//! Check native executable headers before launch; never infer architecture from a URL.

use super::DiscoveryError;
use crate::report::{Architecture, Platform};
use std::{
    io::{Read as _, Seek as _, SeekFrom},
    path::Path,
};

pub(crate) fn matches_host(path: &Path) -> Result<bool, DiscoveryError> {
    let mut file = std::fs::File::open(path).map_err(|_| DiscoveryError::Unreadable)?;
    let mut header = vec![0u8; 4096];
    let count = file
        .read(&mut header)
        .map_err(|_| DiscoveryError::Unreadable)?;
    header.truncate(count);
    if Platform::current() == Platform::Windows && header.starts_with(b"MZ") && header.len() >= 64 {
        let offset = u32::from_le_bytes(
            header[60..64]
                .try_into()
                .map_err(|_| DiscoveryError::Unreadable)?,
        );
        if offset > 1024 * 1024 {
            return Ok(false);
        }
        file.seek(SeekFrom::Start(u64::from(offset)))
            .map_err(|_| DiscoveryError::Unreadable)?;
        let mut pe = [0u8; 6];
        file.read_exact(&mut pe)
            .map_err(|_| DiscoveryError::Unreadable)?;
        return Ok(pe.starts_with(b"PE\0\0")
            && u16::from_le_bytes([pe[4], pe[5]])
                == match Architecture::current() {
                    Architecture::X86_64 => 0x8664,
                    Architecture::Aarch64 => 0xaa64,
                });
    }
    Ok(native_header(
        &header,
        Platform::current(),
        Architecture::current(),
    ))
}

fn native_header(header: &[u8], platform: Platform, architecture: Architecture) -> bool {
    match platform {
        Platform::Linux => {
            header.len() >= 20
                && header.starts_with(b"\x7fELF")
                && header[4..6] == [2, 1]
                && u16::from_le_bytes([header[18], header[19]])
                    == match architecture {
                        Architecture::X86_64 => 62,
                        Architecture::Aarch64 => 183,
                    }
        }
        Platform::Macos => macho(header, architecture),
        Platform::Windows => false,
    }
}

fn macho(header: &[u8], architecture: Architecture) -> bool {
    let expected: u32 = match architecture {
        Architecture::X86_64 => 0x0100_0007,
        Architecture::Aarch64 => 0x0100_000c,
    };
    let Some(magic) = header.get(..4) else {
        return false;
    };
    let word = |offset: usize, little: bool| -> Option<u32> {
        let bytes = header.get(offset..offset + 4)?.try_into().ok()?;
        Some(if little {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        })
    };
    match magic {
        [0xcf, 0xfa, 0xed, 0xfe] => word(4, true) == Some(expected),
        [0xfe, 0xed, 0xfa, 0xcf] => word(4, false) == Some(expected),
        [0xca, 0xfe, 0xba, 0xbe | 0xbf] => {
            let Some(count) = word(4, false).filter(|count| (1..=32).contains(count)) else {
                return false;
            };
            let stride = if magic[3] == 0xbf { 32 } else { 20 };
            header.len() >= 8 + count as usize * stride
                && (0..count as usize)
                    .any(|index| word(8 + index * stride, false) == Some(expected))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elf_architecture_must_match_and_scripts_are_not_native_evidence() {
        let mut header = [0u8; 64];
        header[..6].copy_from_slice(b"\x7fELF\x02\x01");
        header[18] = 183;
        assert!(native_header(
            &header,
            Platform::Linux,
            Architecture::Aarch64
        ));
        assert!(!native_header(
            &header,
            Platform::Linux,
            Architecture::X86_64
        ));
        assert!(!native_header(
            b"#!/bin/sh",
            Platform::Linux,
            Architecture::Aarch64
        ));
    }

    #[test]
    fn universal_macho_requires_the_actual_native_slice() {
        let mut header = [0u8; 48];
        header[..4].copy_from_slice(&[0xca, 0xfe, 0xba, 0xbe]);
        header[4..8].copy_from_slice(&2u32.to_be_bytes());
        header[8..12].copy_from_slice(&0x0100_0007u32.to_be_bytes());
        header[28..32].copy_from_slice(&0x0100_000cu32.to_be_bytes());
        assert!(macho(&header, Architecture::X86_64));
        assert!(macho(&header, Architecture::Aarch64));
        assert!(!macho(&header[..20], Architecture::Aarch64));
        header[4..8].copy_from_slice(&1u32.to_be_bytes());
        assert!(!macho(&header, Architecture::Aarch64));
    }
}
