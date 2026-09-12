#[path = "../../crates/nan-harness-i18n/catalog.rs"]
mod catalog;
mod surface;

use std::path::Path;

pub fn check() -> Result<(), String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| "xtask manifest has no repository parent".to_owned())?
        .join("crates/nan-harness-i18n");
    let locales = catalog::load_locales(&root.join("locales.json"))?;
    let source = catalog::load(&root.join("locales/en.json"))?;
    let contexts: serde_json::Map<String, serde_json::Value> = serde_json::from_str(
        &std::fs::read_to_string(root.join("contexts.json")).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    if source.keys().ne(contexts.keys()) {
        return Err("message contexts must cover exactly the English catalog".to_owned());
    }
    for (key, context) in contexts {
        for field in ["section", "description"] {
            if context
                .get(field)
                .and_then(serde_json::Value::as_str)
                .is_none_or(str::is_empty)
            {
                return Err(format!("{key}: missing context {field}"));
            }
        }
    }
    for code in locales.keys() {
        let translated = catalog::load(&root.join(format!("locales/{code}.json")))?;
        catalog::validate(&source, &translated, true)
            .map_err(|error| format!("{code}: {error}"))?;
    }
    surface::check(
        root.parent()
            .and_then(Path::parent)
            .ok_or("missing repository root")?,
    )?;
    println!(
        "Validated {} messages in {} languages",
        source.len(),
        locales.len()
    );
    Ok(())
}
