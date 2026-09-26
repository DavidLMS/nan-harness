//! Media request contracts against a local provider; no live credentials required.
use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::process::Command;
use std::thread;
use std::time::Duration;

fn provider(status: u16, body: &'static [u8]) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("local provider");
    let address = listener.local_addr().expect("provider address");
    let task = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("media request");
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .expect("read timeout");
        let mut request = Vec::new();
        let mut buffer = [0; 4096];
        loop {
            let count = stream.read(&mut buffer).expect("request bytes");
            assert_ne!(count, 0, "request ended before its body");
            request.extend_from_slice(&buffer[..count]);
            assert!(request.len() < 64 * 1024, "bounded synthetic request");
            if let Some(end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..end]).to_lowercase();
                let length: usize = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length: "))
                    .expect("content length")
                    .parse()
                    .expect("numeric length");
                if request.len() >= end + 4 + length {
                    break;
                }
            }
        }
        write!(
            stream,
            "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .expect("response headers");
        stream.write_all(body).expect("response body");
        String::from_utf8(request).expect("synthetic request text")
    });
    (format!("http://{address}/v1"), task)
}

fn request(command: &str, options: &[&str], status: u16, body: &'static [u8]) -> String {
    let directory = tempfile::tempdir().expect("media workspace");
    let input = directory.path().join("input.txt");
    let output = directory.path().join("output");
    std::fs::write(&input, "Synthetic speech").expect("input fixture");
    std::fs::write(&output, "existing output").expect("existing output fixture");
    let (base_url, server) = provider(status, body);
    let result = Command::new(env!("CARGO_BIN_EXE_nanh"))
        .args(["__media", command, "--provider-base-url", &base_url])
        .arg("--input")
        .arg(&input)
        .arg("--output")
        .arg(&output)
        .args(options)
        .env("NAN_API_KEY", "synthetic-media-key")
        .env("NAN_MEDIA_API_KEY", "synthetic-media-key")
        .output()
        .expect("media helper");
    assert_eq!(result.status.success(), status == 200);
    let expected = if status != 200 {
        b"existing output".as_slice()
    } else if command == "image" {
        b"synthetic-image".as_slice()
    } else if command == "stt" {
        b"synthetic transcript".as_slice()
    } else {
        body
    };
    assert_eq!(std::fs::read(&output).expect("output"), expected);
    let request = server.join().expect("provider finished");
    let headers = request.split("\r\n\r\n").next().expect("headers");
    assert!(headers.to_lowercase().contains("user-agent: nan-harness/"));
    assert!(headers.contains("Bearer synthetic-media-key"));
    request
}

#[test]
fn speech_defaults_and_explicit_options_reach_provider() {
    for options in [
        vec![],
        vec![
            "--voice",
            "bf_emma",
            "--model",
            "custom-tts",
            "--format",
            "wav",
        ],
    ] {
        let request = request("tts", &options, 200, b"synthetic audio");
        assert!(request.starts_with("POST /v1/audio/speech "));
        let body: serde_json::Value =
            serde_json::from_str(request.split_once("\r\n\r\n").expect("body").1)
                .expect("speech JSON");
        let expected = if options.is_empty() {
            ["af_heart", "kokoro", "mp3"]
        } else {
            ["bf_emma", "custom-tts", "wav"]
        };
        for (field, value) in ["voice", "model", "response_format"]
            .into_iter()
            .zip(expected)
        {
            assert_eq!(body[field], value);
        }
    }
}

#[test]
fn transcription_uses_nan_model_and_preserves_explicit_override() {
    for options in [vec![], vec!["--model", "custom-stt"]] {
        let request = request("stt", &options, 200, br#"{"text":"synthetic transcript"}"#);
        assert!(request.starts_with("POST /v1/audio/transcriptions "));
        let model = if options.is_empty() {
            "whisper"
        } else {
            "custom-stt"
        };
        assert!(request.contains(&format!("name=\"model\"\r\n\r\n{model}\r\n")));
    }
}

#[test]
fn failed_media_requests_preserve_existing_output() {
    request("tts", &[], 403, b"synthetic provider rejection");
    request("stt", &[], 401, b"synthetic provider rejection");
}

#[test]
fn image_defaults_and_overrides_reach_the_provider() {
    for (options, model) in [
        (vec!["--prompt", "synthetic"], "flux-2-klein"),
        (
            vec!["--prompt", "synthetic", "--model", "qwen-image-2.1"],
            "qwen-image-2.1",
        ),
    ] {
        let request = request(
            "image",
            &options,
            200,
            br#"{"data":[{"b64_json":"c3ludGhldGljLWltYWdl"}]}"#,
        );
        assert!(request.starts_with("POST /v1/images/generations "));
        let body: serde_json::Value =
            serde_json::from_str(request.split_once("\r\n\r\n").expect("body").1).expect("JSON");
        assert_eq!(body["model"], model);
    }
}

#[test]
fn unsupported_qwen_edits_preserve_the_existing_output() {
    let directory = tempfile::tempdir().expect("workspace");
    let output = directory.path().join("output.png");
    std::fs::write(&output, b"existing image").expect("existing output");
    let result = Command::new(env!("CARGO_BIN_EXE_nanh"))
        .args([
            "__media",
            "image",
            "--provider-base-url",
            "http://127.0.0.1:1/v1",
            "--model",
            "qwen-image-2.1",
            "--prompt",
            "synthetic edit",
            "--input-image",
        ])
        .arg(&output)
        .arg("--output")
        .arg(&output)
        .env("NAN_MEDIA_API_KEY", "synthetic-key")
        .output()
        .expect("helper");
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("flux-2-klein"));
    assert_eq!(
        std::fs::read(output).expect("preserved file"),
        b"existing image"
    );
}
