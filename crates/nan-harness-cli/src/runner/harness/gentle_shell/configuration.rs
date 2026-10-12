mod value;
use super::{CliError, CredentialError, SecretValue};
use nan_harness_core::PlanError;
use nan_harness_i18n::{DiagnosticText, Locale, messages};
use serde_json::Value;
use std::path::Path;

pub(super) struct NativeConfiguration {
    pub key: Option<SecretValue>,
    pub base_url: Option<String>,
}

pub(super) fn invalid(field: &'static str, message: fn(Locale) -> String) -> CliError {
    CliError::InvalidPlan(PlanError::InvalidField {
        field,
        message: DiagnosticText::new(message),
    })
}

pub(super) fn read_json(path: &Path) -> Result<Value, CliError> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|_| invalid("gentle-shell", messages::detail_gentle_shell_config)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Value::Null),
        Err(_) => Err(invalid(
            "gentle-shell",
            messages::detail_gentle_shell_config,
        )),
    }
}

pub(super) fn default_model(home: &Path, cwd: &Path) -> Result<Option<String>, CliError> {
    let global = read_json(&home.join("settings.json"))?;
    let project = read_json(&cwd.join(".pi/settings.json"))?;
    let provider = project
        .get("defaultProvider")
        .unwrap_or(&global["defaultProvider"]);
    let model = project
        .get("defaultModel")
        .unwrap_or(&global["defaultModel"]);
    Ok((provider == "nan")
        .then(|| model.as_str().map(str::to_owned))
        .flatten())
}

pub(super) fn native_configuration(
    home: &Path,
    direct: bool,
    dry_run: bool,
) -> Result<NativeConfiguration, CliError> {
    if dry_run {
        return Ok(NativeConfiguration {
            key: None,
            base_url: None,
        });
    }
    let auth = read_json(&home.join("auth.json"))?;
    let models = read_json(&home.join("models.json"))?;
    let key = resolve_key(&auth, &models, direct, |name| std::env::var(name).ok())?;
    let base_url = models["providers"]["nan"]["baseUrl"]
        .as_str()
        .map(str::to_owned);
    Ok(NativeConfiguration { key, base_url })
}

fn resolve_key(
    auth: &Value,
    models: &Value,
    direct: bool,
    environment: impl Fn(&str) -> Option<String>,
) -> Result<Option<SecretValue>, CliError> {
    let explicit = environment("NAN_API_KEY").filter(|key| !key.trim().is_empty());
    if !direct && let Some(key) = explicit.as_ref() {
        return secret(key.clone()).map(Some);
    }
    let stored = &auth["nan"];
    if !stored.is_null() && stored["type"] != "api_key" {
        return Err(invalid(
            "gentle-shell",
            messages::detail_gentle_shell_credential,
        ));
    }
    let configured = stored
        .get("key")
        .or_else(|| models["providers"]["nan"].get("apiKey"));
    let value = if let Some(value) = configured {
        let value = value
            .as_str()
            .ok_or_else(|| invalid("gentle-shell", messages::detail_gentle_shell_credential))?;
        if let Some(command) = value.strip_prefix('!') {
            return nan_harness_runtime::resolve_gentle_shell_command(command)
                .map(Some)
                .ok_or_else(|| invalid("gentle-shell", messages::detail_gentle_shell_credential));
        }
        Some(
            value::expand(value, |name| {
                stored["env"][name]
                    .as_str()
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
                    .or_else(|| environment(name))
            })
            .ok_or_else(|| invalid("gentle-shell", messages::detail_gentle_shell_credential))?,
        )
    } else {
        explicit
    };
    value.map(secret).transpose()
}

fn secret(value: String) -> Result<SecretValue, CliError> {
    if value.trim().is_empty() {
        return Err(invalid(
            "gentle-shell",
            messages::detail_gentle_shell_credential,
        ));
    }
    SecretValue::new(value)
        .map_err(CredentialError::Secret)
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn native_account_wins_in_direct_mode_but_explicit_key_wins_with_gateway() {
        let auth = json!({"nan": {"type": "api_key", "key": "native-key"}});
        for (direct, expected) in [(true, "native-key"), (false, "explicit-key")] {
            let key = resolve_key(&auth, &Value::Null, direct, |name| {
                (name == "NAN_API_KEY").then(|| "explicit-key".into())
            })
            .unwrap()
            .unwrap();
            key.with_secret(|value| assert_eq!(value, expected));
        }
    }

    #[test]
    fn absent_native_configuration_allows_onboarding_but_invalid_does_not() {
        assert!(
            resolve_key(&Value::Null, &Value::Null, true, |_| None)
                .unwrap()
                .is_none()
        );
        for key in [json!(""), json!(42)] {
            assert!(
                resolve_key(
                    &json!({"nan": {"type": "api_key", "key": key}}),
                    &Value::Null,
                    true,
                    |_| None
                )
                .is_err()
            );
        }
    }

    #[test]
    fn native_environment_reference_is_resolved_without_mutation() {
        let auth = json!({"nan": {"type": "api_key", "key": "${MY_NAN_KEY}"}});
        let key = resolve_key(&auth, &Value::Null, true, |name| {
            (name == "MY_NAN_KEY").then(|| "native-secret".into())
        })
        .unwrap()
        .unwrap();
        key.with_secret(|value| assert_eq!(value, "native-secret"));
    }
}
