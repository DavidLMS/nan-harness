//! Compiled terminal messages. Protocols and diagnostic payloads stay language independent.
use std::sync::OnceLock;

include!(concat!(env!("OUT_DIR"), "/locales.rs"));

static LOCALE: OnceLock<Locale> = OnceLock::new();

/// Set the process language once, before parsing public terminal commands.
pub fn initialize(locale: Locale) {
    let _ = LOCALE.set(locale);
}

/// English is the default; environment locale variables are deliberately ignored.
#[must_use]
pub fn locale() -> Locale {
    LOCALE.get().copied().unwrap_or_default()
}

pub mod messages {
    include!(concat!(env!("OUT_DIR"), "/messages.rs"));
}

/// Interpret an explicit yes/no answer without changing the caller's default.
#[must_use]
pub fn yes_no(locale: Locale, answer: &str) -> Option<bool> {
    match answer.trim().to_lowercase().as_str() {
        "y" | "yes" => Some(true),
        "s" | "si" | "sí" if locale == Locale::Es => Some(true),
        "n" | "no" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{Locale, messages, yes_no};

    #[test]
    fn explicit_answers_preserve_negative_and_unknown_input() {
        for answer in ["s", "SI", " SÍ\n"] {
            assert_eq!(yes_no(Locale::Es, answer), Some(true));
            assert_eq!(yes_no(Locale::En, answer), None);
        }
        for locale in [Locale::En, Locale::Es] {
            assert_eq!(yes_no(locale, "YES"), Some(true));
            assert_eq!(yes_no(locale, "no"), Some(false));
            assert_eq!(yes_no(locale, ""), None);
            assert_eq!(yes_no(locale, "unknown"), None);
        }
    }

    #[test]
    fn plural_rules_cover_zero_one_other_and_full_width_counts() {
        for (locale, singular, plural) in [
            (Locale::En, "request", "requests"),
            (Locale::Es, "solicitud", "solicitudes"),
        ] {
            for count in [0, 1, 2, u64::MAX] {
                let noun = if count == 1 { singular } else { plural };
                assert_eq!(
                    messages::usage_requests(locale, count, &count),
                    format!("{count} {noun}")
                );
            }
        }
    }

    #[test]
    fn owned_error_boundaries_keep_canonical_identity_and_localized_causes() {
        use super::{DiagnosticText, TerminalMessage};
        let message = DiagnosticText::new(|locale| messages::language_current(locale, "es"));
        assert_eq!(message, DiagnosticText::from("Current language: es"));
        assert_eq!(message.to_string(), "Current language: es");
        assert_eq!(message.terminal_message(Locale::Es), "Idioma actual: es");
        let error = std::io::Error::other(message);
        assert_eq!(error.to_string(), "Current language: es");
        assert_eq!(error.terminal_message(Locale::Es), "Idioma actual: es");
    }

    #[test]
    fn parameters_are_inserted_literally() {
        assert_eq!(
            messages::language_current(Locale::Es, "{literal}"),
            "Idioma actual: {literal}"
        );
        assert_eq!(Locale::parse("unsupported"), None);
        assert_eq!(
            messages::fallback_fixture::probe(Locale::Es, "{raw}"),
            "English fallback: {raw}"
        );
    }
}

/// Human-facing projection of a typed diagnostic, independent of canonical Display.
pub trait TerminalMessage {
    fn terminal_message(&self, locale: Locale) -> String;
}

impl<T: TerminalMessage + ?Sized> TerminalMessage for &T {
    fn terminal_message(&self, locale: Locale) -> String {
        T::terminal_message(self, locale)
    }
}

trait LocalizedError: std::error::Error + TerminalMessage + Send + Sync {}
impl<T: std::error::Error + TerminalMessage + Send + Sync> LocalizedError for T {}

/// Preserve a nested diagnostic's canonical and localized presentations across crate boundaries.
#[derive(Debug)]
pub struct ErrorCause(Box<dyn LocalizedError>);

impl ErrorCause {
    #[must_use]
    pub fn new(error: impl std::error::Error + TerminalMessage + Send + Sync + 'static) -> Self {
        Self(Box::new(error))
    }
}

impl std::fmt::Display for ErrorCause {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::error::Error for ErrorCause {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.0.as_ref())
    }
}

impl TerminalMessage for ErrorCause {
    fn terminal_message(&self, locale: Locale) -> String {
        self.0.terminal_message(locale)
    }
}

/// An owned diagnostic that retains canonical text and each terminal projection.
/// Use at string-based error boundaries, before the typed cause would be lost.
/// Equality compares canonical text so translating a diagnostic never changes its identity.
#[derive(Debug, Clone)]
pub struct DiagnosticText {
    canonical: String,
    translations: Vec<(Locale, String)>,
}

impl PartialEq for DiagnosticText {
    fn eq(&self, other: &Self) -> bool {
        self.canonical == other.canonical
    }
}
impl Eq for DiagnosticText {}

impl DiagnosticText {
    #[must_use]
    pub fn new(render: impl Fn(Locale) -> String) -> Self {
        Self {
            canonical: render(Locale::En),
            translations: Locale::ALL
                .iter()
                .copied()
                .filter(|locale| *locale != Locale::En)
                .map(|locale| (locale, render(locale)))
                .collect(),
        }
    }
}

impl From<String> for DiagnosticText {
    fn from(canonical: String) -> Self {
        Self {
            canonical,
            translations: Vec::new(),
        }
    }
}

impl From<&str> for DiagnosticText {
    fn from(canonical: &str) -> Self {
        canonical.to_owned().into()
    }
}

impl std::fmt::Display for DiagnosticText {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.canonical)
    }
}

impl std::error::Error for DiagnosticText {}

impl TerminalMessage for DiagnosticText {
    fn terminal_message(&self, locale: Locale) -> String {
        self.translations
            .iter()
            .find(|(language, _)| *language == locale)
            .map_or_else(|| self.canonical.clone(), |(_, text)| text.clone())
    }
}

impl TerminalMessage for std::io::Error {
    fn terminal_message(&self, locale: Locale) -> String {
        self.get_ref()
            .and_then(|cause| cause.downcast_ref::<DiagnosticText>())
            .map_or_else(|| self.to_string(), |cause| cause.terminal_message(locale))
    }
}
