use crate::report::{Architecture, Platform};
use nan_harness_core::DesktopHarnessKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageFormat {
    Dmg,
    TarGz,
    Deb,
    WindowsSetup,
}

/// Discovery never downloads or executes these sources. The installer must check the
/// extracted binary architecture and journal ownership before changing the system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Distribution {
    Direct {
        url: String,
        format: PackageFormat,
    },
    /// Store installation cannot be unpacked safely into a private run directory.
    MicrosoftStore {
        product_id: &'static str,
    },
    /// Resolve the latest official DEB and its checksum without registering APT sources.
    DebianRepository {
        base_url: &'static str,
        architecture: &'static str,
    },
    /// Source builds are not substituted with a similarly named community application.
    SourceBuild {
        repository: &'static str,
    },
    Unavailable {
        reason: &'static str,
    },
}

/// Official sources checked 2026-09-08; see `canary/checker-platforms.md`.
#[must_use]
pub fn download(
    kind: DesktopHarnessKind,
    platform: Platform,
    architecture: Architecture,
) -> Distribution {
    match kind {
        DesktopHarnessKind::ChatGpt => chatgpt(platform, architecture),
        DesktopHarnessKind::Claude => claude(platform, architecture),
        DesktopHarnessKind::Hermes => hermes(platform),
        DesktopHarnessKind::Pen => pen(platform, architecture),
        DesktopHarnessKind::Zed => zed(platform, architecture),
    }
}

fn direct(url: impl Into<String>, format: PackageFormat) -> Distribution {
    Distribution::Direct {
        url: url.into(),
        format,
    }
}

fn chatgpt(platform: Platform, architecture: Architecture) -> Distribution {
    match platform {
        Platform::Macos if architecture == Architecture::Aarch64 => direct(
            "https://persistent.oaistatic.com/codex-app-prod/Codex.dmg",
            PackageFormat::Dmg,
        ),
        Platform::Macos => Distribution::Unavailable {
            reason: "The official macOS Desktop download requires Apple Silicon.",
        },
        Platform::Windows => Distribution::MicrosoftStore {
            product_id: "9PLM9XGG6VKS",
        },
        Platform::Linux => {
            let architecture = match architecture {
                Architecture::X86_64 => "amd64",
                Architecture::Aarch64 => "arm64",
            };
            direct(
                format!(
                    "https://persistent.oaistatic.com/codex-app-prod/linux/deb/latest/chatgpt_{architecture}.deb"
                ),
                PackageFormat::Deb,
            )
        }
    }
}

fn claude(platform: Platform, architecture: Architecture) -> Distribution {
    match platform {
        Platform::Macos => direct(
            "https://claude.ai/api/desktop/darwin/universal/dmg/latest/redirect",
            PackageFormat::Dmg,
        ),
        Platform::Windows => {
            let architecture = windows_arch(architecture);
            direct(
                format!("https://claude.ai/api/desktop/win32/{architecture}/setup/latest/redirect"),
                PackageFormat::WindowsSetup,
            )
        }
        Platform::Linux => Distribution::DebianRepository {
            base_url: "https://downloads.claude.ai/claude-desktop/apt/stable/",
            architecture: match architecture {
                Architecture::X86_64 => "amd64",
                Architecture::Aarch64 => "arm64",
            },
        },
    }
}

fn hermes(platform: Platform) -> Distribution {
    // The official website exposes one installer per OS, not architecture-specific URLs.
    // Its architecture must be inspected after download; no emulation is implied here.
    match platform {
        Platform::Macos => direct(
            "https://hermes-assets.nousresearch.com/Hermes-Setup.dmg",
            PackageFormat::Dmg,
        ),
        Platform::Windows => direct(
            "https://hermes-assets.nousresearch.com/Hermes-Setup.exe",
            PackageFormat::WindowsSetup,
        ),
        Platform::Linux => Distribution::SourceBuild {
            repository: "NousResearch/hermes-agent",
        },
    }
}

fn pen(platform: Platform, architecture: Architecture) -> Distribution {
    let architecture = windows_arch(architecture);
    let (asset, format) = match platform {
        Platform::Macos => (format!("Pen-mac-{architecture}.dmg"), PackageFormat::Dmg),
        Platform::Windows if architecture == "x64" => {
            ("Pen-win-x64.exe".to_owned(), PackageFormat::WindowsSetup)
        }
        Platform::Windows => {
            return Distribution::Unavailable {
                reason: "Pen lists Windows ARM64 as coming soon.",
            };
        }
        Platform::Linux => (
            format!("Pen-linux-{architecture}.tar.gz"),
            PackageFormat::TarGz,
        ),
    };
    direct(format!("https://www.pen.dev/download/{asset}"), format)
}

fn zed(platform: Platform, architecture: Architecture) -> Distribution {
    let architecture = match architecture {
        Architecture::X86_64 => "x86_64",
        Architecture::Aarch64 => "aarch64",
    };
    let (asset, format) = match platform {
        Platform::Macos => (format!("Zed-{architecture}.dmg"), PackageFormat::Dmg),
        Platform::Linux => (
            format!("zed-linux-{architecture}.tar.gz"),
            PackageFormat::TarGz,
        ),
        Platform::Windows => (
            format!("Zed-{architecture}.exe"),
            PackageFormat::WindowsSetup,
        ),
    };
    direct(
        format!("https://github.com/zed-industries/zed/releases/latest/download/{asset}"),
        format,
    )
}

const fn windows_arch(architecture: Architecture) -> &'static str {
    match architecture {
        Architecture::X86_64 => "x64",
        Architecture::Aarch64 => "arm64",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_covers_every_pair_without_community_substitutes() {
        for kind in DesktopHarnessKind::ALL {
            for platform in [Platform::Linux, Platform::Macos, Platform::Windows] {
                for architecture in [Architecture::X86_64, Architecture::Aarch64] {
                    if let Distribution::Direct { url, .. } = download(kind, platform, architecture)
                    {
                        let url = url::Url::parse(&url).expect("valid official URL");
                        assert_eq!(url.scheme(), "https");
                        assert!(url.username().is_empty());
                        assert!(url.password().is_none());
                    }
                }
            }
        }
    }

    #[test]
    fn missing_distributions_are_not_invented() {
        assert!(matches!(
            download(
                DesktopHarnessKind::Claude,
                Platform::Linux,
                Architecture::X86_64
            ),
            Distribution::DebianRepository {
                architecture: "amd64",
                ..
            }
        ));
        assert!(matches!(
            download(
                DesktopHarnessKind::Hermes,
                Platform::Linux,
                Architecture::Aarch64
            ),
            Distribution::SourceBuild {
                repository: "NousResearch/hermes-agent"
            }
        ));
        assert!(matches!(
            download(
                DesktopHarnessKind::Pen,
                Platform::Windows,
                Architecture::Aarch64
            ),
            Distribution::Unavailable { .. }
        ));
        assert!(matches!(
            download(
                DesktopHarnessKind::ChatGpt,
                Platform::Macos,
                Architecture::X86_64
            ),
            Distribution::Unavailable { .. }
        ));
    }
}
