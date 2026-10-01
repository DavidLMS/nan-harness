use std::{fs, path::Path};
use syn::{Expr, Lit, Meta, Token, parse::Parser as _, punctuated::Punctuated, visit::Visit};

pub(super) fn check(repository: &Path) -> Result<(), String> {
    for directory in [
        "nan-harness-cli",
        "nan-harness-runtime",
        "nan-harness-telemetry",
    ] {
        check_directory(&repository.join("crates").join(directory).join("src"))?;
    }
    Ok(())
}

fn check_directory(directory: &Path) -> Result<(), String> {
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path
            .file_name()
            .is_some_and(|name| name == "tests" || name == "tests.rs")
        {
            continue;
        }
        if path.is_dir() {
            check_directory(&path)?;
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let text = fs::read_to_string(&path).map_err(|error| error.to_string())?;
            check_source(&path, &text)?;
        }
    }
    Ok(())
}

fn check_source(path: &Path, text: &str) -> Result<(), String> {
    let file = syn::parse_file(text).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut visitor = Surface {
        path,
        literals: Vec::new(),
    };
    visitor.visit_file(&file);
    if visitor.literals.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{}: move direct terminal/help literals into the catalog: {:?}",
            path.display(),
            visitor.literals
        ))
    }
}

struct Surface<'a> {
    path: &'a Path,
    literals: Vec<String>,
}

impl Surface<'_> {
    fn inspect(&mut self, text: &str) {
        if contains_words(text) && !machine_template(self.path, text) {
            self.literals.push(text.to_owned());
        }
    }
}

impl<'ast> Visit<'ast> for Surface<'_> {
    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        if module.ident != "tests" {
            syn::visit::visit_item_mod(self, module);
        }
    }

    fn visit_item_impl(&mut self, implementation: &'ast syn::ItemImpl) {
        // Canonical Display is deliberately English for protocols and diagnostics.
        if implementation.trait_.as_ref().is_none_or(|(_, path, _)| {
            path.segments
                .last()
                .is_none_or(|segment| segment.ident != "Display")
        }) {
            syn::visit::visit_item_impl(self, implementation);
        }
    }

    fn visit_macro(&mut self, invocation: &'ast syn::Macro) {
        let Some(name) = invocation.path.get_ident().map(ToString::to_string) else {
            return;
        };
        let index = match name.as_str() {
            "print" | "println" | "eprint" | "eprintln" => 0,
            "write" | "writeln" | "append_report_line" => 1,
            _ => return,
        };
        let Ok(arguments) =
            Punctuated::<Expr, Token![,]>::parse_terminated.parse2(invocation.tokens.clone())
        else {
            return;
        };
        if let Some(Expr::Lit(literal)) = arguments.iter().nth(index)
            && let Lit::Str(text) = &literal.lit
        {
            self.inspect(&text.value());
        }
    }

    fn visit_attribute(&mut self, attribute: &'ast syn::Attribute) {
        if !attribute.path().is_ident("arg") && !attribute.path().is_ident("command") {
            return;
        }
        let Ok(arguments) =
            attribute.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
        else {
            return;
        };
        for argument in arguments {
            if let Meta::NameValue(value) = argument
                && [
                    "help",
                    "long_help",
                    "about",
                    "long_about",
                    "after_help",
                    "before_help",
                ]
                .iter()
                .any(|name| value.path.is_ident(name))
                && let Expr::Lit(literal) = value.value
                && let Lit::Str(text) = literal.lit
            {
                self.inspect(&text.value());
            }
        }
    }
}

fn contains_words(template: &str) -> bool {
    let mut placeholder = false;
    let mut letters = 0;
    for character in template.chars() {
        match character {
            '{' => {
                placeholder = true;
                letters = 0;
            }
            '}' => {
                placeholder = false;
                letters = 0;
            }
            _ if !placeholder && character.is_alphabetic() => {
                letters += 1;
                if letters >= 2 {
                    return true;
                }
            }
            _ => letters = 0,
        }
    }
    false
}

fn machine_template(path: &Path, template: &str) -> bool {
    // These macros serialize machine JSON or native configuration, never terminal prose.
    if template == "nan-harness" {
        return true;
    }
    if path.ends_with("commands/doctor/json.rs") {
        return template
            == r#"{{"schemaVersion":{DOCTOR_SCHEMA_VERSION},"offline":{offline},"harness":"{kind}","level":"error","safeToShare":true}}"#;
    }
    (path.ends_with("commands/persistence/models.rs")
        && (template.starts_with("        - id:") || template.starts_with("- name:")))
        || (path.ends_with("prepared/catalogs/structured.rs")
            && template.starts_with("          - id:"))
}

#[cfg(test)]
mod tests {
    use super::check_source;
    use std::path::Path;

    #[test]
    fn direct_terminal_and_clap_literals_are_rejected_but_values_and_machine_display_are_allowed() {
        for source in [
            r#"fn run() { println!("An English message"); }"#,
            r#"#[arg(help = "English help")] struct Args;"#,
        ] {
            assert!(check_source(Path::new("terminal.rs"), source).is_err());
        }
        for source in [
            r#"fn run() { println!("{}", messages::saved(locale())); }"#,
            r#"impl std::fmt::Display for Error { fn fmt(&self, f: &mut Formatter) { write!(f, "canonical message"); } }"#,
            r#"#[cfg(test)] mod tests { fn fixture() { println!("fake child output"); } }"#,
        ] {
            assert!(check_source(Path::new("terminal.rs"), source).is_ok());
        }
    }
}
