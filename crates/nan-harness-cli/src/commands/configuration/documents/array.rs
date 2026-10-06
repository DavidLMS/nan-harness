use super::{
    BTreeMap, ConfigurationError, JsonEntryReceipt, Path, Value, get_json_path, remove_json_path,
    set_json_path,
};

type Selector = BTreeMap<String, String>;

fn matches(value: &Value, selector: &Selector) -> bool {
    !selector.is_empty()
        && selector
            .iter()
            .all(|(key, expected)| value.get(key).and_then(Value::as_str) == Some(expected))
}

pub(super) fn get_json_entry<'a>(
    document: &'a Value,
    path: &[String],
    selector: Option<&Selector>,
    file: &Path,
) -> Result<Option<&'a Value>, ConfigurationError> {
    let value = get_json_path(document, path);
    let Some(selector) = selector else {
        return Ok(value);
    };
    let Some(value) = value else {
        return Ok(None);
    };
    let array = value
        .as_array()
        .ok_or_else(|| ConfigurationError::DocumentFieldNotArray {
            path: file.to_path_buf(),
            field: path.last().cloned().unwrap_or_default(),
        })?;
    let mut found = array.iter().filter(|value| matches(value, selector));
    let first = found.next();
    if found.next().is_some() || selector.is_empty() {
        return Err(ConfigurationError::UnmanagedDocumentConflict(
            file.to_path_buf(),
        ));
    }
    Ok(first)
}

pub(super) fn set_json_entry(
    document: &mut Value,
    path: &[String],
    selector: Option<&Selector>,
    value: Value,
    file: &Path,
) -> Result<(), ConfigurationError> {
    let Some(selector) = selector else {
        return set_json_path(document, path, value, file);
    };
    if !matches(&value, selector) {
        return Err(ConfigurationError::InvalidManagedPath);
    }
    get_json_entry(document, path, Some(selector), file)?;
    let mut array = get_json_path(document, path)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if let Some(index) = array.iter().position(|item| matches(item, selector)) {
        array[index] = value;
    } else {
        array.push(value);
    }
    set_json_path(document, path, Value::Array(array), file)
}

pub(super) fn restore_json_entry(
    document: &mut Value,
    entry: &JsonEntryReceipt,
    file: &Path,
) -> Result<(), ConfigurationError> {
    if entry.container_only {
        if entry.previous.is_none()
            && get_json_path(document, &entry.path)
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty)
        {
            remove_json_path(document, &entry.path);
        }
        return Ok(());
    }
    if let Some(previous) = &entry.previous {
        return set_json_entry(
            document,
            &entry.path,
            entry.selector.as_ref(),
            previous.clone(),
            file,
        );
    }
    let Some(selector) = &entry.selector else {
        remove_json_path(document, &entry.path);
        return Ok(());
    };
    get_json_entry(document, &entry.path, Some(selector), file)?;
    let mut array = get_json_path(document, &entry.path)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    array.retain(|value| !matches(value, selector));
    if array.is_empty() && entry.created_array {
        remove_json_path(document, &entry.path);
        Ok(())
    } else {
        set_json_path(document, &entry.path, Value::Array(array), file)
    }
}
