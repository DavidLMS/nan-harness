use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::collections::BTreeMap;
use std::fmt;
use thiserror::Error;
use zeroize::Zeroizing;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SecretRef(String);

impl SecretRef {
    /// Creates a validated symbolic secret reference.
    ///
    /// # Errors
    ///
    /// Returns [`SecretError`] when the value is not a valid reference name.
    pub fn new(value: impl Into<String>) -> Result<Self, SecretError> {
        let value = value.into();
        if is_valid_secret_ref(&value) {
            Ok(Self(value))
        } else {
            Err(SecretError::InvalidReference(value))
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("SecretRef").field(&self.0).finish()
    }
}

impl fmt::Display for SecretRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for SecretRef {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for SecretRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(de::Error::custom)
    }
}

pub struct SecretValue(Zeroizing<String>);

impl SecretValue {
    /// Wraps a non-empty secret value in zeroizing storage.
    ///
    /// # Errors
    ///
    /// Returns [`SecretError`] when the supplied value is empty.
    pub fn new(value: impl Into<String>) -> Result<Self, SecretError> {
        let value = value.into();
        if value.is_empty() {
            Err(SecretError::EmptyValue)
        } else {
            Ok(Self(Zeroizing::new(value)))
        }
    }

    pub fn with_secret<T>(&self, operation: impl FnOnce(&str) -> T) -> T {
        operation(self.0.as_str())
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretValue([REDACTED])")
    }
}

#[derive(Default)]
pub struct SecretStore {
    values: BTreeMap<SecretRef, SecretValue>,
}

impl SecretStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, reference: SecretRef, value: SecretValue) {
        self.values.insert(reference, value);
    }

    #[must_use]
    pub fn contains(&self, reference: &SecretRef) -> bool {
        self.values.contains_key(reference)
    }

    /// Gives a closure temporary access to one secret value.
    ///
    /// # Errors
    ///
    /// Returns [`SecretError`] when the reference is absent from the store.
    pub fn with_secret<T>(
        &self,
        reference: &SecretRef,
        operation: impl FnOnce(&str) -> T,
    ) -> Result<T, SecretError> {
        self.values
            .get(reference)
            .map(|value| value.with_secret(operation))
            .ok_or_else(|| SecretError::MissingReference(reference.to_string()))
    }
}

impl fmt::Debug for SecretStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SecretStore")
            .field("references", &self.values.keys().collect::<Vec<_>>())
            .finish()
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SecretError {
    #[error("secret references must match ^[a-z][a-z0-9_]{{2,63}}$")]
    InvalidReference(String),
    #[error("secret values cannot be empty")]
    EmptyValue,
    #[error("secret reference '{0}' is not available")]
    MissingReference(String),
}

fn is_valid_secret_ref(value: &str) -> bool {
    let mut characters = value.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    value.len() <= 64
        && value.len() >= 3
        && first.is_ascii_lowercase()
        && characters.all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
        })
}
