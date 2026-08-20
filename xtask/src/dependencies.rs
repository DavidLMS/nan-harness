use std::env;
use std::ffi::{OsStr, OsString};
use std::process::Command;

const LINUX_TARGETS: [&str; 2] = ["aarch64-unknown-linux-musl", "x86_64-unknown-linux-musl"];
const NON_LINUX_TARGETS: [&str; 3] = [
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "x86_64-pc-windows-msvc",
];

pub(crate) fn check() -> Result<(), String> {
    for target in LINUX_TARGETS {
        require_dependency_path(
            "sha2@0.10",
            target,
            "normal",
            &[
                "secret-service",
                "zbus-secret-service-keyring-store",
                "keyring",
            ],
        )?;
        require_dependency_path(
            "crypto-common@0.1",
            target,
            "normal",
            &[
                "secret-service",
                "zbus-secret-service-keyring-store",
                "keyring",
            ],
        )?;
    }

    for target in NON_LINUX_TARGETS {
        require_absent("sha2@0.10", target, "normal")?;
        require_absent("crypto-common@0.1", target, "normal")?;
    }

    require_dependency_path("syn@2", "all", "normal,build", &["nan-harness-cli"])?;
    require_dependency_path("syn@3", "all", "normal,build", &["nan-harness-cli"])?;

    println!(
        "dependency policy ok: legacy crypto remains Linux-only through keyring; syn major versions remain tracked"
    );
    Ok(())
}

fn require_dependency_path(
    package: &str,
    target: &str,
    edges: &str,
    required_packages: &[&str],
) -> Result<(), String> {
    let tree = dependency_tree(package, target, edges)?;
    if tree.trim().is_empty() {
        return Err(format!(
            "expected dependency '{package}' for target '{target}', but cargo tree found no path"
        ));
    }
    for required in required_packages {
        if !tree.contains(required) {
            return Err(format!(
                "dependency '{package}' for target '{target}' no longer follows the reviewed path through '{required}'"
            ));
        }
    }
    Ok(())
}

fn require_absent(package: &str, target: &str, edges: &str) -> Result<(), String> {
    let tree = dependency_tree(package, target, edges)?;
    if tree.trim().is_empty() {
        Ok(())
    } else {
        Err(format!(
            "dependency '{package}' unexpectedly reached non-Linux target '{target}'"
        ))
    }
}

fn dependency_tree(package: &str, target: &str, edges: &str) -> Result<String, String> {
    let cargo = env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    let arguments = [
        "tree",
        "--locked",
        "--package",
        "nan-harness-cli",
        "--target",
        target,
        "--invert",
        package,
        "--edges",
        edges,
    ];
    let output = Command::new(&cargo)
        .args(arguments)
        .output()
        .map_err(|error| {
            format!(
                "could not start {}: {error}",
                display_command(&cargo, &arguments)
            )
        })?;
    if !output.status.success() {
        return Err(format!(
            "{} exited with {}: {}",
            display_command(&cargo, &arguments),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout)
        .map_err(|error| format!("cargo tree returned non-UTF-8 output: {error}"))
}

fn display_command(cargo: &OsStr, arguments: &[&str]) -> String {
    let cargo = cargo.to_string_lossy();
    format!("{cargo} {}", arguments.join(" "))
}
