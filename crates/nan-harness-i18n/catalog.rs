//! Build-time and repository-gate validation; never linked into the terminal binary.
use serde::Deserialize;
use serde::de::{self, MapAccess, Visitor};
use std::{collections::BTreeMap, fmt, fs, path::Path};

pub type Catalog = BTreeMap<String, Template>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Template {
    Text(String),
    Plural { one: String, other: String },
}

impl Template {
    pub fn variants(&self) -> Vec<&str> {
        match self {
            Self::Text(text) => vec![text],
            Self::Plural { one, other } => vec![one, other],
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocaleMetadata {
    pub name: String,
    pub native_name: String,
    pub plural_rule: String,
}

pub fn load_locales(path: &Path) -> Result<BTreeMap<String, LocaleMetadata>, String> {
    let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
    // Check duplicate keys before deriving the metadata representation.
    let _: Node = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    let locales: BTreeMap<String, LocaleMetadata> =
        serde_json::from_str(&text).map_err(|error| error.to_string())?;
    if !locales.contains_key("en") {
        return Err("English must be declared as the fallback language".to_owned());
    }
    for (code, metadata) in &locales {
        if !identifier(code)
            || !code.bytes().all(|byte| byte.is_ascii_lowercase())
            || metadata.name.is_empty()
            || metadata.native_name.is_empty()
            || metadata.plural_rule != "one-other"
        {
            return Err(format!("invalid locale metadata for {code}"));
        }
    }
    Ok(locales)
}

pub fn load(path: &Path) -> Result<Catalog, String> {
    let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
    parse(&text)
}

pub fn parse(text: &str) -> Result<Catalog, String> {
    let Node::Object(entries) = serde_json::from_str(text).map_err(|error| error.to_string())?
    else {
        return Err("a catalog must be an object".to_owned());
    };
    entries
        .into_iter()
        .map(|(key, value)| {
            let template = match value {
                Node::Text(text) => Template::Text(text),
                Node::Object(mut variants) => {
                    if variants.len() != 2 {
                        return Err(format!(
                            "{key}: plural messages require exactly one and other"
                        ));
                    }
                    let Some(Node::Text(one)) = variants.remove("one") else {
                        return Err(format!("{key}: missing text for one"));
                    };
                    let Some(Node::Text(other)) = variants.remove("other") else {
                        return Err(format!("{key}: missing text for other"));
                    };
                    Template::Plural { one, other }
                }
            };
            Ok((key, template))
        })
        .collect()
}

pub fn validate(source: &Catalog, translated: &Catalog, complete: bool) -> Result<(), String> {
    for key in translated.keys() {
        if !source.contains_key(key) {
            return Err(format!("unknown translation key: {key}"));
        }
    }
    for (key, template) in source {
        if !identifier(key) || source.contains_key(&format!("{key}_text")) {
            return Err(format!("invalid or colliding message key: {key}"));
        }
        let variants = template.variants();
        let names = parameters(variants[0]).map_err(|error| format!("{key}: {error}"))?;
        for text in &variants {
            validate_parameters(key, text, &names)?;
        }
        let Some(translation) = translated.get(key) else {
            if complete {
                return Err(format!("missing translation: {key}"));
            }
            continue;
        };
        let translated_variants = translation.variants();
        if variants.len() != translated_variants.len() {
            return Err(format!("{key}: plural variants differ"));
        }
        for text in translated_variants {
            validate_parameters(key, text, &names)?;
        }
    }
    Ok(())
}

fn validate_parameters(key: &str, text: &str, names: &[String]) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err(format!("{key}: empty message"));
    }
    if parameters(text).map_err(|error| format!("{key}: {error}"))? != names {
        return Err(format!("{key}: named parameters differ"));
    }
    Ok(())
}

pub fn parameters(message: &str) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    let mut characters = message.chars().peekable();
    while let Some(character) = characters.next() {
        if character != '{' && character != '}' {
            continue;
        }
        if characters.peek() == Some(&character) {
            characters.next();
            continue;
        }
        if character == '}' {
            return Err("unescaped closing brace".to_owned());
        }
        let mut name = String::new();
        loop {
            let Some(character) = characters.next() else {
                return Err("unclosed named parameter".to_owned());
            };
            if character == '}' {
                break;
            }
            name.push(character);
        }
        if !identifier(&name) || matches!(name.as_str(), "locale" | "quantity") {
            return Err(format!("invalid named parameter: {name}"));
        }
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().enumerate().all(|(index, byte)| {
            byte == b'_' || byte.is_ascii_lowercase() || (index > 0 && byte.is_ascii_digit())
        })
        && !matches!(
            value,
            "as" | "async"
                | "await"
                | "break"
                | "const"
                | "continue"
                | "crate"
                | "dyn"
                | "else"
                | "enum"
                | "extern"
                | "false"
                | "fn"
                | "for"
                | "if"
                | "impl"
                | "in"
                | "let"
                | "loop"
                | "match"
                | "mod"
                | "move"
                | "mut"
                | "pub"
                | "ref"
                | "return"
                | "self"
                | "static"
                | "struct"
                | "super"
                | "trait"
                | "true"
                | "type"
                | "unsafe"
                | "use"
                | "where"
                | "while"
        )
}

#[derive(Debug)]
enum Node {
    Text(String),
    Object(BTreeMap<String, Node>),
}

impl<'de> Deserialize<'de> for Node {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct NodeVisitor;
        impl<'de> Visitor<'de> for NodeVisitor {
            type Value = Node;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("text or an object with unique keys")
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Node, E> {
                Ok(Node::Text(value.to_owned()))
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Node, M::Error> {
                let mut entries = BTreeMap::new();
                while let Some((key, value)) = map.next_entry::<String, Node>()? {
                    if entries.insert(key.clone(), value).is_some() {
                        return Err(de::Error::custom(format!("duplicate key: {key}")));
                    }
                }
                Ok(Node::Object(entries))
            }
        }
        deserializer.deserialize_any(NodeVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::{parameters, parse, validate};

    #[test]
    fn malformed_templates_and_duplicate_keys_are_rejected() {
        for message in ["{", "}", "{name:>8}", "{name.call()}", "{locale}", "{0}"] {
            assert!(parameters(message).is_err(), "{message}");
        }
        assert_eq!(parameters("{{literal}} {name} {name}").unwrap(), ["name"]);
        assert!(parse(r#"{"key":"one","key":"two"}"#).is_err());
        assert!(parse(r#"{"key":{"one":"one","one":"two","other":"many"}}"#).is_err());
    }

    #[test]
    fn translation_validation_checks_coverage_and_each_plural_variant() {
        let source = parse(r#"{"count":{"one":"{count} item","other":"{count} items"}}"#).unwrap();
        let missing = parse("{}").unwrap();
        assert!(validate(&source, &missing, false).is_ok());
        assert!(validate(&source, &missing, true).is_err());
        for text in [
            r#"{"count":"{count} cosas"}"#,
            r#"{"count":{"one":"{count} cosa","other":"cosas"}}"#,
            r#"{"extra":"extra"}"#,
        ] {
            assert!(validate(&source, &parse(text).unwrap(), true).is_err());
        }
        let translated =
            parse(r#"{"count":{"one":"{count} cosa","other":"{count} cosas"}}"#).unwrap();
        assert!(validate(&source, &translated, true).is_ok());
    }
}
