use super::{InstallationPaths, UninstallError, prompt};
use crate::commands::persistence::PersistentIntegration;
use nan_harness_core::HarnessKind;
use std::io::{BufRead, Cursor, Error, ErrorKind, Read, Write};
use std::path::Path;

#[test]
fn empty_inventory_discloses_no_configuration_and_no_optional_items() {
    let mut input = Cursor::new("yes\n");
    let mut output = Vec::new();

    let confirmed = prompt(
        &installation(false),
        Path::new("/tmp/synthetic/nan-harness/data"),
        &[],
        &[],
        false,
        false,
        false,
        false,
        &mut input,
        &mut output,
    )
    .expect("empty inventory should prompt");
    let output = String::from_utf8(output).expect("prompt output should be UTF-8");

    assert!(confirmed);
    assert_line(&output, "  - Managed harness configurations: none");
    assert_line(&output, "  - Saved NaN API key: none");
    assert_line(
        &output,
        "  - Application data: '/tmp/synthetic/nan-harness/data'",
    );
    assert_line(
        &output,
        "  - Executable: '/tmp/synthetic/nan-harness/bin/nan-harness'",
    );
    assert_absent(&output, "ChatGPT Desktop profile");
    assert_absent(&output, "Hermes CLI/Desktop shared profile");
    assert_absent(&output, "Pen Desktop native NaN provider");
    assert_absent(&output, "  - Alias:");
    assert_absent(&output, "/tmp/synthetic/nan-harness/bin/nanh");
}

#[test]
fn mixed_inventory_is_deduplicated_and_sorted_by_name() {
    let integrations = [
        PersistentIntegration::Aider,
        PersistentIntegration::Pi,
        PersistentIntegration::Aider,
    ];
    let native_configurations = [HarnessKind::Codex, HarnessKind::Fx];
    let mut input = Cursor::new("yes\n");
    let mut output = Vec::new();

    let confirmed = prompt(
        &installation(false),
        Path::new("/tmp/synthetic/nan-harness/data"),
        &integrations,
        &native_configurations,
        false,
        false,
        false,
        false,
        &mut input,
        &mut output,
    )
    .expect("mixed inventory should prompt");
    let output = String::from_utf8(output).expect("prompt output should be UTF-8");

    assert!(confirmed);
    assert_line(
        &output,
        "  - Managed harness configurations: Aider, Pi, codex, fx",
    );
    assert_absent(&output, ", Aider, Aider");
}

#[test]
fn explicit_flags_disclose_desktop_consequences_and_saved_key() {
    let mut input = Cursor::new("no\n");
    let mut output = Vec::new();

    let confirmed = prompt(
        &installation(true),
        Path::new("/tmp/synthetic/nan-harness/data"),
        &[],
        &[],
        true,
        true,
        true,
        true,
        &mut input,
        &mut output,
    )
    .expect("explicit flags should prompt");
    let output = String::from_utf8(output).expect("prompt output should be UTF-8");

    assert!(!confirmed);
    assert_line(&output, "  - Saved NaN API key: yes");
    assert_line(
        &output,
        "  - ChatGPT Desktop profile: authentication, history, and cache",
    );
    assert_line(
        &output,
        "  - Hermes CLI/Desktop shared profile: conversations and local state",
    );
    assert_line(
        &output,
        "  - Pen Desktop native NaN provider and copied key",
    );
    assert_line(&output, "  - Alias: '/tmp/synthetic/nan-harness/bin/nanh'");
}

#[test]
fn remove_alias_controls_alias_disclosure() {
    for (remove_alias, alias_is_disclosed) in [(false, false), (true, true)] {
        let mut input = Cursor::new("yes\n");
        let mut output = Vec::new();

        prompt(
            &installation(remove_alias),
            Path::new("/tmp/synthetic/nan-harness/data"),
            &[],
            &[],
            false,
            false,
            false,
            false,
            &mut input,
            &mut output,
        )
        .expect("alias flag should prompt");
        let output = String::from_utf8(output).expect("prompt output should be UTF-8");

        assert_eq!(
            output.contains("  - Alias: '/tmp/synthetic/nan-harness/bin/nanh'"),
            alias_is_disclosed
        );
    }
}

#[test]
fn failed_flush_returns_prompt_without_reading_consent() {
    let mut input = Cursor::new("yes\n");
    let mut output = FailingFlushWriter;

    let error = prompt(
        &installation(false),
        Path::new("/tmp/synthetic/nan-harness/data"),
        &[],
        &[],
        false,
        false,
        false,
        false,
        &mut input,
        &mut output,
    )
    .expect_err("a failed flush should prevent consent");

    assert!(matches!(error, UninstallError::Prompt(_)));
    assert_eq!(input.position(), 0);
}

#[test]
fn failed_reader_returns_prompt_without_consent() {
    let mut input = FailingReader { consulted: false };
    let mut output = Vec::new();

    let error = prompt(
        &installation(false),
        Path::new("/tmp/synthetic/nan-harness/data"),
        &[],
        &[],
        false,
        false,
        false,
        false,
        &mut input,
        &mut output,
    )
    .expect_err("a failed reader should prevent consent");

    assert!(matches!(error, UninstallError::Prompt(_)));
    assert!(input.consulted);
    assert!(
        String::from_utf8(output)
            .expect("prompt output should be UTF-8")
            .contains("Continue? [y/N]:")
    );
}

fn installation(remove_alias: bool) -> InstallationPaths {
    InstallationPaths {
        executable_path: "/tmp/synthetic/nan-harness/bin/nan-harness".into(),
        alias_path: "/tmp/synthetic/nan-harness/bin/nanh".into(),
        remove_alias,
        #[cfg(windows)]
        user_path_entry_added: false,
    }
}

fn assert_line(output: &str, expected: &str) {
    assert!(
        output.lines().any(|line| line == expected),
        "prompt output did not contain the expected line:\n{expected}\nactual:\n{output}"
    );
}

fn assert_absent(output: &str, unexpected: &str) {
    assert!(
        !output.contains(unexpected),
        "prompt output unexpectedly contained:\n{unexpected}"
    );
}

struct FailingFlushWriter;

impl Write for FailingFlushWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Err(Error::new(ErrorKind::BrokenPipe, "synthetic failure"))
    }
}

struct FailingReader {
    consulted: bool,
}

impl Read for FailingReader {
    fn read(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> {
        self.consulted = true;
        Err(Error::new(ErrorKind::BrokenPipe, "synthetic failure"))
    }
}

impl BufRead for FailingReader {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        self.consulted = true;
        Err(Error::new(ErrorKind::BrokenPipe, "synthetic failure"))
    }

    fn consume(&mut self, amount: usize) {
        let _ = amount;
    }
}
