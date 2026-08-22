use std::process::Stdio;
use std::time::Duration;
use thiserror::Error;
use tokio::io::AsyncWriteExt as _;
use tokio::process::Command;
use zeroize::Zeroizing;

const SECURITY: &str = "/usr/bin/security";
const SERVICE: &str = "dev.nan-harness.canary";
const TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) const API_KEY_ACCOUNT: &str = "NAN_API_KEY";
pub(crate) const NTFY_TOKEN_ACCOUNT: &str = "NTFY_TOKEN";

pub(crate) async fn store(account: &str, secret: &str) -> Result<(), CredentialError> {
    let input = Zeroizing::new(password_input(secret)?);
    let mut child = Command::new(SECURITY)
        .args([
            "add-generic-password",
            "-U",
            "-a",
            account,
            "-s",
            SERVICE,
            "-T",
            SECURITY,
            "-w",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(CredentialError::StartSecurity)?;
    child
        .stdin
        .take()
        .ok_or(CredentialError::MissingStdin)?
        .write_all(input.as_ref())
        .await
        .map_err(CredentialError::WriteSecret)?;
    let status = tokio::time::timeout(TIMEOUT, child.wait())
        .await
        .map_err(|_| CredentialError::SecurityTimeout)?
        .map_err(CredentialError::WaitForSecurity)?;
    if status.success() {
        Ok(())
    } else {
        Err(CredentialError::SecurityFailure)
    }
}

pub(crate) async fn read(account: &str) -> Result<Zeroizing<String>, CredentialError> {
    let output = tokio::time::timeout(
        TIMEOUT,
        Command::new(SECURITY)
            .args(["find-generic-password", "-a", account, "-s", SERVICE, "-w"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output(),
    )
    .await
    .map_err(|_| CredentialError::SecurityTimeout)?
    .map_err(CredentialError::StartSecurity)?;
    if !output.status.success() {
        return Err(CredentialError::SecurityFailure);
    }
    let mut secret = Zeroizing::new(
        String::from_utf8(output.stdout).map_err(CredentialError::NonUnicodeSecret)?,
    );
    while secret.ends_with(['\r', '\n']) {
        secret.pop();
    }
    if secret.is_empty() {
        Err(CredentialError::EmptySecret)
    } else {
        Ok(secret)
    }
}

fn password_input(secret: &str) -> Result<Vec<u8>, CredentialError> {
    if secret.is_empty() {
        return Err(CredentialError::EmptySecret);
    }
    if secret.contains(['\r', '\n', '\0']) {
        return Err(CredentialError::InvalidSecret);
    }
    Ok(format!("{secret}\n{secret}\n").into_bytes())
}

#[derive(Debug, Error)]
pub(crate) enum CredentialError {
    #[error("the credential must not be empty")]
    EmptySecret,
    #[error("the credential contains unsupported control characters")]
    InvalidSecret,
    #[error("could not start the macOS security command: {0}")]
    StartSecurity(std::io::Error),
    #[error("the macOS security command did not expose stdin")]
    MissingStdin,
    #[error("could not write the credential to the macOS security command: {0}")]
    WriteSecret(std::io::Error),
    #[error("could not wait for the macOS security command: {0}")]
    WaitForSecurity(std::io::Error),
    #[error("the macOS security command timed out")]
    SecurityTimeout,
    #[error("the macOS security command returned a failure status")]
    SecurityFailure,
    #[error("the stored credential is not valid Unicode: {0}")]
    NonUnicodeSecret(std::string::FromUtf8Error),
}

#[cfg(test)]
mod tests {
    use super::{CredentialError, password_input};

    #[test]
    fn password_input_confirms_the_secret_without_using_arguments() {
        assert_eq!(
            password_input("secret").expect("secret should be accepted"),
            b"secret\nsecret\n"
        );
        assert!(matches!(
            password_input("secret\nnext"),
            Err(CredentialError::InvalidSecret)
        ));
    }
}
