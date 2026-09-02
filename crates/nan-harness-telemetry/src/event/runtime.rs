use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeContext {
    os_family: OsFamily,
    architecture: Architecture,
    #[serde(default)]
    target_environment: TargetEnvironment,
    interactive: bool,
}

impl RuntimeContext {
    pub(crate) fn current(interactive: bool) -> Self {
        Self {
            os_family: OsFamily::current(),
            architecture: Architecture::current(),
            target_environment: TargetEnvironment::current(),
            interactive,
        }
    }

    #[must_use]
    pub fn os_family(&self) -> OsFamily {
        self.os_family
    }

    #[must_use]
    pub fn architecture(&self) -> Architecture {
        self.architecture
    }

    #[must_use]
    pub fn target_environment(&self) -> TargetEnvironment {
        self.target_environment
    }

    #[must_use]
    pub fn interactive(&self) -> bool {
        self.interactive
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TargetEnvironment {
    Gnu,
    Musl,
    Msvc,
    #[default]
    Other,
}

impl TargetEnvironment {
    const fn current() -> Self {
        if cfg!(target_env = "gnu") {
            Self::Gnu
        } else if cfg!(target_env = "musl") {
            Self::Musl
        } else if cfg!(target_env = "msvc") {
            Self::Msvc
        } else {
            Self::Other
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Gnu => "gnu",
            Self::Musl => "musl",
            Self::Msvc => "msvc",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OsFamily {
    Linux,
    Macos,
    Windows,
    Other,
}

impl OsFamily {
    const fn current() -> Self {
        if cfg!(target_os = "linux") {
            Self::Linux
        } else if cfg!(target_os = "macos") {
            Self::Macos
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Other
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Macos => "macos",
            Self::Windows => "windows",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Architecture {
    #[serde(rename = "x86_64")]
    X86_64,
    #[serde(rename = "aarch64")]
    Aarch64,
    #[serde(rename = "other")]
    Other,
}

impl Architecture {
    const fn current() -> Self {
        if cfg!(target_arch = "x86_64") {
            Self::X86_64
        } else if cfg!(target_arch = "aarch64") {
            Self::Aarch64
        } else {
            Self::Other
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::X86_64 => "x86_64",
            Self::Aarch64 => "aarch64",
            Self::Other => "other",
        }
    }
}
