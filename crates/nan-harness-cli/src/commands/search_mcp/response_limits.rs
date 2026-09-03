const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

pub(super) async fn read_response_body(
    response: &mut reqwest::Response,
) -> Result<Vec<u8>, &'static str> {
    if response
        .content_length()
        .is_some_and(|size| size > MAX_RESPONSE_BYTES as u64)
    {
        return Err("NH-SEARCH-MCP-008");
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "NH-SEARCH-MCP-006")? {
        append_response_chunk(&mut body, &chunk)?;
    }
    Ok(body)
}

fn append_response_chunk(body: &mut Vec<u8>, chunk: &[u8]) -> Result<(), &'static str> {
    if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
        return Err("NH-SEARCH-MCP-008");
    }
    body.extend_from_slice(chunk);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{MAX_RESPONSE_BYTES, append_response_chunk};

    #[test]
    fn response_limit_accepts_the_exact_boundary() {
        let mut body = vec![b'x'; MAX_RESPONSE_BYTES - 1];

        assert_eq!(append_response_chunk(&mut body, b"y"), Ok(()));
        assert_eq!(body.len(), MAX_RESPONSE_BYTES);
    }

    #[test]
    fn response_limit_rejects_a_crossing_chunk_without_appending_it() {
        let mut body = vec![b'x'; MAX_RESPONSE_BYTES - 1];

        assert_eq!(
            append_response_chunk(&mut body, b"yz"),
            Err("NH-SEARCH-MCP-008")
        );
        assert_eq!(body.len(), MAX_RESPONSE_BYTES - 1);
        assert_eq!(body.last(), Some(&b'x'));
    }

    #[test]
    fn response_limit_rejects_overflowing_lengths() {
        let mut body = vec![b'x'; MAX_RESPONSE_BYTES];

        assert_eq!(
            append_response_chunk(&mut body, b"z"),
            Err("NH-SEARCH-MCP-008")
        );
        assert_eq!(body.len(), MAX_RESPONSE_BYTES);
    }
}
