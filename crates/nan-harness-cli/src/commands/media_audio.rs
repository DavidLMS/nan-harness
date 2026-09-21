use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use thiserror::Error;

pub(crate) const MAX_AUDIO_REQUEST_BYTES: usize = 24 * 1024 * 1024;

const CHUNK_TARGET_BYTES: usize = 20 * 1024 * 1024;
const CHUNK_OVERLAP_FRAMES: usize = 16_000;
const WAV_HEADER_BYTES: usize = 44;
const NORMALIZED_SAMPLE_RATE: u32 = 16_000;
const READ_FRAMES: usize = 4_096;
const WRITE_BUFFER_BYTES: usize = 64 * 1024;
const MAX_MERGE_TOKENS: usize = 64;
const MIN_MERGE_OVERLAP: usize = 1;

#[derive(Debug, Error)]
pub(crate) enum AudioError {
    #[error("audio input could not be read")]
    Io(#[source] io::Error),
    #[error("large audio input must be a supported PCM WAV file")]
    Format,
    #[error("audio chunks could not be written")]
    Write(#[source] io::Error),
}

#[derive(Clone, Copy)]
struct ChunkPolicy {
    target_bytes: usize,
    max_bytes: usize,
    overlap_frames: usize,
}

struct WavSpec {
    channels: u16,
    sample_rate: u32,
    bits_per_sample: u16,
    block_align: u16,
    data_offset: u64,
    data_bytes: u64,
}

struct PcmWavReader {
    file: File,
    spec: WavSpec,
    remaining_frames: u64,
}

struct LinearResampler {
    step: f64,
    next_position: f64,
    input_position: f64,
    previous: Option<f32>,
}

struct ChunkWriter {
    file: File,
    path: PathBuf,
    frames: usize,
    data_bytes: u64,
    buffer: Vec<u8>,
}

#[derive(Debug)]
struct MergeToken {
    start: usize,
    normalized: String,
}

pub(crate) fn prepare_chunks(path: &Path) -> Result<(tempfile::TempDir, Vec<PathBuf>), AudioError> {
    prepare_chunks_with_policy(
        path,
        ChunkPolicy {
            target_bytes: CHUNK_TARGET_BYTES,
            max_bytes: MAX_AUDIO_REQUEST_BYTES,
            overlap_frames: CHUNK_OVERLAP_FRAMES,
        },
    )
}

pub(crate) fn merge_transcripts(parts: &[String]) -> String {
    let mut merged = String::new();
    for part in parts {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if merged.is_empty() {
            merged.push_str(part);
        } else {
            merged = merge_pair(&merged, part);
        }
    }
    merged
}

fn prepare_chunks_with_policy(
    path: &Path,
    policy: ChunkPolicy,
) -> Result<(tempfile::TempDir, Vec<PathBuf>), AudioError> {
    validate_policy(policy)?;
    let mut file = File::open(path).map_err(AudioError::Io)?;
    let spec = parse_wav(&mut file)?;
    let mut reader = PcmWavReader::new(file, spec)?;
    let temp_dir = tempfile::tempdir().map_err(AudioError::Write)?;
    let mut chunks = Vec::new();
    let mut current = None;
    let mut tail = VecDeque::with_capacity(policy.overlap_frames);
    let capacity_frames = (policy.target_bytes - WAV_HEADER_BYTES) / 2;
    let mut resampler = LinearResampler::new(reader.spec.sample_rate);

    while let Some(samples) = reader.read_mono_frames(READ_FRAMES)? {
        for sample in samples {
            for resampled_sample in resampler.push(sample) {
                let pcm_sample = pcm_i16(resampled_sample);
                if current
                    .as_ref()
                    .is_some_and(|writer: &ChunkWriter| writer.frames() >= capacity_frames)
                {
                    let finished = current.take().ok_or(AudioError::Format)?;
                    chunks.push(finished.finish()?);
                    let mut next = ChunkWriter::new(
                        temp_dir
                            .path()
                            .join(format!("chunk-{:04}.wav", chunks.len())),
                    )?;
                    let overlap: Vec<i16> = tail.iter().copied().collect();
                    next.write_samples(&overlap)?;
                    current = Some(next);
                }
                if current.is_none() {
                    current = Some(ChunkWriter::new(
                        temp_dir
                            .path()
                            .join(format!("chunk-{:04}.wav", chunks.len())),
                    )?);
                }
                current
                    .as_mut()
                    .ok_or(AudioError::Format)?
                    .write_sample(pcm_sample)?;
                if policy.overlap_frames > 0 {
                    if tail.len() == policy.overlap_frames {
                        tail.pop_front();
                    }
                    tail.push_back(pcm_sample);
                }
            }
        }
    }

    if let Some(writer) = current {
        chunks.push(writer.finish()?);
    }
    if chunks.is_empty() {
        return Err(AudioError::Format);
    }
    Ok((temp_dir, chunks))
}

fn validate_policy(policy: ChunkPolicy) -> Result<(), AudioError> {
    if policy.target_bytes > policy.max_bytes
        || policy.target_bytes <= WAV_HEADER_BYTES + 2
        || policy.overlap_frames > (policy.target_bytes - WAV_HEADER_BYTES) / 2
    {
        return Err(AudioError::Format);
    }
    Ok(())
}

impl PcmWavReader {
    fn new(file: File, spec: WavSpec) -> Result<Self, AudioError> {
        let remaining_frames = spec.data_bytes / u64::from(spec.block_align);
        let mut file = file;
        file.seek(SeekFrom::Start(spec.data_offset))
            .map_err(AudioError::Io)?;
        Ok(Self {
            file,
            spec,
            remaining_frames,
        })
    }

    fn read_mono_frames(&mut self, requested: usize) -> Result<Option<Vec<f32>>, AudioError> {
        if self.remaining_frames == 0 {
            return Ok(None);
        }
        let requested = u64::try_from(requested).map_err(|_| AudioError::Format)?;
        let frames = usize::try_from(self.remaining_frames.min(requested))
            .map_err(|_| AudioError::Format)?;
        let byte_count = frames
            .checked_mul(usize::from(self.spec.block_align))
            .ok_or(AudioError::Format)?;
        let mut bytes = vec![0; byte_count];
        self.file.read_exact(&mut bytes).map_err(AudioError::Io)?;
        self.remaining_frames -= frames as u64;

        let channels = usize::from(self.spec.channels);
        let sample_bytes = usize::from(self.spec.bits_per_sample / 8);
        let mut mono = Vec::with_capacity(frames);
        for frame in bytes.chunks_exact(usize::from(self.spec.block_align)) {
            let mut sum = 0.0;
            for channel in frame.chunks_exact(sample_bytes).take(channels) {
                sum += decode_sample(channel, self.spec.bits_per_sample);
            }
            mono.push(sum / f32::from(self.spec.channels));
        }
        Ok(Some(mono))
    }
}

impl LinearResampler {
    fn new(input_rate: u32) -> Self {
        Self {
            step: f64::from(input_rate) / f64::from(NORMALIZED_SAMPLE_RATE),
            next_position: 0.0,
            input_position: 0.0,
            previous: None,
        }
    }

    fn push(&mut self, current: f32) -> Vec<f32> {
        let current_position = self.input_position;
        let mut output = Vec::new();
        while self.next_position <= current_position {
            let value = match self.previous {
                Some(previous) if self.input_position > 0.0 => {
                    let fraction = (self.next_position - (current_position - 1.0)).clamp(0.0, 1.0);
                    #[allow(clippy::cast_possible_truncation)]
                    let fraction = fraction as f32;
                    previous + (current - previous) * fraction
                }
                _ => current,
            };
            output.push(value);
            self.next_position += self.step;
        }
        self.previous = Some(current);
        self.input_position += 1.0;
        output
    }
}

impl ChunkWriter {
    fn new(path: PathBuf) -> Result<Self, AudioError> {
        let mut file = File::create(&path).map_err(AudioError::Write)?;
        write_wav_header(&mut file).map_err(AudioError::Write)?;
        Ok(Self {
            file,
            path,
            frames: 0,
            data_bytes: 0,
            buffer: Vec::with_capacity(WRITE_BUFFER_BYTES),
        })
    }

    fn frames(&self) -> usize {
        self.frames
    }

    fn write_sample(&mut self, sample: i16) -> Result<(), AudioError> {
        self.buffer.extend_from_slice(&sample.to_le_bytes());
        self.frames += 1;
        self.data_bytes += 2;
        if self.buffer.len() >= WRITE_BUFFER_BYTES {
            self.flush_buffer()?;
        }
        Ok(())
    }

    fn write_samples(&mut self, samples: &[i16]) -> Result<(), AudioError> {
        for sample in samples {
            self.write_sample(*sample)?;
        }
        Ok(())
    }

    fn flush_buffer(&mut self) -> Result<(), AudioError> {
        self.file
            .write_all(&self.buffer)
            .map_err(AudioError::Write)?;
        self.buffer.clear();
        Ok(())
    }

    fn finish(mut self) -> Result<PathBuf, AudioError> {
        self.flush_buffer()?;
        let riff_size = 36u64
            .checked_add(self.data_bytes)
            .ok_or(AudioError::Format)?;
        let riff_size = u32::try_from(riff_size).map_err(|_| AudioError::Format)?;
        let data_size = u32::try_from(self.data_bytes).map_err(|_| AudioError::Format)?;
        self.file
            .seek(SeekFrom::Start(4))
            .and_then(|_| self.file.write_all(&riff_size.to_le_bytes()))
            .and_then(|()| self.file.seek(SeekFrom::Start(40)))
            .and_then(|_| self.file.write_all(&data_size.to_le_bytes()))
            .and_then(|()| self.file.flush())
            .map_err(AudioError::Write)?;
        Ok(self.path)
    }
}

fn parse_wav(file: &mut File) -> Result<WavSpec, AudioError> {
    let file_len = file.metadata().map_err(AudioError::Io)?.len();
    let mut header = [0; 12];
    file.read_exact(&mut header)
        .map_err(|_| AudioError::Format)?;
    if &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" {
        return Err(AudioError::Format);
    }

    let mut fmt = None;
    let mut data = None;
    let mut position = 12u64;
    while position.checked_add(8).is_some_and(|end| end <= file_len) {
        file.seek(SeekFrom::Start(position))
            .map_err(AudioError::Io)?;
        let mut chunk_header = [0; 8];
        file.read_exact(&mut chunk_header)
            .map_err(|_| AudioError::Format)?;
        let chunk_bytes = u64::from(u32::from_le_bytes([
            chunk_header[4],
            chunk_header[5],
            chunk_header[6],
            chunk_header[7],
        ]));
        let chunk_data = position.checked_add(8).ok_or(AudioError::Format)?;
        let chunk_end = chunk_data
            .checked_add(chunk_bytes)
            .and_then(|end| end.checked_add(chunk_bytes % 2))
            .ok_or(AudioError::Format)?;
        if chunk_end > file_len {
            return Err(AudioError::Format);
        }
        match &chunk_header[0..4] {
            b"fmt " => fmt = Some(parse_fmt(file, chunk_data, chunk_bytes)?),
            b"data" if data.is_none() => data = Some((chunk_data, chunk_bytes)),
            _ => {}
        }
        position = chunk_end;
    }

    let (channels, sample_rate, bits_per_sample, block_align) = fmt.ok_or(AudioError::Format)?;
    let (data_offset, data_bytes) = data.ok_or(AudioError::Format)?;
    if channels == 0
        || sample_rate == 0
        || !matches!(bits_per_sample, 8 | 16 | 24 | 32)
        || block_align != channels.saturating_mul(bits_per_sample / 8)
        || data_bytes == 0
        || data_bytes % u64::from(block_align) != 0
    {
        return Err(AudioError::Format);
    }
    Ok(WavSpec {
        channels,
        sample_rate,
        bits_per_sample,
        block_align,
        data_offset,
        data_bytes,
    })
}

fn parse_fmt(
    file: &mut File,
    offset: u64,
    chunk_bytes: u64,
) -> Result<(u16, u32, u16, u16), AudioError> {
    if chunk_bytes < 16 {
        return Err(AudioError::Format);
    }
    file.seek(SeekFrom::Start(offset)).map_err(AudioError::Io)?;
    let mut fmt = [0; 16];
    file.read_exact(&mut fmt).map_err(|_| AudioError::Format)?;
    let format = u16::from_le_bytes([fmt[0], fmt[1]]);
    if format != 1 {
        return Err(AudioError::Format);
    }
    Ok((
        u16::from_le_bytes([fmt[2], fmt[3]]),
        u32::from_le_bytes([fmt[4], fmt[5], fmt[6], fmt[7]]),
        u16::from_le_bytes([fmt[14], fmt[15]]),
        u16::from_le_bytes([fmt[12], fmt[13]]),
    ))
}

fn decode_sample(sample: &[u8], bits_per_sample: u16) -> f32 {
    match bits_per_sample {
        8 => (f32::from(sample[0]) - 128.0) / 128.0,
        16 => f32::from(i16::from_le_bytes([sample[0], sample[1]])) / 32_768.0,
        24 => {
            let value =
                i32::from(sample[0]) | (i32::from(sample[1]) << 8) | (i32::from(sample[2]) << 16);
            let value = if value & 0x0080_0000 != 0 {
                value | !0x00ff_ffff
            } else {
                value
            };
            #[allow(clippy::cast_precision_loss)]
            {
                value as f32 / 8_388_608.0
            }
        }
        32 => {
            #[allow(clippy::cast_precision_loss)]
            {
                i32::from_le_bytes([sample[0], sample[1], sample[2], sample[3]]) as f32
                    / 2_147_483_648.0
            }
        }
        _ => 0.0,
    }
}

fn pcm_i16(sample: f32) -> i16 {
    let sample = sample.clamp(-1.0, 1.0) * 32_767.0;
    if sample <= f32::from(i16::MIN) {
        i16::MIN
    } else if sample >= f32::from(i16::MAX) {
        i16::MAX
    } else {
        #[allow(clippy::cast_possible_truncation)]
        {
            sample.round() as i16
        }
    }
}

fn write_wav_header(file: &mut File) -> io::Result<()> {
    file.write_all(b"RIFF")?;
    file.write_all(&0u32.to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16u32.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&NORMALIZED_SAMPLE_RATE.to_le_bytes())?;
    file.write_all(&(NORMALIZED_SAMPLE_RATE * 2).to_le_bytes())?;
    file.write_all(&2u16.to_le_bytes())?;
    file.write_all(&16u16.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&0u32.to_le_bytes())
}

fn merge_pair(left: &str, right: &str) -> String {
    let left_tokens = merge_tokens(left);
    let right_tokens = merge_tokens(right);
    let overlap = find_overlap(&left_tokens, &right_tokens);
    let right_start = right_tokens
        .get(overlap)
        .map_or(right.len(), |token| token.start);
    let remainder = right[right_start..].trim_start();
    if remainder.is_empty() {
        left.trim_end().to_owned()
    } else {
        format!("{} {remainder}", left.trim_end())
    }
}

fn merge_tokens(text: &str) -> Vec<MergeToken> {
    let mut cursor = 0;
    text.split_whitespace()
        .map(|token| {
            let start = cursor + text[cursor..].find(token).unwrap_or(0);
            cursor = start + token.len();
            MergeToken {
                start,
                normalized: normalize_token(token),
            }
        })
        .collect()
}

fn normalize_token(token: &str) -> String {
    let normalized: String = token
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    if normalized.is_empty() {
        token.to_lowercase()
    } else {
        normalized
    }
}

fn find_overlap(left: &[MergeToken], right: &[MergeToken]) -> usize {
    let maximum = left.len().min(right.len()).min(MAX_MERGE_TOKENS);
    for length in (MIN_MERGE_OVERLAP..=maximum).rev() {
        let left_start = left.len() - length;
        if left[left_start..]
            .iter()
            .zip(&right[..length])
            .all(|(left, right)| left.normalized == right.normalized)
        {
            return length;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::{
        AudioError, ChunkPolicy, MAX_AUDIO_REQUEST_BYTES, decode_sample, merge_transcripts,
        prepare_chunks_with_policy,
    };
    use std::fs;
    use std::io::Write;
    use std::path::Path;

    #[test]
    fn rejects_non_pcm_wav_inputs() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("audio.bin");
        fs::write(&path, b"not a wav").expect("input should be written");

        assert!(matches!(
            prepare_chunks_with_policy(
                &path,
                ChunkPolicy {
                    target_bytes: 256,
                    max_bytes: MAX_AUDIO_REQUEST_BYTES,
                    overlap_frames: 4,
                }
            ),
            Err(AudioError::Format)
        ));
    }

    #[test]
    fn converts_multichannel_wav_and_keeps_chunks_below_limit() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("stereo.wav");
        write_pcm_wav(&path, 2, 8_000, 16, 10_000);

        let (chunks_dir, chunks) = prepare_chunks_with_policy(
            &path,
            ChunkPolicy {
                target_bytes: 512,
                max_bytes: 768,
                overlap_frames: 4,
            },
        )
        .expect("WAV should be converted");
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(fs::metadata(chunk).expect("chunk metadata").len() <= 512);
            assert_eq!(&fs::read(chunk).expect("chunk bytes")[0..4], b"RIFF");
        }
        assert!(chunks_dir.path().exists());
    }

    #[test]
    fn merges_overlapping_transcription_boundaries() {
        let parts = vec![
            String::from("the quick brown fox"),
            String::from("brown fox jumps over"),
            String::from("over the lazy dog"),
        ];
        assert_eq!(
            merge_transcripts(&parts),
            "the quick brown fox jumps over the lazy dog"
        );
    }

    #[test]
    fn skips_empty_transcription_parts() {
        assert_eq!(
            merge_transcripts(&[String::new(), String::from("hello")]),
            "hello"
        );
    }

    #[test]
    fn decodes_supported_pcm_widths() {
        assert!((decode_sample(&[0], 8) + 1.0).abs() < 0.01);
        assert!((decode_sample(&[0, 0x40], 16) - 0.5).abs() < 0.01);
        assert!((decode_sample(&[0, 0, 0x40], 24) - 0.5).abs() < 0.01);
        assert!((decode_sample(&[0, 0, 0, 0x40], 32) - 0.5).abs() < 0.01);
    }

    fn write_pcm_wav(path: &Path, channels: u16, sample_rate: u32, bits: u16, frames: usize) {
        let bytes_per_sample = usize::from(bits / 8);
        let data_bytes = frames * usize::from(channels) * bytes_per_sample;
        let data_bytes_u32 = u32::try_from(data_bytes).unwrap_or(0);
        let mut file = fs::File::create(path).expect("WAV should be created");
        file.write_all(b"RIFF").expect("header should be written");
        file.write_all(&(36u32 + data_bytes_u32).to_le_bytes())
            .expect("header should be written");
        file.write_all(b"WAVEfmt ")
            .expect("header should be written");
        file.write_all(&16u32.to_le_bytes())
            .expect("header should be written");
        file.write_all(&1u16.to_le_bytes())
            .expect("header should be written");
        file.write_all(&channels.to_le_bytes())
            .expect("header should be written");
        file.write_all(&sample_rate.to_le_bytes())
            .expect("header should be written");
        file.write_all(&(sample_rate * u32::from(channels) * u32::from(bits / 8)).to_le_bytes())
            .expect("header should be written");
        file.write_all(&(channels * (bits / 8)).to_le_bytes())
            .expect("header should be written");
        file.write_all(&bits.to_le_bytes())
            .expect("header should be written");
        file.write_all(b"data").expect("header should be written");
        file.write_all(&data_bytes_u32.to_le_bytes())
            .expect("header should be written");
        for frame in 0..frames {
            for channel in 0..usize::from(channels) {
                let value = if channel == 0 {
                    i16::try_from(frame).unwrap_or(i16::MAX)
                } else {
                    0
                };
                file.write_all(&value.to_le_bytes())
                    .expect("sample should be written");
            }
        }
    }
}
