use super::*;

/// Vibe stores providers, models and MCP servers as arrays of tables, so
/// ownership is tracked per entry: the `nan` provider, models served by it,
/// and the `nan-search` server. Every other entry keeps its place and bytes.
const ARRAYS: [&str; 3] = ["providers", "models", "mcp_servers"];
const OWNED_PROVIDER: &str = "nan";
const ACTIVE_MODEL: &str = "active_model";

pub(crate) fn prepare_vibe(
    plan: &VibePlan,
    previous: Option<&VibeReceipt>,
) -> Result<PreparedDocument, ConfigurationError> {
    if previous.is_some_and(|receipt| receipt.path != plan.path) {
        return Err(ConfigurationError::ReceiptMismatch);
    }
    let original = read_optional(&plan.path)?;
    let permissions = file_permissions(&plan.path)?;
    let mut document = parse_vibe_document(original.as_deref(), &plan.path)?;
    if let Some(receipt) = previous
        && !vibe_receipt_is_active(&document, receipt)
    {
        return Err(ConfigurationError::ManagedDocumentChanged(
            plan.path.clone(),
        ));
    }
    let owned = previous
        .map(|receipt| {
            receipt
                .entries
                .iter()
                .map(|entry| (entry.array.clone(), entry.name.clone()))
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let previous_active_model = match previous {
        Some(receipt) => receipt.previous_active_model.clone(),
        None => read_active_model(&document, &plan.path)?,
    };
    reject_unowned_conflicts(&document, plan, &owned)?;
    for array in ARRAYS {
        apply_array(&mut document, plan, array, &owned)?;
    }
    set_active_model(&mut document, &plan.active_model);

    let entries = plan
        .entries
        .iter()
        .map(|entry| {
            hash_toml_item(&Item::Table(entry.table.clone())).map(|sha256| VibeEntryReceipt {
                array: entry.array.to_owned(),
                name: entry.name.clone(),
                sha256,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let active_model_sha256 = hash_toml_item(&document[ACTIVE_MODEL])?;
    let created_file = previous.map_or(original.is_none(), |receipt| receipt.created_file);
    Ok(PreparedDocument {
        path: plan.path.clone(),
        original,
        permissions,
        replacement: Some(document.to_string().into_bytes()),
        receipt: DocumentReceipt::VibeToml(VibeReceipt {
            path: plan.path.clone(),
            created_file,
            entries,
            active_model_sha256,
            previous_active_model,
        }),
    })
}

pub(crate) fn prepare_vibe_removal(
    receipt: &VibeReceipt,
) -> Result<PreparedDocument, ConfigurationError> {
    let original = read_optional(&receipt.path)?;
    let permissions = file_permissions(&receipt.path)?;
    if original.is_none() {
        return Err(ConfigurationError::ManagedDocumentChanged(
            receipt.path.clone(),
        ));
    }
    let mut document = parse_vibe_document(original.as_deref(), &receipt.path)?;
    if !vibe_receipt_is_active(&document, receipt) {
        return Err(ConfigurationError::ManagedDocumentChanged(
            receipt.path.clone(),
        ));
    }
    for array in ARRAYS {
        let Some(entries) = document
            .get_mut(array)
            .and_then(Item::as_array_of_tables_mut)
        else {
            continue;
        };
        let mut index = 0;
        while index < entries.len() {
            let owned = entries
                .get(index)
                .is_some_and(|table| owns_entry(receipt, array, table));
            if owned {
                entries.remove(index);
            } else {
                index += 1;
            }
        }
        if entries.is_empty() {
            document.remove(array);
        }
    }
    match &receipt.previous_active_model {
        Some(model) => set_active_model(&mut document, model),
        None => {
            document.remove(ACTIVE_MODEL);
        }
    }
    let rendered = document.to_string();
    let replacement = if receipt.created_file && rendered.trim().is_empty() {
        None
    } else {
        Some(rendered.into_bytes())
    };
    Ok(PreparedDocument {
        path: receipt.path.clone(),
        original,
        permissions,
        replacement,
        receipt: DocumentReceipt::VibeToml(receipt.clone()),
    })
}

pub(crate) fn vibe_receipt_is_active(document: &DocumentMut, receipt: &VibeReceipt) -> bool {
    document
        .get(ACTIVE_MODEL)
        .and_then(|item| hash_toml_item(item).ok())
        .is_some_and(|hash| hash == receipt.active_model_sha256)
        && receipt.entries.iter().all(|expected| {
            document
                .get(expected.array.as_str())
                .and_then(Item::as_array_of_tables)
                .and_then(|entries| {
                    entries
                        .iter()
                        .find(|table| entry_matches(&expected.array, table, &expected.name))
                })
                .and_then(|table| hash_toml_item(&Item::Table(table.clone())).ok())
                .is_some_and(|hash| hash == expected.sha256)
        })
}

fn parse_vibe_document(
    original: Option<&[u8]>,
    path: &Path,
) -> Result<DocumentMut, ConfigurationError> {
    let Some(contents) = original else {
        return Ok(DocumentMut::new());
    };
    let source =
        String::from_utf8(contents.to_vec()).map_err(|source| ConfigurationError::InvalidUtf8 {
            path: path.to_path_buf(),
            source,
        })?;
    if source.trim().is_empty() {
        return Ok(DocumentMut::new());
    }
    source
        .parse::<DocumentMut>()
        .map_err(|source| ConfigurationError::ParseToml {
            path: path.to_path_buf(),
            source,
        })
}

fn read_active_model(
    document: &DocumentMut,
    path: &Path,
) -> Result<Option<String>, ConfigurationError> {
    document
        .get(ACTIVE_MODEL)
        .map(|item| {
            item.as_str()
                .map(str::to_owned)
                .ok_or_else(|| ConfigurationError::TomlFieldNotString {
                    path: path.to_path_buf(),
                    field: ACTIVE_MODEL.to_owned(),
                })
        })
        .transpose()
}

/// Replaces the value while keeping the user's comment and spacing on that line.
fn set_active_model(document: &mut DocumentMut, model: &str) {
    if let Some(existing) = document.get_mut(ACTIVE_MODEL).and_then(Item::as_value_mut) {
        let decor = existing.decor().clone();
        *existing = toml_edit::Value::from(model);
        *existing.decor_mut() = decor;
    } else {
        document[ACTIVE_MODEL] = value(model);
    }
}

/// Whether `table` is the entry named `name` that nan-harness would write:
/// models are scoped to the `nan` provider so a same-named user model survives.
fn entry_matches(array: &str, table: &Table, name: &str) -> bool {
    table.get("name").and_then(Item::as_str) == Some(name)
        && (array != "models" || is_owned_model(table))
}

fn is_owned_model(table: &Table) -> bool {
    table.get("provider").and_then(Item::as_str) == Some(OWNED_PROVIDER)
}

fn owns_entry(receipt: &VibeReceipt, array: &str, table: &Table) -> bool {
    receipt
        .entries
        .iter()
        .any(|entry| entry.array == array && entry_matches(array, table, &entry.name))
}

fn existing_array<'a>(
    document: &'a DocumentMut,
    array: &str,
    path: &Path,
) -> Result<Option<&'a ArrayOfTables>, ConfigurationError> {
    match document.get(array) {
        None => Ok(None),
        Some(item) => item.as_array_of_tables().map(Some).ok_or_else(|| {
            ConfigurationError::TomlFieldNotTable {
                path: path.to_path_buf(),
                field: array.to_owned(),
            }
        }),
    }
}

/// Refuses to touch a provider, model or server that nan-harness did not
/// create: the same name, a model on the `nan` provider, or a colliding alias.
fn reject_unowned_conflicts(
    document: &DocumentMut,
    plan: &VibePlan,
    owned: &BTreeSet<(String, String)>,
) -> Result<(), ConfigurationError> {
    let conflict = || ConfigurationError::UnmanagedDocumentConflict(plan.path.clone());
    for array in ARRAYS {
        let Some(existing) = existing_array(document, array, &plan.path)? else {
            continue;
        };
        let desired = plan
            .entries
            .iter()
            .filter(|entry| entry.array == array)
            .collect::<Vec<_>>();
        for table in existing {
            let name = table.get("name").and_then(Item::as_str);
            let is_owned = name.is_some_and(|name| {
                owned.contains(&(array.to_owned(), name.to_owned()))
                    && entry_matches(array, table, name)
            });
            if is_owned {
                continue;
            }
            let collides = desired
                .iter()
                .any(|entry| entry_matches(array, table, &entry.name))
                || (array == "models"
                    && (is_owned_model(table)
                        || desired.iter().any(|entry| {
                            table.get("alias").and_then(Item::as_str) == Some(entry.name.as_str())
                        })));
            if collides {
                return Err(conflict());
            }
        }
    }
    Ok(())
}

fn apply_array(
    document: &mut DocumentMut,
    plan: &VibePlan,
    array: &str,
    owned: &BTreeSet<(String, String)>,
) -> Result<(), ConfigurationError> {
    let desired = plan
        .entries
        .iter()
        .filter(|entry| entry.array == array)
        .collect::<Vec<_>>();
    existing_array(document, array, &plan.path)?;
    if !document.contains_key(array) {
        if desired.is_empty() {
            return Ok(());
        }
        document[array] = Item::ArrayOfTables(ArrayOfTables::new());
    }
    let entries = document[array].as_array_of_tables_mut().ok_or_else(|| {
        ConfigurationError::TomlFieldNotTable {
            path: plan.path.clone(),
            field: array.to_owned(),
        }
    })?;
    let mut index = 0;
    while index < entries.len() {
        let stale = entries.get(index).is_some_and(|table| {
            owned.iter().any(|(owned_array, name)| {
                owned_array == array
                    && entry_matches(array, table, name)
                    && !desired.iter().any(|entry| &entry.name == name)
            })
        });
        if stale {
            entries.remove(index);
        } else {
            index += 1;
        }
    }
    for entry in desired {
        let mut table = entry.table.clone();
        let slot = entries
            .iter_mut()
            .find(|current| entry_matches(array, current, &entry.name));
        if let Some(slot) = slot {
            let decor = slot.decor().clone();
            *slot = table;
            *slot.decor_mut() = decor;
        } else {
            table.decor_mut().set_prefix("\n");
            entries.push(table);
        }
    }
    if entries.is_empty() {
        document.remove(array);
    }
    Ok(())
}
