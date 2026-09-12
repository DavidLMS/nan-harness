//! Read `ProductVersion` from a PE `RT_VERSION` resource without executing the installer.
//! Structures follow the PE/COFF specification and `VS_VERSIONINFO` documentation.

use std::fs::File;
use std::io::{Read as _, Seek as _, SeekFrom};
use std::path::Path;

const MAX_RESOURCE_BYTES: u32 = 16 * 1024 * 1024;
const RT_VERSION: u32 = 16;

fn word(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(offset..offset + 2)?.try_into().ok()?))
}

fn dword(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(offset..offset + 4)?.try_into().ok()?))
}

fn read_at(file: &mut File, offset: u64, length: usize) -> Option<Vec<u8>> {
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut bytes = vec![0u8; length];
    file.read_exact(&mut bytes).ok()?;
    Some(bytes)
}

/// Every `ProductVersion` string in the version resource, which must agree.
pub(crate) fn product_version(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let dos = read_at(&mut file, 0, 64)?;
    if dos.get(..2)? != b"MZ" {
        return None;
    }
    let pe = u64::from(dword(&dos, 60)?);
    let headers = read_at(&mut file, pe, 24)?;
    if headers.get(..4)? != b"PE\0\0" {
        return None;
    }
    let sections = usize::from(word(&headers, 6)?);
    let optional_size = usize::from(word(&headers, 20)?);
    if sections == 0 || sections > 96 {
        return None;
    }
    let optional = read_at(&mut file, pe + 24, optional_size)?;
    let directories = match word(&optional, 0)? {
        0x10b => 96,
        0x20b => 112,
        _ => return None,
    };
    if dword(&optional, directories - 4)? < 3 {
        return None;
    }
    let resource_rva = dword(&optional, directories + 16)?;
    let resource_size = dword(&optional, directories + 20)?;
    if resource_rva == 0 || resource_size == 0 || resource_size > MAX_RESOURCE_BYTES {
        return None;
    }
    let table = read_at(&mut file, pe + 24 + optional_size as u64, sections * 40)?;
    let file_offset = |rva: u32, length: u32| -> Option<u64> {
        (0..sections).find_map(|index| {
            let section = &table[index * 40..index * 40 + 40];
            let virtual_address = dword(section, 12)?;
            let raw_size = dword(section, 16)?;
            let raw_offset = dword(section, 20)?;
            let delta = rva.checked_sub(virtual_address)?;
            (delta.checked_add(length)? <= raw_size)
                .then(|| u64::from(raw_offset) + u64::from(delta))
        })
    };
    let resources = read_at(
        &mut file,
        file_offset(resource_rva, resource_size)?,
        usize::try_from(resource_size).ok()?,
    )?;
    let versions = child(&resources, 0, Some(RT_VERSION), true)?;
    let named = child(&resources, versions, None, true)?;
    let language = child(&resources, named, None, false)?;
    let entry = resources.get(language as usize..language as usize + 16)?;
    let data_rva = dword(entry, 0)?;
    let data_size = dword(entry, 4)?;
    let data = resources
        .get(usize::try_from(data_rva.checked_sub(resource_rva)?).ok()?..)?
        .get(..usize::try_from(data_size).ok()?)?;
    let mut found = Vec::new();
    collect(data, 0, data.len(), 0, &mut found)?;
    let first = found.first()?.clone();
    found.iter().all(|value| *value == first).then_some(first)
}

/// Target of the single entry (or the entry with `id`) in one resource directory level.
fn child(resources: &[u8], directory: u32, id: Option<u32>, subdirectory: bool) -> Option<u32> {
    let directory = (directory & 0x7fff_ffff) as usize;
    let named = usize::from(word(resources, directory + 12)?);
    let ids = usize::from(word(resources, directory + 14)?);
    let mut matches = (0..named + ids).filter_map(|index| {
        let entry = directory + 16 + index * 8;
        let name = dword(resources, entry)?;
        let target = dword(resources, entry + 4)?;
        id.is_none_or(|id| name == id).then_some(target)
    });
    let target = matches.next()?;
    (matches.next().is_none() && (target & 0x8000_0000 != 0) == subdirectory).then_some(target)
}

fn align(offset: usize) -> usize {
    (offset + 3) & !3
}

/// Walk one `VS_VERSIONINFO` node and its children, collecting ProductVersion values.
fn collect(data: &[u8], start: usize, limit: usize, depth: u8, found: &mut Vec<String>) -> Option<()> {
    if depth > 4 {
        return None;
    }
    let length = usize::from(word(data, start)?);
    let value_length = usize::from(word(data, start + 2)?);
    let text = word(data, start + 4)? == 1;
    let end = start.checked_add(length)?;
    if length < 6 || end > limit {
        return None;
    }
    let mut cursor = start + 6;
    let mut key = Vec::new();
    loop {
        let unit = word(data, cursor)?;
        cursor += 2;
        if unit == 0 {
            break;
        }
        key.push(unit);
        if cursor >= end {
            return None;
        }
    }
    let key = String::from_utf16(&key).ok()?;
    cursor = align(cursor);
    let value_bytes = if text { value_length * 2 } else { value_length };
    if key == "ProductVersion" && text {
        let units = (0..value_length)
            .map(|index| word(data, cursor + index * 2))
            .collect::<Option<Vec<_>>>()?;
        let value = String::from_utf16(&units).ok()?;
        found.push(value.trim_end_matches('\0').trim().to_owned());
    }
    cursor = align(cursor.checked_add(value_bytes)?);
    while cursor + 6 <= end {
        let child_length = usize::from(word(data, cursor)?);
        if child_length == 0 {
            break;
        }
        collect(data, cursor, end, depth + 1, found)?;
        cursor = align(cursor + child_length);
    }
    Some(())
}

#[cfg(test)]
pub(crate) fn fixture(product_version: &str) -> Vec<u8> {
    fn node(key: &str, value: &[u8], text: bool, value_length: usize, children: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = vec![0u8; 6];
        for unit in key.encode_utf16().chain(std::iter::once(0)) {
            bytes.extend(unit.to_le_bytes());
        }
        while bytes.len() % 4 != 0 {
            bytes.push(0);
        }
        bytes.extend(value);
        while bytes.len() % 4 != 0 {
            bytes.push(0);
        }
        for child in children {
            bytes.extend(child);
            while bytes.len() % 4 != 0 {
                bytes.push(0);
            }
        }
        let length = u16::try_from(bytes.len()).expect("small node");
        bytes[0..2].copy_from_slice(&length.to_le_bytes());
        bytes[2..4].copy_from_slice(&u16::try_from(value_length).expect("small").to_le_bytes());
        bytes[4..6].copy_from_slice(&u16::from(text).to_le_bytes());
        bytes
    }
    let text = product_version
        .encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    let string = node("ProductVersion", &text, true, text.len() / 2, &[]);
    let table = node("040904b0", &[], true, 0, &[string]);
    let info = node("StringFileInfo", &[], true, 0, &[table]);
    let version = node("VS_VERSION_INFO", &[0u8; 52], false, 52, &[info]);
    // One .rsrc section at RVA 0x1000, file offset 0x200.
    let rva = 0x1000u32;
    let mut resources = vec![0u8; 16 + 8];
    resources[14..16].copy_from_slice(&1u16.to_le_bytes());
    resources[16..20].copy_from_slice(&RT_VERSION.to_le_bytes());
    resources[20..24].copy_from_slice(&(0x8000_0000u32 | 24).to_le_bytes());
    let mut level = vec![0u8; 24];
    level[14..16].copy_from_slice(&1u16.to_le_bytes());
    level[16..20].copy_from_slice(&1u32.to_le_bytes());
    level[20..24].copy_from_slice(&(0x8000_0000u32 | 48).to_le_bytes());
    resources.extend(level);
    let mut language = vec![0u8; 24];
    language[14..16].copy_from_slice(&1u16.to_le_bytes());
    language[16..20].copy_from_slice(&0x409u32.to_le_bytes());
    language[20..24].copy_from_slice(&72u32.to_le_bytes());
    resources.extend(language);
    let mut data_entry = vec![0u8; 16];
    data_entry[0..4].copy_from_slice(&(rva + 88).to_le_bytes());
    data_entry[4..8].copy_from_slice(&u32::try_from(version.len()).expect("small").to_le_bytes());
    resources.extend(data_entry);
    resources.extend(version);
    let mut image = vec![0u8; 0x200];
    image[..2].copy_from_slice(b"MZ");
    image[60..64].copy_from_slice(&64u32.to_le_bytes());
    image[64..68].copy_from_slice(b"PE\0\0");
    image[70..72].copy_from_slice(&1u16.to_le_bytes());
    image[84..86].copy_from_slice(&240u16.to_le_bytes());
    let optional = 88;
    image[optional..optional + 2].copy_from_slice(&0x20bu16.to_le_bytes());
    image[optional + 108..optional + 112].copy_from_slice(&16u32.to_le_bytes());
    image[optional + 128..optional + 132].copy_from_slice(&rva.to_le_bytes());
    let size = u32::try_from(resources.len()).expect("small");
    image[optional + 132..optional + 136].copy_from_slice(&size.to_le_bytes());
    let section = optional + 240;
    image[section..section + 5].copy_from_slice(b".rsrc");
    image[section + 12..section + 16].copy_from_slice(&rva.to_le_bytes());
    image[section + 16..section + 20].copy_from_slice(&size.to_le_bytes());
    image[section + 20..section + 24].copy_from_slice(&0x200u32.to_le_bytes());
    image.extend(resources);
    image
}
