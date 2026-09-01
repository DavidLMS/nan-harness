#[allow(clippy::wildcard_imports)]
use super::*;

pub(super) fn replace_top_level_block(
    source: &str,
    key: &str,
    replacement: &str,
) -> Result<String, HermesDesktopError> {
    let lines = source.lines().collect::<Vec<_>>();
    let prefix = format!("{key}:");
    let mut start = None;
    let mut end = lines.len();
    for (index, line) in lines.iter().enumerate() {
        if line.starts_with(&prefix) {
            if line.trim() != prefix {
                return Err(HermesDesktopError::UnsupportedProfileConfig(key.to_owned()));
            }
            if start.replace(index).is_some() {
                return Err(HermesDesktopError::UnsupportedProfileConfig(key.to_owned()));
            }
            continue;
        }
        if start.is_some() && !line.is_empty() && !line.starts_with(char::is_whitespace) {
            end = index;
            break;
        }
    }
    let mut output = Vec::new();
    if let Some(start) = start {
        output.extend_from_slice(&lines[..start]);
        output.extend(replacement.lines());
        output.extend_from_slice(&lines[end..]);
    } else {
        output.extend_from_slice(&lines);
        if !output.is_empty() && !output.last().is_some_and(|line| line.is_empty()) {
            output.push("");
        }
        output.extend(replacement.lines());
    }
    Ok(format!("{}\n", output.join("\n")))
}

pub(super) fn replace_provider_entry(
    source: &str,
    provider: &str,
    replacement: &str,
) -> Result<String, HermesDesktopError> {
    let lines = source.lines().collect::<Vec<_>>();
    let providers_start = lines.iter().position(|line| line.starts_with("providers:"));
    let Some(providers_start) = providers_start else {
        let mut output = source.trim_end().to_owned();
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        output.push_str("providers:\n");
        output.push_str(replacement);
        output.push('\n');
        return Ok(output);
    };
    if lines[providers_start] != "providers:" {
        return Err(HermesDesktopError::UnsupportedProfileConfig(
            "providers".to_owned(),
        ));
    }
    if lines
        .iter()
        .skip(providers_start + 1)
        .any(|line| line.starts_with("providers:"))
    {
        return Err(HermesDesktopError::UnsupportedProfileConfig(
            "providers".to_owned(),
        ));
    }
    let providers_end = lines
        .iter()
        .enumerate()
        .skip(providers_start + 1)
        .find(|(_, line)| !line.is_empty() && !line.starts_with(char::is_whitespace))
        .map_or(lines.len(), |(index, _)| index);
    let target = format!("  {provider}:");
    let entry_start = lines[providers_start + 1..providers_end]
        .iter()
        .position(|line| line.starts_with(&target))
        .map(|index| providers_start + 1 + index);
    let mut output = Vec::new();
    if let Some(entry_start) = entry_start {
        if lines[entry_start] != target {
            return Err(HermesDesktopError::UnsupportedProfileConfig(format!(
                "providers.{provider}"
            )));
        }
        let entry_end = lines
            .iter()
            .enumerate()
            .take(providers_end)
            .skip(entry_start + 1)
            .find(|(_, line)| {
                !line.is_empty()
                    && (line.starts_with("  ") && !line.starts_with("   ")
                        || !line.starts_with(char::is_whitespace))
            })
            .map_or(providers_end, |(index, _)| index);
        output.extend_from_slice(&lines[..entry_start]);
        output.extend(replacement.lines());
        output.extend_from_slice(&lines[entry_end..]);
    } else {
        output.extend_from_slice(&lines[..providers_end]);
        output.extend(replacement.lines());
        output.extend_from_slice(&lines[providers_end..]);
    }
    Ok(format!("{}\n", output.join("\n")))
}

pub(super) fn yaml_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_owned())
}
