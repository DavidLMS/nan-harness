mod catalog;

use catalog::{Catalog, LocaleMetadata, Template};
use std::{
    collections::BTreeMap,
    env,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

fn main() {
    println!("cargo:rerun-if-changed=locales");
    println!("cargo:rerun-if-changed=locales.json");
    println!("cargo:rerun-if-changed=catalog.rs");
    let locales = catalog::load_locales(Path::new("locales.json")).expect("valid locale metadata");
    let english = catalog::load(Path::new("locales/en.json")).expect("valid English catalog");
    let mut catalogs = BTreeMap::new();
    for code in locales.keys() {
        let translated = catalog::load(&PathBuf::from(format!("locales/{code}.json")))
            .expect("valid message catalog");
        catalog::validate(&english, &translated, false)
            .expect("matching named parameters and variants");
        catalogs.insert(code.clone(), translated);
    }
    let mut output = String::new();
    for (key, source) in &english {
        emit_message(&mut output, key, source, &locales, &catalogs);
    }
    output.push_str("#[cfg(test)] pub mod fallback_fixture {\n");
    emit_message(
        &mut output,
        "probe",
        &Template::Text("English fallback: {value}".to_owned()),
        &locales,
        &BTreeMap::new(),
    );
    output.push_str("}\n");
    let destination = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR"));
    fs::write(destination.join("messages.rs"), output).expect("write compiled messages");
    fs::write(destination.join("locales.rs"), emit_locales(&locales))
        .expect("write compiled locales");
}

fn emit_message(
    output: &mut String,
    key: &str,
    source: &Template,
    locales: &BTreeMap<String, LocaleMetadata>,
    catalogs: &BTreeMap<String, Catalog>,
) {
    let variants = source.variants();
    let parameters = catalog::parameters(variants[0]).expect("validated named parameters");
    let mut signature = String::new();
    for name in &parameters {
        write!(signature, ", {name}: &(impl std::fmt::Display + ?Sized)").expect("write to string");
    }
    let selector = if variants.len() == 2 {
        ", quantity: u64"
    } else {
        ""
    };
    let documentation = format!("English source:\n```text\n{}\n```", variants[0]);
    let static_text = parameters.is_empty() && variants.len() == 1;
    if static_text {
        writeln!(output, "#[doc = {documentation:?}]\n#[must_use]\npub const fn {key}_text(locale: crate::Locale) -> &'static str {{ match locale {{").expect("write to string");
        emit_arms(output, locales, |code| {
            let text = translation(catalogs, code, key, source).variants()[0]
                .replace("{{", "{")
                .replace("}}", "}");
            format!("{text:?}")
        });
        output.push_str("}}\n");
    }
    if key == "search_nan_web_search_is_enabled_at_mode_version_state_interested_sessions_problem" {
        output.push_str("#[expect(clippy::too_many_arguments, reason = \"one complete status message retains all eight named fields\")]\n");
    }
    writeln!(output, "#[doc = {documentation:?}]\n#[must_use]\npub fn {key}(locale: crate::Locale{selector}{signature}) -> String {{").expect("write to string");
    if static_text {
        writeln!(output, "{key}_text(locale).to_owned()\n}}").expect("write to string");
        return;
    }
    output.push_str("match locale {\n");
    emit_arms(output, locales, |code| {
        render(&translation(catalogs, code, key, source).variants())
    });
    output.push_str("}\n}\n");
}

fn emit_arms(
    output: &mut String,
    locales: &BTreeMap<String, LocaleMetadata>,
    expression: impl Fn(&str) -> String,
) {
    let mut groups = BTreeMap::<String, Vec<String>>::new();
    for code in locales.keys() {
        groups
            .entry(expression(code))
            .or_default()
            .push(format!("crate::Locale::{}", variant(code)));
    }
    for (body, patterns) in groups {
        writeln!(output, "{} => {body},", patterns.join(" | ")).expect("write to string");
    }
}

fn translation<'a>(
    catalogs: &'a BTreeMap<String, Catalog>,
    code: &str,
    key: &str,
    source: &'a Template,
) -> &'a Template {
    catalogs
        .get(code)
        .and_then(|catalog| catalog.get(key))
        .unwrap_or(source)
}

fn render(variants: &[&str]) -> String {
    let expression = |text: &str| {
        if catalog::parameters(text)
            .expect("validated parameters")
            .is_empty()
        {
            let literal = text.replace("{{", "{").replace("}}", "}");
            format!("{literal:?}.to_owned()")
        } else {
            format!("format!({text:?})")
        }
    };
    if variants.len() == 2 {
        format!(
            "if quantity == 1 {{ {} }} else {{ {} }}",
            expression(variants[0]),
            expression(variants[1])
        )
    } else {
        expression(variants[0])
    }
}

fn variant(code: &str) -> String {
    let mut value = code.to_owned();
    value[..1].make_ascii_uppercase();
    value
}

fn emit_locales(locales: &BTreeMap<String, LocaleMetadata>) -> String {
    let mut output = "/// Languages declared by the bundled locale metadata.\n#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]\npub enum Locale {\n".to_owned();
    for code in locales.keys() {
        if code == "en" {
            output.push_str("#[default]\n");
        }
        writeln!(output, "{},", variant(code)).expect("write to string");
    }
    output.push_str(
        "}\nimpl Locale {\n/// All bundled languages.\npub const ALL: &'static [Self] = &[\n",
    );
    for code in locales.keys() {
        writeln!(output, "Self::{},", variant(code)).expect("write to string");
    }
    output.push_str("] ;\n/// Parse an explicit language code.\n#[must_use]\npub fn parse(value: &str) -> Option<Self> { match value {\n");
    for code in locales.keys() {
        writeln!(output, "{code:?} => Some(Self::{}),", variant(code)).expect("write to string");
    }
    output.push_str("_ => None,\n}}\n/// Stable preference code.\n#[must_use]\npub const fn code(self) -> &'static str { match self {\n");
    for code in locales.keys() {
        writeln!(output, "Self::{} => {code:?},", variant(code)).expect("write to string");
    }
    output.push_str("}}\n/// Language name as written by its speakers.\n#[must_use]\npub const fn native_name(self) -> &'static str { match self {\n");
    for (code, metadata) in locales {
        writeln!(
            output,
            "Self::{} => {:?},",
            variant(code),
            metadata.native_name
        )
        .expect("write to string");
    }
    output.push_str("}}\n}\n");
    output
}
