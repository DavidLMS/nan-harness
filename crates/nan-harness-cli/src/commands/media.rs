use crate::commands::media_audio;
use base64::Engine as _;
use reqwest::multipart::{Form, Part};
use reqwest::{Client, Url};
use serde::Deserialize;
use serde_json::json;
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;
use thiserror::Error;

const SUBCOMMAND: &str = "__media";
const DEFAULT_BASE_URL: &str = "https://api.nan.builders/v1";
const MAX_IMAGE_BYTES: usize = 50 * 1024 * 1024;
const MAX_TEXT_BYTES: usize = 256 * 1024;
const MAX_JSON_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Error)]
enum MediaError {
    #[error("invalid media arguments")]
    Arguments,
    #[error("a NaN API key is required")]
    MissingApiKey,
    #[error("invalid provider endpoint")]
    InvalidEndpoint,
    #[error("media input is missing or exceeds its size limit")]
    Input,
    #[error("large audio input must be a supported PCM WAV file")]
    AudioFormat,
    #[error("audio input could not be prepared")]
    AudioPrepare,
    #[error("transcription failed for audio chunk {index} of {total}")]
    AudioChunk { index: usize, total: usize },
    #[error("media output path is required")]
    Output,
    #[error("media request failed")]
    Request,
    #[error("media provider returned an unsupported response")]
    Response,
    #[error("media output could not be written")]
    Write,
}

#[derive(Debug, Default)]
struct Arguments {
    command: String,
    provider_base_url: Option<String>,
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    prompt: Option<String>,
    prompt_file: Option<PathBuf>,
    input_images: Vec<PathBuf>,
    language: Option<String>,
    voice: Option<String>,
    format: Option<String>,
    model: Option<String>,
}

pub(crate) async fn run_if_requested() -> Option<ExitCode> {
    let mut values = std::env::args_os();
    let _executable = values.next();
    if values.next().as_deref() != Some(std::ffi::OsStr::new(SUBCOMMAND)) {
        return None;
    }
    let result = match parse(values) {
        Ok(arguments) => run(arguments).await,
        Err(error) => Err(error),
    };
    Some(match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    })
}

fn parse(values: impl IntoIterator<Item = OsString>) -> Result<Arguments, MediaError> {
    let mut values = values.into_iter();
    let command = values
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or(MediaError::Arguments)?;
    if !matches!(command.as_str(), "stt" | "tts" | "image") {
        return Err(MediaError::Arguments);
    }
    let mut arguments = Arguments {
        command,
        ..Arguments::default()
    };
    while let Some(value) = values.next() {
        let key = value.into_string().map_err(|_| MediaError::Arguments)?;
        match key.as_str() {
            "--provider-base-url" => {
                arguments.provider_base_url = Some(next_value(&mut values)?);
            }
            "--input" => arguments.input = Some(PathBuf::from(next_value(&mut values)?)),
            "--output" => arguments.output = Some(PathBuf::from(next_value(&mut values)?)),
            "--prompt" => arguments.prompt = Some(next_value(&mut values)?),
            "--prompt-file" => {
                arguments.prompt_file = Some(PathBuf::from(next_value(&mut values)?));
            }
            "--input-image" => arguments
                .input_images
                .push(PathBuf::from(next_value(&mut values)?)),
            "--language" => arguments.language = Some(next_value(&mut values)?),
            "--voice" => arguments.voice = Some(next_value(&mut values)?),
            "--format" => arguments.format = Some(next_value(&mut values)?),
            "--model" => arguments.model = Some(next_value(&mut values)?),
            _ => return Err(MediaError::Arguments),
        }
    }
    Ok(arguments)
}

fn next_value<I: Iterator<Item = OsString>>(values: &mut I) -> Result<String, MediaError> {
    values
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or(MediaError::Arguments)
}

async fn run(arguments: Arguments) -> Result<(), MediaError> {
    let api_key = std::env::var("NAN_API_KEY").map_err(|_| MediaError::MissingApiKey)?;
    if api_key.trim().is_empty() {
        return Err(MediaError::MissingApiKey);
    }
    let base_url = endpoint(arguments.provider_base_url.as_deref())?;
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_mins(3))
        .build()
        .map_err(|_| MediaError::Request)?;
    match arguments.command.as_str() {
        "stt" => transcribe(&client, &base_url, &api_key, &arguments).await,
        "tts" => synthesize(&client, &base_url, &api_key, &arguments).await,
        "image" => generate_image(&client, &base_url, &api_key, &arguments).await,
        _ => Err(MediaError::Arguments),
    }
}

fn endpoint(explicit: Option<&str>) -> Result<Url, MediaError> {
    let raw = explicit
        .map(str::to_owned)
        .or_else(|| std::env::var("NAN_HARNESS_PROVIDER_BASE_URL").ok())
        .or_else(|| std::env::var("NAN_BASE_URL").ok())
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_owned());
    let mut url = Url::parse(raw.trim()).map_err(|_| MediaError::InvalidEndpoint)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || url.username() != ""
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(MediaError::InvalidEndpoint);
    }
    let path = url.path().trim_end_matches('/').to_owned();
    url.set_path(&path);
    Ok(url)
}

async fn transcribe(
    client: &Client,
    base_url: &Url,
    api_key: &str,
    arguments: &Arguments,
) -> Result<(), MediaError> {
    let input = arguments.input.as_deref().ok_or(MediaError::Input)?;
    let request_limit =
        u64::try_from(media_audio::MAX_AUDIO_REQUEST_BYTES).map_err(|_| MediaError::Input)?;
    let input_size = std::fs::metadata(input)
        .map_err(|_| MediaError::Input)?
        .len();
    if input_size <= request_limit {
        let bytes = read_limited(input, media_audio::MAX_AUDIO_REQUEST_BYTES)
            .map_err(|_| MediaError::Input)?;
        let text = transcribe_bytes(client, base_url, api_key, arguments, bytes, "audio").await?;
        return write_text_or_stdout(arguments.output.as_deref(), &text);
    }

    let (_temporary_directory, chunks) = media_audio::prepare_chunks(input).map_err(|error| {
        if matches!(error, media_audio::AudioError::Format) {
            MediaError::AudioFormat
        } else {
            MediaError::AudioPrepare
        }
    })?;
    let total = chunks.len();
    let mut parts = Vec::with_capacity(total);
    for (index, chunk) in chunks.iter().enumerate() {
        let bytes = read_limited(chunk, media_audio::MAX_AUDIO_REQUEST_BYTES)
            .map_err(|_| MediaError::AudioPrepare)?;
        let text = transcribe_bytes(client, base_url, api_key, arguments, bytes, "audio.wav")
            .await
            .map_err(|_| MediaError::AudioChunk {
                index: index + 1,
                total,
            })?;
        parts.push(text);
    }
    let text = media_audio::merge_transcripts(&parts);
    write_text_or_stdout(arguments.output.as_deref(), &text)
}

async fn transcribe_bytes(
    client: &Client,
    base_url: &Url,
    api_key: &str,
    arguments: &Arguments,
    bytes: Vec<u8>,
    file_name: &str,
) -> Result<String, MediaError> {
    let mut form = Form::new()
        .text(
            "model",
            arguments.model.as_deref().unwrap_or("whisper-1").to_owned(),
        )
        .text("response_format", "json".to_owned())
        .part("file", Part::bytes(bytes).file_name(file_name.to_owned()));
    if let Some(language) = &arguments.language {
        form = form.text("language", language.clone());
    }
    let response = client
        .post(join(base_url, "audio/transcriptions"))
        .bearer_auth(api_key)
        .multipart(form)
        .send()
        .await
        .map_err(|_| MediaError::Request)?;
    let payload: TranscriptionResponse = response_json(response).await?;
    Ok(payload.text)
}

async fn synthesize(
    client: &Client,
    base_url: &Url,
    api_key: &str,
    arguments: &Arguments,
) -> Result<(), MediaError> {
    let input = arguments.input.as_deref().ok_or(MediaError::Input)?;
    let text = read_limited(input, MAX_TEXT_BYTES).map_err(|_| MediaError::Input)?;
    let text = String::from_utf8(text).map_err(|_| MediaError::Input)?;
    let mut body = json!({
        "model": arguments.model.as_deref().unwrap_or("kokoro"),
        "input": text,
        "response_format": arguments.format.as_deref().unwrap_or("mp3")
    });
    if let Some(voice) = &arguments.voice {
        body["voice"] = json!(voice);
    }
    let response = client
        .post(join(base_url, "audio/speech"))
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await
        .map_err(|_| MediaError::Request)?;
    let bytes = response_bytes(response, MAX_IMAGE_BYTES).await?;
    write_output(arguments.output.as_deref(), &bytes)
}

async fn generate_image(
    client: &Client,
    base_url: &Url,
    api_key: &str,
    arguments: &Arguments,
) -> Result<(), MediaError> {
    if arguments.input_images.len() > 4 {
        return Err(MediaError::Input);
    }
    let prompt = if let Some(prompt) = &arguments.prompt {
        prompt.clone()
    } else if let Some(path) = &arguments.prompt_file {
        String::from_utf8(read_limited(path, MAX_TEXT_BYTES).map_err(|_| MediaError::Input)?)
            .map_err(|_| MediaError::Input)?
    } else {
        return Err(MediaError::Input);
    };
    let model = arguments.model.as_deref().unwrap_or("flux-2-klein");
    let response = if arguments.input_images.is_empty() {
        client
            .post(join(base_url, "images/generations"))
            .bearer_auth(api_key)
            .json(&json!({"model": model, "prompt": prompt, "n": 1}))
            .send()
            .await
            .map_err(|_| MediaError::Request)?
    } else {
        let mut form = Form::new()
            .text("model", model.to_owned())
            .text("prompt", prompt)
            .text("n", "1".to_owned());
        for path in &arguments.input_images {
            let bytes = read_limited(path, MAX_IMAGE_BYTES).map_err(|_| MediaError::Input)?;
            form = form.part("image[]", Part::bytes(bytes).file_name("reference"));
        }
        client
            .post(join(base_url, "images/edits"))
            .bearer_auth(api_key)
            .multipart(form)
            .send()
            .await
            .map_err(|_| MediaError::Request)?
    };
    let payload: ImageResponse = response_json(response).await?;
    let item = payload.data.first().ok_or(MediaError::Response)?;
    let bytes = if let Some(encoded) = &item.b64_json {
        if encoded.len() > (MAX_IMAGE_BYTES / 3) * 4 + 4 {
            return Err(MediaError::Response);
        }
        base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| MediaError::Response)?
    } else if let Some(url) = &item.url {
        let parsed = Url::parse(url).map_err(|_| MediaError::Response)?;
        if !matches!(parsed.scheme(), "http" | "https")
            || parsed.host_str().is_none()
            || parsed.username() != ""
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err(MediaError::Response);
        }
        let response = client
            .get(parsed)
            .send()
            .await
            .map_err(|_| MediaError::Request)?;
        response_bytes(response, MAX_IMAGE_BYTES).await?
    } else {
        return Err(MediaError::Response);
    };
    write_output(arguments.output.as_deref(), &bytes)
}

fn join(base_url: &Url, suffix: &str) -> Url {
    let mut url = base_url.clone();
    let path = format!("{}/{}", base_url.path().trim_end_matches('/'), suffix);
    url.set_path(&path);
    url
}

#[derive(Debug, Deserialize)]
struct TranscriptionResponse {
    text: String,
}

#[derive(Debug, Deserialize)]
struct ImageResponse {
    data: Vec<ImageData>,
}

#[derive(Debug, Deserialize)]
struct ImageData {
    #[serde(default)]
    b64_json: Option<String>,
    #[serde(default)]
    url: Option<String>,
}

async fn response_json<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
) -> Result<T, MediaError> {
    if !response.status().is_success() {
        return Err(MediaError::Request);
    }
    let bytes = response.bytes().await.map_err(|_| MediaError::Request)?;
    if bytes.len() > MAX_JSON_BYTES {
        return Err(MediaError::Response);
    }
    serde_json::from_slice(&bytes).map_err(|_| MediaError::Response)
}

async fn response_bytes(response: reqwest::Response, limit: usize) -> Result<Vec<u8>, MediaError> {
    if !response.status().is_success() {
        return Err(MediaError::Request);
    }
    let bytes = response.bytes().await.map_err(|_| MediaError::Request)?;
    if bytes.len() > limit {
        return Err(MediaError::Response);
    }
    Ok(bytes.to_vec())
}

fn read_limited(path: &Path, limit: usize) -> Result<Vec<u8>, std::io::Error> {
    let limit_u64 = u64::try_from(limit).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "media size limit is not supported on this platform",
        )
    })?;
    if std::fs::metadata(path)?.len() > limit_u64 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "media input exceeds its size limit",
        ));
    }
    let bytes = std::fs::read(path)?;
    if bytes.len() > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "media input exceeds its size limit",
        ));
    }
    Ok(bytes)
}

fn write_output(path: Option<&Path>, bytes: &[u8]) -> Result<(), MediaError> {
    let path = path.ok_or(MediaError::Output)?;
    std::fs::write(path, bytes).map_err(|_| MediaError::Write)
}

fn write_text_or_stdout(path: Option<&Path>, text: &str) -> Result<(), MediaError> {
    if let Some(path) = path {
        write_output_atomically(path, text.as_bytes())
    } else {
        println!("{text}");
        Ok(())
    }
}

fn write_output_atomically(path: &Path, bytes: &[u8]) -> Result<(), MediaError> {
    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    let mut temporary =
        tempfile::NamedTempFile::new_in(directory).map_err(|_| MediaError::Write)?;
    temporary.write_all(bytes).map_err(|_| MediaError::Write)?;
    temporary.persist(path).map_err(|_| MediaError::Write)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{MediaError, endpoint, parse};
    use std::ffi::OsString;

    #[test]
    fn parser_keeps_image_edit_inputs_and_media_options() {
        let arguments = parse(
            [
                "image",
                "--prompt",
                "a blue square",
                "--input-image",
                "first.png",
                "--input-image",
                "second.png",
                "--output",
                "result.png",
                "--model",
                "flux-2-klein",
            ]
            .into_iter()
            .map(OsString::from),
        )
        .expect("image arguments should parse");

        assert_eq!(arguments.command, "image");
        assert_eq!(arguments.input_images.len(), 2);
        assert_eq!(arguments.prompt.as_deref(), Some("a blue square"));
        assert_eq!(arguments.model.as_deref(), Some("flux-2-klein"));
        assert_eq!(
            arguments.output.as_deref().and_then(|path| path.to_str()),
            Some("result.png")
        );
    }

    #[test]
    fn endpoint_rejects_credentials_and_query_parameters() {
        for value in [
            "https://user:secret@example.test/v1",
            "https://example.test/v1?token=secret",
            "file:///tmp/provider",
        ] {
            assert!(matches!(
                endpoint(Some(value)),
                Err(MediaError::InvalidEndpoint)
            ));
        }
        assert!(endpoint(Some("https://example.test/v1")).is_ok());
    }
}
