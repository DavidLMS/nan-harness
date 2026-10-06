//! Bounded ZIP central-directory reader (APPNOTE 6.3.10). Only stored and deflated
//! entries are read; nothing is extracted by this module.

use std::fs::File;
use std::io::{Read, Seek as _, SeekFrom};
use std::path::Path;

const MAX_ENTRIES: u64 = 100_000;
const MAX_DIRECTORY_BYTES: u64 = 64 * 1024 * 1024;
const MAX_EXPANDED_BYTES: u64 = 4 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ZipError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ZipEntry {
    pub name: String,
    pub method: u16,
    pub flags: u16,
    pub compressed: u64,
    pub uncompressed: u64,
    pub local_offset: u64,
}

fn u16_at(bytes: &[u8], offset: usize) -> Result<u16, ZipError> {
    Ok(u16::from_le_bytes(
        bytes
            .get(offset..offset + 2)
            .ok_or(ZipError)?
            .try_into()
            .map_err(|_| ZipError)?,
    ))
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, ZipError> {
    Ok(u32::from_le_bytes(
        bytes
            .get(offset..offset + 4)
            .ok_or(ZipError)?
            .try_into()
            .map_err(|_| ZipError)?,
    ))
}

fn u64_at(bytes: &[u8], offset: usize) -> Result<u64, ZipError> {
    Ok(u64::from_le_bytes(
        bytes
            .get(offset..offset + 8)
            .ok_or(ZipError)?
            .try_into()
            .map_err(|_| ZipError)?,
    ))
}

fn read_at(file: &mut File, offset: u64, length: u64) -> Result<Vec<u8>, ZipError> {
    file.seek(SeekFrom::Start(offset)).map_err(|_| ZipError)?;
    let mut bytes = vec![0u8; usize::try_from(length).map_err(|_| ZipError)?];
    file.read_exact(&mut bytes).map_err(|_| ZipError)?;
    Ok(bytes)
}

pub(crate) fn entries(path: &Path) -> Result<Vec<ZipEntry>, ZipError> {
    let mut file = File::open(path).map_err(|_| ZipError)?;
    let length = file.metadata().map_err(|_| ZipError)?.len();
    parse(&mut file, length)
}

fn parse(file: &mut File, length: u64) -> Result<Vec<ZipEntry>, ZipError> {
    let tail_length = length.min(22 + 65_535);
    let tail = read_at(file, length - tail_length, tail_length)?;
    let end = (0..tail.len().saturating_sub(21))
        .rev()
        .find(|&index| {
            u32_at(&tail, index) == Ok(0x0605_4b50)
                && u16_at(&tail, index + 20)
                    .is_ok_and(|comment| index + 22 + usize::from(comment) == tail.len())
        })
        .ok_or(ZipError)?;
    let record = &tail[end..];
    if u16_at(record, 4)? != 0 || u16_at(record, 6)? != 0 {
        return Err(ZipError);
    }
    let mut count = u64::from(u16_at(record, 10)?);
    let mut size = u64::from(u32_at(record, 12)?);
    let mut offset = u64::from(u32_at(record, 16)?);
    if count == 0xffff || size == 0xffff_ffff || offset == 0xffff_ffff {
        let locator = end.checked_sub(20).ok_or(ZipError)?;
        if u32_at(&tail, locator)? != 0x0706_4b50 {
            return Err(ZipError);
        }
        let record = read_at(file, u64_at(&tail, locator + 8)?, 56)?;
        if u32_at(&record, 0)? != 0x0606_4b50
            || u32_at(&record, 16)? != 0
            || u32_at(&record, 20)? != 0
        {
            return Err(ZipError);
        }
        count = u64_at(&record, 32)?;
        size = u64_at(&record, 40)?;
        offset = u64_at(&record, 48)?;
    }
    if count > MAX_ENTRIES
        || size > MAX_DIRECTORY_BYTES
        || offset.checked_add(size).is_none_or(|end| end > length)
    {
        return Err(ZipError);
    }
    let directory = read_at(file, offset, size)?;
    let mut position = 0usize;
    let mut result = Vec::new();
    for _ in 0..count {
        let header = directory.get(position..position + 46).ok_or(ZipError)?;
        if u32_at(header, 0)? != 0x0201_4b50 {
            return Err(ZipError);
        }
        let name_length = usize::from(u16_at(header, 28)?);
        let extra_length = usize::from(u16_at(header, 30)?);
        let comment_length = usize::from(u16_at(header, 32)?);
        let name_start = position + 46;
        let extra_start = name_start + name_length;
        let name = directory.get(name_start..extra_start).ok_or(ZipError)?;
        let extra = directory
            .get(extra_start..extra_start + extra_length)
            .ok_or(ZipError)?;
        let mut entry = ZipEntry {
            name: String::from_utf8(name.to_vec()).map_err(|_| ZipError)?,
            method: u16_at(header, 10)?,
            flags: u16_at(header, 8)?,
            compressed: u64::from(u32_at(header, 20)?),
            uncompressed: u64::from(u32_at(header, 24)?),
            local_offset: u64::from(u32_at(header, 42)?),
        };
        zip64_fields(&mut entry, extra)?;
        result.push(entry);
        position = extra_start + extra_length + comment_length;
    }
    if position != directory.len() {
        return Err(ZipError);
    }
    Ok(result)
}

fn zip64_fields(entry: &mut ZipEntry, mut extra: &[u8]) -> Result<(), ZipError> {
    while !extra.is_empty() {
        let id = u16_at(extra, 0)?;
        let length = usize::from(u16_at(extra, 2)?);
        let data = extra.get(4..4 + length).ok_or(ZipError)?;
        if id == 1 {
            let mut cursor = 0;
            for field in [
                &mut entry.uncompressed,
                &mut entry.compressed,
                &mut entry.local_offset,
            ] {
                if *field == 0xffff_ffff {
                    *field = u64_at(data, cursor)?;
                    cursor += 8;
                }
            }
        }
        extra = &extra[4 + length..];
    }
    if [entry.uncompressed, entry.compressed, entry.local_offset].contains(&0xffff_ffff) {
        return Err(ZipError);
    }
    Ok(())
}

/// Reject traversal, absolute, platform-specific, duplicate or oversized entries.
pub(crate) fn validate(entries: &[ZipEntry]) -> Result<(), ZipError> {
    let mut names = std::collections::BTreeSet::new();
    let mut total = 0u64;
    for entry in entries {
        let name = entry.name.trim_end_matches('/');
        if name.is_empty()
            || entry.name.starts_with('/')
            || entry.name.contains(['\\', '\0', ':'])
            || name
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
            || entry.flags & 1 != 0
            || !matches!(entry.method, 0 | 8)
            || !names.insert(name.to_owned())
        {
            return Err(ZipError);
        }
        total = total.checked_add(entry.uncompressed).ok_or(ZipError)?;
    }
    if total > MAX_EXPANDED_BYTES {
        return Err(ZipError);
    }
    Ok(())
}

/// A bounded reader over one validated entry's uncompressed bytes.
pub(crate) fn open(path: &Path, entry: &ZipEntry) -> Result<Box<dyn Read>, ZipError> {
    let mut file = File::open(path).map_err(|_| ZipError)?;
    let header = read_at(&mut file, entry.local_offset, 30)?;
    if u32_at(&header, 0)? != 0x0403_4b50 {
        return Err(ZipError);
    }
    let start =
        entry.local_offset + 30 + u64::from(u16_at(&header, 26)?) + u64::from(u16_at(&header, 28)?);
    file.seek(SeekFrom::Start(start)).map_err(|_| ZipError)?;
    let data = file.take(entry.compressed);
    Ok(match entry.method {
        0 => Box::new(data),
        8 => Box::new(flate2::read::DeflateDecoder::new(data).take(entry.uncompressed)),
        _ => return Err(ZipError),
    })
}
