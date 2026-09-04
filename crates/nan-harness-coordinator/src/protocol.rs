use serde::{Deserialize, Serialize};
use std::io;
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};

pub(crate) const PROTOCOL_VERSION: u8 = 2;
const MAX_FRAME_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointKind {
    Inference,
    Models,
    Search,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestLane {
    Inference,
    Control,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestPriority {
    Foreground,
    Background,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptOutcome {
    Success,
    Cancelled,
    Transport,
    Timeout,
    RateLimited,
    ServerError,
    InvalidResponse,
    Terminal,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Receipt {
    pub(crate) protocol_version: u8,
    pub(crate) port: u16,
    pub(crate) token: String,
    pub(crate) generation: String,
    #[serde(default)]
    pub(crate) pid: u32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum ClientMessage {
    Acquire {
        protocol_version: u8,
        token: String,
        scope: String,
        launch_id: String,
        endpoint: EndpointKind,
        model: Option<String>,
        lane: RequestLane,
        priority: RequestPriority,
    },
    Progress {
        lease_id: u64,
        phase: AttemptPhase,
        elapsed_ms: u64,
    },
    Observe {
        lease_id: u64,
        outcome: AttemptOutcome,
        retry_after_ms: Option<u64>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AttemptPhase {
    HeadersReceived,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum ServerMessage {
    Granted { lease_id: u64, queued_ms: u64 },
    Retry { delay_ms: u64 },
    Complete,
    Rejected { reason: String },
}

pub(crate) async fn write_frame<T>(
    writer: &mut (impl AsyncWrite + Unpin),
    value: &T,
) -> io::Result<()>
where
    T: Serialize,
{
    let payload = serde_json::to_vec(value).map_err(io::Error::other)?;
    if payload.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame is too large",
        ));
    }
    let length = u32::try_from(payload.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "frame is too large"))?;
    writer.write_u32(length).await?;
    writer.write_all(&payload).await?;
    writer.flush().await
}

pub(crate) async fn read_frame<T>(reader: &mut (impl AsyncRead + Unpin)) -> io::Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let length = reader.read_u32().await? as usize;
    if length > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame is too large",
        ));
    }
    let mut payload = vec![0_u8; length];
    reader.read_exact(&mut payload).await?;
    serde_json::from_slice(&payload).map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::{MAX_FRAME_BYTES, ServerMessage, read_frame, write_frame};
    use std::io::ErrorKind;
    use tokio::io::AsyncWriteExt as _;

    #[tokio::test]
    async fn framed_messages_round_trip_without_delimiters() {
        let (mut writer, mut reader) = tokio::io::duplex(1024);
        write_frame(&mut writer, &ServerMessage::Retry { delay_ms: 750 })
            .await
            .expect("frame should write");
        let message = read_frame::<ServerMessage>(&mut reader)
            .await
            .expect("frame should read");
        assert!(matches!(message, ServerMessage::Retry { delay_ms: 750 }));
    }

    #[tokio::test]
    async fn oversized_frames_are_rejected_before_allocating_the_payload() {
        let (mut writer, mut reader) = tokio::io::duplex(16);
        writer
            .write_u32(u32::try_from(MAX_FRAME_BYTES + 1).expect("frame limit fits u32"))
            .await
            .expect("length should write");
        let error = read_frame::<ServerMessage>(&mut reader)
            .await
            .expect_err("oversized frame should fail");
        assert_eq!(error.kind(), ErrorKind::InvalidData);
    }
}
