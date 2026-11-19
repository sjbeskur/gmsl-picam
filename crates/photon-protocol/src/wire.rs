//! UDP packets use bounded UTF-8 JSON. Stream postcards use a fixed header,
//! bounded JSON metadata, then raw JPEG bytes (no base64 or Rust memory layout).
use crate::{DaemonStatus, FrameMetadata, SettingsPatch};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    io::{self, Read, Write},
    sync::Arc,
};

pub const VERSION: u16 = 1;
pub const MAX_COMMAND_BYTES: usize = 1200;
pub const MAX_RESPONSE_BYTES: usize = 4096;
pub const MAX_METADATA_BYTES: usize = 16 * 1024;
pub const MAX_IMAGE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "settings",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Command {
    Ping,
    Status,
    Start,
    Stop,
    Settings(SettingsPatch),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandPacket {
    pub version: u16,
    pub request_id: u64,
    pub command: Command,
}
impl CommandPacket {
    pub fn new(request_id: u64, command: Command) -> Self {
        Self {
            version: VERSION,
            request_id,
            command,
        }
    }
    pub fn encode(&self) -> io::Result<Vec<u8>> {
        check_version(self.version)?;
        encode_json(self, MAX_COMMAND_BYTES)
    }
    pub fn decode(bytes: &[u8]) -> io::Result<Self> {
        let packet: Self = decode_json(bytes, MAX_COMMAND_BYTES)?;
        check_version(packet.version)?;
        Ok(packet)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Invalid,
    Conflict,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum CommandResult {
    Pong,
    Status(Box<DaemonStatus>),
    Error { code: ErrorCode, message: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandResponse {
    pub version: u16,
    pub request_id: u64,
    pub result: CommandResult,
}
impl CommandResponse {
    pub fn new(request_id: u64, result: CommandResult) -> Self {
        Self {
            version: VERSION,
            request_id,
            result,
        }
    }
    pub fn encode(&self) -> io::Result<Vec<u8>> {
        check_version(self.version)?;
        encode_json(self, MAX_RESPONSE_BYTES)
    }
    pub fn decode(bytes: &[u8]) -> io::Result<Self> {
        let response: Self = decode_json(bytes, MAX_RESPONSE_BYTES)?;
        check_version(response.version)?;
        Ok(response)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImagePostcard {
    pub metadata: FrameMetadata,
    pub jpeg: Arc<[u8]>,
}

fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}
fn check_version(version: u16) -> io::Result<()> {
    if version != VERSION {
        return Err(invalid("unsupported photon protocol version"));
    }
    Ok(())
}
fn encode_json<T: Serialize>(value: &T, limit: usize) -> io::Result<Vec<u8>> {
    let bytes = serde_json::to_vec(value).map_err(invalid)?;
    if bytes.len() > limit {
        return Err(invalid("photon packet exceeds size limit"));
    }
    Ok(bytes)
}
fn decode_json<T: DeserializeOwned>(bytes: &[u8], limit: usize) -> io::Result<T> {
    if bytes.len() > limit {
        return Err(invalid("photon packet exceeds size limit"));
    }
    serde_json::from_slice(bytes).map_err(invalid)
}

/// Header: PHTN (4), version u16, encoding u16 (1=JPEG), JSON length u32,
/// JPEG length u32. All integers are big endian. No native struct bytes are sent.
pub fn write_postcard(writer: &mut impl Write, postcard: &ImagePostcard) -> io::Result<()> {
    if postcard.jpeg.is_empty() || postcard.jpeg.len() > MAX_IMAGE_BYTES {
        return Err(invalid("invalid JPEG payload length"));
    }
    let metadata = encode_json(&postcard.metadata, MAX_METADATA_BYTES)?;
    let mut header = [0u8; 16];
    header[..4].copy_from_slice(b"PHTN");
    header[4..6].copy_from_slice(&VERSION.to_be_bytes());
    header[6..8].copy_from_slice(&1u16.to_be_bytes());
    header[8..12].copy_from_slice(&(metadata.len() as u32).to_be_bytes());
    header[12..16].copy_from_slice(&(postcard.jpeg.len() as u32).to_be_bytes());
    writer.write_all(&header)?;
    writer.write_all(&metadata)?;
    writer.write_all(&postcard.jpeg)?;
    writer.flush()
}

pub fn read_postcard(reader: &mut impl Read) -> io::Result<ImagePostcard> {
    let mut header = [0u8; 16];
    reader.read_exact(&mut header)?;
    if &header[..4] != b"PHTN" {
        return Err(invalid("invalid photon stream magic"));
    }
    check_version(u16::from_be_bytes(header[4..6].try_into().unwrap()))?;
    if u16::from_be_bytes(header[6..8].try_into().unwrap()) != 1 {
        return Err(invalid("unsupported image encoding"));
    }
    let metadata_len = u32::from_be_bytes(header[8..12].try_into().unwrap()) as usize;
    let image_len = u32::from_be_bytes(header[12..16].try_into().unwrap()) as usize;
    if metadata_len == 0
        || metadata_len > MAX_METADATA_BYTES
        || image_len == 0
        || image_len > MAX_IMAGE_BYTES
    {
        return Err(invalid("invalid photon stream lengths"));
    }
    let mut metadata = vec![0; metadata_len];
    reader.read_exact(&mut metadata)?;
    let metadata = decode_json(&metadata, MAX_METADATA_BYTES)?;
    let mut jpeg = vec![0; image_len];
    reader.read_exact(&mut jpeg)?;
    Ok(ImagePostcard {
        metadata,
        jpeg: Arc::from(jpeg),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame() -> ImagePostcard {
        ImagePostcard {
            jpeg: Arc::from([255, 216, 255, 217]),
            metadata: FrameMetadata {
                frame_index: 9,
                sensor_ts_ns: 123,
                exposure_us: Some(59000),
                analogue_gain: Some(1.5),
                coi_ns: None,
                first_row_coi_ns: None,
                frame_duration_us: Some(60985),
                width: 1280,
                height: 720,
                pixel_format: "NV12".into(),
                captured_at_unix_ms: 12345,
            },
        }
    }
    #[test]
    fn command_wire_format_and_bounds_are_explicit() {
        let packet = CommandPacket::new(42, Command::Ping);
        assert_eq!(
            packet.encode().unwrap(),
            br#"{"version":1,"request_id":42,"command":{"type":"ping"}}"#
        );
        assert_eq!(
            CommandPacket::decode(&packet.encode().unwrap()).unwrap(),
            packet
        );
        assert!(CommandPacket::decode(
            br#"{"version":2,"request_id":42,"command":{"type":"ping"}}"#
        )
        .is_err());
        assert!(CommandPacket::decode(
            br#"{"version":1,"request_id":42,"command":{"type":"explode"}}"#
        )
        .is_err());
        assert!(CommandPacket::decode(&vec![b' '; MAX_COMMAND_BYTES + 1]).is_err());
        let reply = CommandResponse::new(
            42,
            CommandResult::Error {
                code: ErrorCode::Conflict,
                message: "stop first".into(),
            },
        );
        assert_eq!(
            CommandResponse::decode(&reply.encode().unwrap()).unwrap(),
            reply
        );
    }
    #[test]
    fn fragmented_streams_preserve_metadata_image_and_frame_boundaries() {
        let expected = frame();
        let mut bytes = Vec::new();
        write_postcard(&mut bytes, &expected).unwrap();
        write_postcard(&mut bytes, &expected).unwrap();
        assert_eq!(&bytes[..8], b"PHTN\x00\x01\x00\x01");
        struct Fragments<'a>(&'a [u8]);
        impl Read for Fragments<'_> {
            fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
                let n = out.len().min(3).min(self.0.len());
                out[..n].copy_from_slice(&self.0[..n]);
                self.0 = &self.0[n..];
                Ok(n)
            }
        }
        let mut reader = Fragments(&bytes);
        assert_eq!(read_postcard(&mut reader).unwrap(), expected);
        assert_eq!(read_postcard(&mut reader).unwrap(), expected);
        assert!(reader.0.is_empty());
        for length in [1, 15, 16, bytes.len() / 2 - 1] {
            assert!(read_postcard(&mut &bytes[..length]).is_err());
        }
    }
    #[test]
    fn rejects_invalid_headers_before_allocating_or_reading_payloads() {
        let mut bytes = Vec::new();
        write_postcard(&mut bytes, &frame()).unwrap();
        for (offset, replacement) in [
            (0, b"BAD!".to_vec()),
            (4, 2u16.to_be_bytes().to_vec()),
            (6, 2u16.to_be_bytes().to_vec()),
            (8, u32::MAX.to_be_bytes().to_vec()),
            (12, u32::MAX.to_be_bytes().to_vec()),
        ] {
            let mut bad = bytes.clone();
            bad[offset..offset + replacement.len()].copy_from_slice(&replacement);
            let mut reader = io::Cursor::new(bad);
            assert!(read_postcard(&mut reader).is_err());
            assert_eq!(reader.position(), 16);
        }
    }
}
