use super::super::{PersistenceError, discover_models};
use nan_harness_core::SecretValue;
use nan_harness_runtime::{ConfigOverrides, ConfigResolver, ProcessEnvironment};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

const MAX_TEST_MODELS_RESPONSE_BYTES: usize = 1024 * 1024;

enum RawResponseBody {
    ContentLength { declared: usize, body: Vec<u8> },
    Chunked(Vec<Vec<u8>>),
}

async fn discover_from_raw_response(
    status: u16,
    body: RawResponseBody,
) -> Result<Vec<nan_harness_core::CodingModelProfile>, PersistenceError> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("raw provider should bind");
    let address = listener.local_addr().expect("raw provider address");
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("request should arrive");
        let mut request = Vec::new();
        let mut buffer = [0_u8; 1024];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            let read = stream.read(&mut buffer).await.expect("request should read");
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
        }
        let reason = if status == 200 { "OK" } else { "Error" };
        match body {
            RawResponseBody::ContentLength { declared, body } => {
                stream
                    .write_all(
                        format!(
                            "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {declared}\r\nConnection: close\r\n\r\n"
                        )
                        .as_bytes(),
                    )
                    .await
                    .expect("response headers should write");
                stream
                    .write_all(&body)
                    .await
                    .expect("response body should write");
            }
            RawResponseBody::Chunked(chunks) => {
                stream
                    .write_all(
                        format!(
                            "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n"
                        )
                        .as_bytes(),
                    )
                    .await
                    .expect("response headers should write");
                for chunk in chunks {
                    stream
                        .write_all(format!("{:x}\r\n", chunk.len()).as_bytes())
                        .await
                        .expect("chunk header should write");
                    stream.write_all(&chunk).await.expect("chunk should write");
                    stream
                        .write_all(b"\r\n")
                        .await
                        .expect("chunk terminator should write");
                }
                stream
                    .write_all(b"0\r\n\r\n")
                    .await
                    .expect("response should finish");
            }
        }
    });
    let config = ConfigResolver::resolve(
        &ProcessEnvironment,
        ConfigOverrides {
            provider_base_url: Some(format!("http://{address}/v1")),
            nan_api_key: Some(
                SecretValue::new("test-api-key").expect("test credential should be valid"),
            ),
        },
    )
    .expect("test configuration should resolve");
    let result = discover_models(&config).await;
    task.await.expect("raw provider should finish");
    result
}

fn padded_test_catalog(size: usize) -> Vec<u8> {
    let mut body = br#"{"data":[{"id":"qwen3.6"}]}"#.to_vec();
    assert!(body.len() <= size, "requested test body is too small");
    body.resize(size, b' ');
    body
}

#[tokio::test]
async fn model_discovery_bounds_success_responses() {
    let small = padded_test_catalog(64);
    let models = discover_from_raw_response(
        200,
        RawResponseBody::ContentLength {
            declared: small.len(),
            body: small,
        },
    )
    .await
    .expect("small catalog should be accepted");
    assert_eq!(models[0].id, "qwen3.6");

    let declared = discover_from_raw_response(
        200,
        RawResponseBody::ContentLength {
            declared: MAX_TEST_MODELS_RESPONSE_BYTES + 1,
            body: Vec::new(),
        },
    )
    .await
    .expect_err("oversized declared response should be rejected");
    assert!(matches!(declared, PersistenceError::ModelDiscoveryTooLarge));

    let oversized = padded_test_catalog(MAX_TEST_MODELS_RESPONSE_BYTES + 1);
    let chunked = discover_from_raw_response(
        200,
        RawResponseBody::Chunked(vec![
            oversized[..MAX_TEST_MODELS_RESPONSE_BYTES].to_vec(),
            oversized[MAX_TEST_MODELS_RESPONSE_BYTES..].to_vec(),
        ]),
    )
    .await
    .expect_err("oversized chunked response should be rejected");
    assert!(matches!(chunked, PersistenceError::ModelDiscoveryTooLarge));

    let invalid = discover_from_raw_response(
        200,
        RawResponseBody::ContentLength {
            declared: 8,
            body: b"not-json".to_vec(),
        },
    )
    .await
    .expect_err("invalid response should be rejected");
    assert!(matches!(invalid, PersistenceError::ParseModels(_)));

    let boundary = padded_test_catalog(MAX_TEST_MODELS_RESPONSE_BYTES);
    let models = discover_from_raw_response(
        200,
        RawResponseBody::ContentLength {
            declared: boundary.len(),
            body: boundary,
        },
    )
    .await
    .expect("response at the exact boundary should be accepted");
    assert_eq!(models[0].id, "qwen3.6");
}
