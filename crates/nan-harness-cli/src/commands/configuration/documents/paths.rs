use super::*;

pub(crate) fn set_json_path(
    document: &mut Value,
    path: &[String],
    value: Value,
    document_path: &Path,
) -> Result<(), ConfigurationError> {
    let Some((last, parents)) = path.split_last() else {
        return Err(ConfigurationError::InvalidManagedPath);
    };
    let mut current = document;
    for segment in parents {
        let object =
            current
                .as_object_mut()
                .ok_or_else(|| ConfigurationError::DocumentFieldNotObject {
                    path: document_path.to_path_buf(),
                    field: segment.clone(),
                })?;
        current = object
            .entry(segment.clone())
            .or_insert_with(|| Value::Object(Map::new()));
    }
    current
        .as_object_mut()
        .ok_or_else(|| ConfigurationError::DocumentFieldNotObject {
            path: document_path.to_path_buf(),
            field: last.clone(),
        })?
        .insert(last.clone(), value);
    Ok(())
}

pub(crate) fn get_json_path<'a>(document: &'a Value, path: &[String]) -> Option<&'a Value> {
    path.iter().try_fold(document, |current, segment| {
        current.as_object()?.get(segment)
    })
}

pub(crate) fn remove_json_path(document: &mut Value, path: &[String]) {
    if path.is_empty() {
        return;
    }
    remove_json_path_inner(document, path);
}

pub(crate) fn remove_json_path_inner(document: &mut Value, path: &[String]) -> bool {
    let Some(object) = document.as_object_mut() else {
        return false;
    };
    if path.len() == 1 {
        object.remove(&path[0]);
    } else if let Some(child) = object.get_mut(&path[0])
        && remove_json_path_inner(child, &path[1..])
    {
        object.remove(&path[0]);
    }
    object.is_empty()
}

pub(crate) fn set_yaml_path(
    document: &mut YamlValue,
    path: &[String],
    value: YamlValue,
    document_path: &Path,
) -> Result<(), ConfigurationError> {
    let Some((last, parents)) = path.split_last() else {
        return Err(ConfigurationError::InvalidManagedPath);
    };
    let mut current = document;
    for segment in parents {
        let mapping =
            current
                .as_mapping_mut()
                .ok_or_else(|| ConfigurationError::YamlFieldNotMapping {
                    path: document_path.to_path_buf(),
                    field: segment.clone(),
                })?;
        current = mapping
            .entry(YamlValue::String(segment.clone()))
            .or_insert_with(|| YamlValue::Mapping(serde_yaml_ng::Mapping::default()));
    }
    current
        .as_mapping_mut()
        .ok_or_else(|| ConfigurationError::YamlFieldNotMapping {
            path: document_path.to_path_buf(),
            field: last.clone(),
        })?
        .insert(YamlValue::String(last.clone()), value);
    Ok(())
}

pub(crate) fn get_yaml_path<'a>(document: &'a YamlValue, path: &[String]) -> Option<&'a YamlValue> {
    path.iter().try_fold(document, |current, segment| {
        current
            .as_mapping()?
            .get(YamlValue::String(segment.clone()))
    })
}

pub(crate) fn remove_yaml_path(document: &mut YamlValue, path: &[String]) {
    if path.is_empty() {
        return;
    }
    remove_yaml_path_inner(document, path);
}

pub(crate) fn remove_yaml_path_inner(document: &mut YamlValue, path: &[String]) -> bool {
    let Some(mapping) = document.as_mapping_mut() else {
        return false;
    };
    let key = YamlValue::String(path[0].clone());
    if path.len() == 1 {
        mapping.remove(&key);
    } else if let Some(child) = mapping.get_mut(&key)
        && remove_yaml_path_inner(child, &path[1..])
    {
        mapping.remove(&key);
    }
    mapping.is_empty()
}

pub(crate) fn append_unique_yaml_value(
    current: Option<&YamlValue>,
    value: &YamlValue,
    path: &Path,
    field: &str,
) -> Result<YamlValue, ConfigurationError> {
    let mut values = match current {
        Some(YamlValue::Sequence(values)) => values.clone(),
        Some(_) => {
            return Err(ConfigurationError::YamlFieldNotSequence {
                path: path.to_path_buf(),
                field: field.to_owned(),
            });
        }
        None => Vec::new(),
    };
    if !values.contains(value) {
        values.push(value.clone());
    }
    Ok(YamlValue::Sequence(values))
}

pub(crate) fn append_unique_json_value(
    current: Option<&Value>,
    value: &Value,
    path: &Path,
    field: &str,
) -> Result<Value, ConfigurationError> {
    let mut values = match current {
        Some(Value::Array(values)) => values.clone(),
        Some(_) => {
            return Err(ConfigurationError::DocumentFieldNotArray {
                path: path.to_path_buf(),
                field: field.to_owned(),
            });
        }
        None => Vec::new(),
    };
    if !values.contains(value) {
        values.push(value.clone());
    }
    Ok(Value::Array(values))
}

pub(crate) fn hash_yaml(value: &YamlValue) -> Result<String, ConfigurationError> {
    serde_yaml_ng::to_string(value)
        .map(|rendered| sha256(rendered.as_bytes()))
        .map_err(ConfigurationError::SerializeYaml)
}
