use super::MAX_CONFIGURATION_BYTES;
use super::errors::SearchPolicyError;
use super::signal::DetectionSignal;
use nan_harness_core::HarnessKind;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

pub(super) const HERMES_SEARCH_ENVIRONMENT: &[&str] = &[
    "BRAVE_SEARCH_API_KEY",
    "EXA_API_KEY",
    "FIRECRAWL_API_KEY",
    "KEENABLE_API_KEY",
    "PARALLEL_API_KEY",
    "SEARXNG_BASE_URL",
    "TAVILY_API_KEY",
];

const OPENCLAW_SEARCH_ENVIRONMENT: &[&str] = &[
    "BRAVE_API_KEY",
    "EXA_API_KEY",
    "FIRECRAWL_API_KEY",
    "GEMINI_API_KEY",
    "GOOGLE_API_KEY",
    "KIMI_API_KEY",
    "MINIMAX_API_KEY",
    "MINIMAX_CODE_PLAN_KEY",
    "MINIMAX_CODING_API_KEY",
    "MINIMAX_OAUTH_TOKEN",
    "MOONSHOT_API_KEY",
    "OPENROUTER_API_KEY",
    "PARALLEL_API_KEY",
    "PERPLEXITY_API_KEY",
    "SEARXNG_BASE_URL",
    "TAVILY_API_KEY",
    "XAI_API_KEY",
];

pub(super) fn detect_environment(
    harness: HarnessKind,
    home: &Path,
) -> Result<DetectionSignal, SearchPolicyError> {
    let (names, dotenv) = match harness {
        HarnessKind::Hermes => {
            let hermes_home =
                env::var_os("HERMES_HOME").map_or_else(|| home.join(".hermes"), PathBuf::from);
            (HERMES_SEARCH_ENVIRONMENT, Some(hermes_home.join(".env")))
        }
        HarnessKind::OpenClaw => (
            OPENCLAW_SEARCH_ENVIRONMENT,
            Some(home.join(".openclaw/.env")),
        ),
        _ => (&[][..], None),
    };
    if names.iter().any(|name| {
        env::var_os(name)
            .as_deref()
            .is_some_and(|value| !value.is_empty())
    }) {
        return Ok(DetectionSignal::External);
    }
    dotenv.map_or(Ok(DetectionSignal::None), |path| {
        inspect_dotenv(&path, names)
    })
}

pub(super) fn inspect_dotenv(
    path: &Path,
    search_environment: &[&str],
) -> Result<DetectionSignal, SearchPolicyError> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => metadata,
        Ok(_) => return Ok(DetectionSignal::None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(DetectionSignal::None);
        }
        Err(source) => {
            return Err(SearchPolicyError::ReadConfiguration {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    if metadata.len() > MAX_CONFIGURATION_BYTES {
        return Err(SearchPolicyError::ConfigurationTooLarge(path.to_path_buf()));
    }
    let contents =
        fs::read_to_string(path).map_err(|source| SearchPolicyError::ReadConfiguration {
            path: path.to_path_buf(),
            source,
        })?;
    let configured = contents.lines().any(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return false;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((name, value)) = line.split_once('=') else {
            return false;
        };
        search_environment.contains(&name.trim())
            && !value.trim().trim_matches(['\'', '"']).is_empty()
    });
    Ok(if configured {
        DetectionSignal::External
    } else {
        DetectionSignal::None
    })
}

pub(super) fn home_directory() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        env::var_os("HOME").map(PathBuf::from)
    }
}
