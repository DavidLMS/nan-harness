use super::super::CredentialError;
use nan_harness_core::SecretValue;
use std::io::{BufRead as _, Write as _};

pub(super) fn prompt_logout_choice() -> Result<Option<bool>, CredentialError> {
    let mut output = std::io::stderr().lock();
    write!(output, "Choose [1]: ").map_err(CredentialError::Prompt)?;
    output.flush().map_err(CredentialError::Prompt)?;
    let mut response = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut response)
        .map_err(CredentialError::Prompt)?;
    parse_logout_choice(&response)
}

pub(super) fn prompt_yes_no(prompt: &str, default: bool) -> Result<bool, CredentialError> {
    let mut output = std::io::stderr().lock();
    write!(output, "{prompt}").map_err(CredentialError::Prompt)?;
    output.flush().map_err(CredentialError::Prompt)?;
    let mut response = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut response)
        .map_err(CredentialError::Prompt)?;
    Ok(parse_yes_no(&response, default))
}

pub(super) fn prompt_api_key() -> Result<SecretValue, CredentialError> {
    let api_key = rpassword::prompt_password("NaN API key (input hidden): ")
        .map_err(CredentialError::Prompt)?;
    SecretValue::new(api_key).map_err(CredentialError::Secret)
}

fn parse_logout_choice(response: &str) -> Result<Option<bool>, CredentialError> {
    match response.trim() {
        "" | "1" => Ok(Some(true)),
        "2" => Ok(Some(false)),
        "3" => Ok(None),
        _ => Err(CredentialError::InvalidLogoutChoice),
    }
}

fn parse_yes_no(response: &str, default: bool) -> bool {
    match response.trim().to_ascii_lowercase().as_str() {
        "y" | "yes" => true,
        "n" | "no" => false,
        _ => default,
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_logout_choice, parse_yes_no};
    use crate::commands::credentials::CredentialError;

    #[test]
    fn yes_no_parsing_preserves_defaults_and_case_insensitivity() {
        assert!(parse_yes_no(" YES\n", false));
        assert!(!parse_yes_no("No\n", true));
        assert!(parse_yes_no("\n", true));
        assert!(!parse_yes_no("unexpected", false));
    }

    #[test]
    fn logout_choice_parsing_preserves_default_and_cancel() {
        assert!(matches!(parse_logout_choice("\n"), Ok(Some(true))));
        assert!(matches!(parse_logout_choice("1\n"), Ok(Some(true))));
        assert!(matches!(parse_logout_choice("2\n"), Ok(Some(false))));
        assert!(matches!(parse_logout_choice("3\n"), Ok(None)));
        assert!(matches!(
            parse_logout_choice("4\n"),
            Err(CredentialError::InvalidLogoutChoice)
        ));
    }
}
