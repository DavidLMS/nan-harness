use crate::app::{HarnessRunArgs, MediaArgs};
use nan_harness_core::{HarnessKind, MediaSelection};
use serde_json::Value;
use serde_yaml_ng::Value as YamlValue;
use std::fs;
use std::path::{Path, PathBuf};

const MAX_CONFIGURATION_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExistingCapability {
    None,
    Managed,
    External,
}

/// Returns the explicit media request, leaving `None` to mean automatic selection.
pub(crate) fn requested_media(arguments: &MediaArgs) -> Option<MediaSelection> {
    if arguments.force_media {
        Some(MediaSelection::all())
    } else if arguments.force_stt || arguments.force_tts || arguments.force_image {
        Some(MediaSelection {
            stt: arguments.force_stt,
            tts: arguments.force_tts,
            image: arguments.force_image,
        })
    } else {
        None
    }
}

pub(crate) fn launch_media(
    kind: HarnessKind,
    arguments: &HarnessRunArgs,
    home: &Path,
    working_directory: &Path,
) -> MediaSelection {
    resolve(
        kind,
        requested_media(&arguments.media),
        None,
        home,
        working_directory,
    )
}

pub(crate) fn configuration_media(
    kind: HarnessKind,
    requested: Option<MediaSelection>,
    previously_managed: MediaSelection,
    home: &Path,
    working_directory: &Path,
) -> MediaSelection {
    resolve(
        kind,
        requested,
        Some(previously_managed),
        home,
        working_directory,
    )
}

fn resolve(
    kind: HarnessKind,
    requested: Option<MediaSelection>,
    previously_managed: Option<MediaSelection>,
    home: &Path,
    _working_directory: &Path,
) -> MediaSelection {
    if !matches!(kind, HarnessKind::Hermes | HarnessKind::OpenClaw) {
        return MediaSelection::none();
    }
    let detected = detect(kind, home);
    let automatic = MediaSelection {
        stt: automatic_capability(detected.stt, previously_managed.map(|value| value.stt)),
        tts: automatic_capability(detected.tts, previously_managed.map(|value| value.tts)),
        image: automatic_capability(detected.image, previously_managed.map(|value| value.image)),
    };
    match requested {
        Some(requested) => MediaSelection {
            stt: requested.stt || previously_managed.is_some_and(|value| value.stt),
            tts: requested.tts || previously_managed.is_some_and(|value| value.tts),
            image: requested.image || previously_managed.is_some_and(|value| value.image),
        },
        None => automatic,
    }
}

fn automatic_capability(detected: ExistingCapability, _previously_managed: Option<bool>) -> bool {
    match detected {
        ExistingCapability::External => false,
        ExistingCapability::Managed | ExistingCapability::None => true,
    }
}

#[derive(Debug, Clone, Copy)]
struct DetectedMedia {
    stt: ExistingCapability,
    tts: ExistingCapability,
    image: ExistingCapability,
}

fn detect(kind: HarnessKind, home: &Path) -> DetectedMedia {
    match kind {
        HarnessKind::Hermes => detect_hermes(home),
        HarnessKind::OpenClaw => detect_openclaw(home),
        _ => DetectedMedia {
            stt: ExistingCapability::External,
            tts: ExistingCapability::External,
            image: ExistingCapability::External,
        },
    }
}

fn detect_hermes(home: &Path) -> DetectedMedia {
    let path = std::env::var_os("HERMES_HOME")
        .map_or_else(|| home.join(".hermes"), PathBuf::from)
        .join("config.yaml");
    let Ok(contents) = read_bounded(&path) else {
        return DetectedMedia {
            stt: ExistingCapability::None,
            tts: ExistingCapability::None,
            image: ExistingCapability::None,
        };
    };
    let Ok(value) = serde_yaml_ng::from_slice::<YamlValue>(&contents) else {
        return external_media();
    };
    DetectedMedia {
        stt: hermes_provider_state(&value, "stt", &["stt", "provider"], "nan-whisper"),
        tts: hermes_provider_state(&value, "tts", &["tts", "provider"], "nan-kokoro"),
        image: provider_state(&value, &["image_gen", "provider"], &["nan-harness"]),
    }
}

fn hermes_provider_state(
    value: &YamlValue,
    section: &str,
    provider_path: &[&str],
    managed: &str,
) -> ExistingCapability {
    let state = provider_state(value, provider_path, &[managed]);
    if state == ExistingCapability::None
        && value
            .get(section)
            .and_then(|section| section.get("providers"))
            .and_then(|providers| providers.get(managed))
            .is_some()
    {
        ExistingCapability::External
    } else {
        state
    }
}

fn detect_openclaw(home: &Path) -> DetectedMedia {
    let path = home.join(".openclaw/openclaw.json");
    let Ok(contents) = read_bounded(&path) else {
        return DetectedMedia {
            stt: ExistingCapability::None,
            tts: ExistingCapability::None,
            image: ExistingCapability::None,
        };
    };
    let Ok(value) = serde_json::from_slice::<Value>(&contents) else {
        return external_media();
    };
    let tts = openclaw_provider_state(value.pointer("/tts/provider"), "nan-harness");
    DetectedMedia {
        stt: openclaw_audio_state(&value),
        tts: if tts == ExistingCapability::None
            && value.pointer("/tts/providers/nan-harness").is_some()
        {
            ExistingCapability::External
        } else {
            tts
        },
        image: openclaw_image_state(&value),
    }
}

fn openclaw_audio_state(value: &Value) -> ExistingCapability {
    let Some(models) = value.pointer("/tools/media/audio/models") else {
        return ExistingCapability::None;
    };
    let Some(models) = models.as_array() else {
        return ExistingCapability::External;
    };
    match models.first() {
        Some(model) => openclaw_provider_state(model.get("provider"), "nan-harness"),
        None => ExistingCapability::None,
    }
}

fn provider_state(value: &YamlValue, path: &[&str], managed: &[&str]) -> ExistingCapability {
    let mut current = value;
    for part in path {
        let Some(next) = current.get(*part) else {
            return ExistingCapability::None;
        };
        current = next;
    }
    match current.as_str() {
        Some(value) if managed.contains(&value) => ExistingCapability::Managed,
        Some(_) | None => ExistingCapability::External,
    }
}

fn openclaw_provider_state(value: Option<&Value>, managed: &str) -> ExistingCapability {
    match value.and_then(Value::as_str) {
        Some(value) if value == managed => ExistingCapability::Managed,
        Some(_) => ExistingCapability::External,
        None => ExistingCapability::None,
    }
}

fn openclaw_image_state(value: &Value) -> ExistingCapability {
    let primary = value
        .pointer("/agents/defaults/mediaModels/image/primary")
        .or_else(|| value.pointer("/agents/defaults/imageGenerationModel"));
    match primary.and_then(Value::as_str) {
        Some(value) if value.starts_with("nan-harness/") => ExistingCapability::Managed,
        Some(_) => ExistingCapability::External,
        None if value
            .pointer("/plugins/entries/nan-harness-media")
            .is_some() =>
        {
            ExistingCapability::External
        }
        None => ExistingCapability::None,
    }
}

fn external_media() -> DetectedMedia {
    DetectedMedia {
        stt: ExistingCapability::External,
        tts: ExistingCapability::External,
        image: ExistingCapability::External,
    }
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, std::io::Error> {
    let contents = fs::read(path)?;
    if contents.len() > MAX_CONFIGURATION_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "configuration exceeds the supported size",
        ));
    }
    Ok(contents)
}

#[cfg(test)]
mod tests {
    use super::configuration_media;
    use nan_harness_core::{HarnessKind, MediaSelection};
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn automatic_selection_preserves_external_capabilities() {
        let root = tempdir().expect("temporary home");
        fs::create_dir_all(root.path().join(".hermes")).expect("hermes home");
        fs::write(
            root.path().join(".hermes/config.yaml"),
            "stt:\n  provider: external\ntts:\n  provider: nan-kokoro\n",
        )
        .expect("config");
        let selected = configuration_media(
            HarnessKind::Hermes,
            None,
            MediaSelection::none(),
            root.path(),
            root.path(),
        );
        assert_eq!(
            selected,
            MediaSelection {
                stt: false,
                tts: true,
                image: true
            }
        );
    }

    #[test]
    fn explicit_selection_can_force_one_capability() {
        let root = tempdir().expect("temporary home");
        fs::create_dir_all(root.path().join(".openclaw")).expect("openclaw home");
        fs::write(
            root.path().join(".openclaw/openclaw.json"),
            r#"{"tts":{"provider":"external"}}"#,
        )
        .expect("config");
        let selected = configuration_media(
            HarnessKind::OpenClaw,
            Some(MediaSelection {
                stt: false,
                tts: true,
                image: false,
            }),
            MediaSelection::none(),
            root.path(),
            root.path(),
        );
        assert!(selected.tts);
        assert!(!selected.stt);
        assert!(!selected.image);
    }
}
