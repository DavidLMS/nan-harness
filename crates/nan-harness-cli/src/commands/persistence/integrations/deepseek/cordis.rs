use super::super::super::{ManagedCordisDocument, ManagedCordisEntry, PersistenceError, sha256};
use serde_yaml_ng::{Mapping, Value};
use std::path::Path;

pub(super) fn parse(source: &str, path: &Path) -> Result<Value, PersistenceError> {
    let document = if source.trim().is_empty() {
        Value::Sequence(Vec::new())
    } else {
        serde_yaml_ng::from_str(source)
            .map_err(|_| PersistenceError::InvalidManagedSection(path.to_owned()))?
    };
    if !document.is_sequence() {
        return Err(PersistenceError::InvalidManagedSection(path.to_owned()));
    }
    Ok(document)
}

pub(super) fn render(document: &Value) -> Result<String, PersistenceError> {
    serde_yaml_ng::to_string(document)
        .map_err(|error| PersistenceError::RenderConfiguration(error.to_string()))
}

fn matching_entries<'a>(document: &'a Value, id: &str, out: &mut Vec<&'a Value>) {
    if let Some(rows) = document.as_sequence() {
        for row in rows {
            if row.get("id").and_then(Value::as_str) == Some(id) {
                out.push(row);
            }
            if let Some(insert) = row.get("insert") {
                matching_entries(insert, id, out);
            }
        }
    }
}

pub(super) fn entry<'a>(
    document: &'a Value,
    id: &str,
    path: &Path,
) -> Result<Option<&'a Value>, PersistenceError> {
    let mut matches = Vec::new();
    matching_entries(document, id, &mut matches);
    if matches.len() > 1 {
        return Err(PersistenceError::InvalidManagedSection(path.to_owned()));
    }
    Ok(matches.into_iter().next())
}

fn entry_mut<'a>(document: &'a mut Value, id: &str) -> Option<&'a mut Value> {
    for row in document.as_sequence_mut()? {
        if row.get("id").and_then(Value::as_str) == Some(id) {
            return Some(row);
        }
        if let Some(insert) = row.get_mut("insert")
            && let Some(found) = entry_mut(insert, id)
        {
            return Some(found);
        }
    }
    None
}

pub(super) fn field<'a>(value: &'a Value, key: &[String]) -> Option<&'a Value> {
    key.iter()
        .try_fold(value, |current, part| current.get(part))
}

fn set(
    value: &mut Value,
    key: &[String],
    desired: Option<Value>,
    path: &Path,
) -> Result<(), PersistenceError> {
    let Some((first, rest)) = key.split_first() else {
        return Err(PersistenceError::InvalidManagedSection(path.to_owned()));
    };
    let map = value
        .as_mapping_mut()
        .ok_or_else(|| PersistenceError::InvalidManagedSection(path.to_owned()))?;
    if rest.is_empty() {
        if let Some(desired) = desired {
            map.insert(Value::String(first.clone()), desired);
        } else {
            map.remove(Value::String(first.clone()));
        }
        return Ok(());
    }
    let child = map
        .entry(Value::String(first.clone()))
        .or_insert_with(|| Value::Mapping(Mapping::new()));
    set(child, rest, desired, path)?;
    if child.as_mapping().is_some_and(Mapping::is_empty) {
        map.remove(Value::String(first.clone()));
    }
    Ok(())
}

pub(super) fn verify(
    document: &Value,
    receipt: &ManagedCordisDocument,
) -> Result<(), PersistenceError> {
    for owned in &receipt.entries {
        if entry(document, &owned.id, &receipt.path)?.and_then(|row| field(row, &owned.key))
            != Some(&owned.value)
        {
            return Err(PersistenceError::ManagedSectionChanged(
                receipt.path.clone(),
            ));
        }
    }
    Ok(())
}

pub(super) fn own(
    document: &mut Value,
    receipt: &mut ManagedCordisDocument,
    id: &str,
    key: &[&str],
    desired: Value,
    exclusive: bool,
) -> Result<(), PersistenceError> {
    let key = key
        .iter()
        .map(|part| (*part).to_owned())
        .collect::<Vec<_>>();
    let existing = entry(document, id, &receipt.path)?
        .and_then(|row| field(row, &key))
        .cloned();
    let previous = receipt
        .entries
        .iter()
        .find(|owned| owned.id == id && owned.key == key);
    if previous.is_none() && exclusive && existing.is_some() {
        return Err(PersistenceError::UnmanagedSectionConflict(
            receipt.path.clone(),
        ));
    }
    let baseline = previous.map_or(existing.clone(), |owned| owned.previous.clone());
    if entry(document, id, &receipt.path)?.is_none() {
        let row = Value::Mapping(Mapping::from_iter([(
            Value::String("id".to_owned()),
            Value::String(id.to_owned()),
        )]));
        document
            .as_sequence_mut()
            .ok_or_else(|| PersistenceError::InvalidManagedSection(receipt.path.clone()))?
            .push(row);
        receipt.created_entries.push(id.to_owned());
    }
    set(
        entry_mut(document, id)
            .ok_or_else(|| PersistenceError::InvalidManagedSection(receipt.path.clone()))?,
        &key,
        Some(desired.clone()),
        &receipt.path,
    )?;
    receipt
        .entries
        .retain(|owned| owned.id != id || owned.key != key);
    receipt.entries.push(ManagedCordisEntry {
        id: id.to_owned(),
        key,
        value: desired,
        previous: baseline,
    });
    Ok(())
}

pub(super) fn restore(
    document: &mut Value,
    receipt: &ManagedCordisDocument,
) -> Result<(), PersistenceError> {
    verify(document, receipt)?;
    for owned in receipt.entries.iter().rev() {
        set(
            entry_mut(document, &owned.id)
                .ok_or_else(|| PersistenceError::InvalidManagedSection(receipt.path.clone()))?,
            &owned.key,
            owned.previous.clone(),
            &receipt.path,
        )?;
    }
    // A row created by us can acquire unrelated user fields. Remove only an empty patch.
    document
        .as_sequence_mut()
        .ok_or_else(|| PersistenceError::InvalidManagedSection(receipt.path.clone()))?
        .retain(|row| {
            !(row.as_mapping().is_some_and(|map| {
                map.keys()
                    .all(|key| matches!(key.as_str(), Some("id" | "name")))
            }) && row
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| receipt.created_entries.iter().any(|created| created == id)))
        });
    Ok(())
}

pub(super) fn receipt(
    path: &Path,
    original: Option<&str>,
    previous: Option<&ManagedCordisDocument>,
) -> ManagedCordisDocument {
    previous.cloned().unwrap_or_else(|| ManagedCordisDocument {
        path: path.to_owned(),
        entries: Vec::new(),
        created_entries: Vec::new(),
        original: original.map(str::to_owned),
        rendered_sha256: sha256(original.unwrap_or_default().as_bytes()),
    })
}

/// Search has a separate byte-owned receipt. Preserve its exact block when sharing the home patch.
pub(super) fn render_preserving_search(
    document: &Value,
    source: &str,
    path: &Path,
) -> Result<String, PersistenceError> {
    let begin = "# nan-harness:begin search-mcp";
    let end = "# nan-harness:end search-mcp";
    let Some(start) = source.find(begin) else {
        return render(document);
    };
    let tail = source[start..]
        .find(end)
        .ok_or(PersistenceError::InvalidManagedBlock)?
        + start
        + end.len();
    let finish = tail + usize::from(source.as_bytes().get(tail) == Some(&b'\n'));
    let block = &source[start..finish];
    let search = parse(block, path)?;
    let mut rest = document.clone();
    for owned in search
        .as_sequence()
        .ok_or_else(|| PersistenceError::InvalidManagedSection(path.to_owned()))?
    {
        let rows = rest
            .as_sequence_mut()
            .ok_or_else(|| PersistenceError::InvalidManagedSection(path.to_owned()))?;
        let index = rows
            .iter()
            .position(|row| row == owned)
            .ok_or(PersistenceError::InvalidManagedBlock)?;
        rows.remove(index);
    }
    let mut rendered = if rest.as_sequence().is_some_and(Vec::is_empty) {
        String::new()
    } else {
        render(&rest)?
    };
    rendered.push_str(block);
    Ok(rendered)
}
