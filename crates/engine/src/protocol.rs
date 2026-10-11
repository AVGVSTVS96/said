use std::io::{self, Read, Write};

use said_core::{WorkerRequest, WorkerResponse};
use serde::de::DeserializeOwned;

pub const MAX_JSON_BYTES: usize = 1_048_576;
pub const MAX_PCM_BYTES: usize = 1_048_576;

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("frame exceeds {limit} bytes: {length}")]
    Oversized { length: usize, limit: usize },
    #[error("PCM is required exactly for Audio requests")]
    AudioPayload,
    #[error("PCM block has an odd byte count")]
    OddPcm,
}

pub type Result<T> = std::result::Result<T, ProtocolError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestFrame {
    pub message: WorkerRequest,
    pub pcm: Option<Vec<i16>>,
}

pub fn write_request(
    writer: &mut impl Write,
    message: &WorkerRequest,
    pcm: Option<&[i16]>,
) -> Result<()> {
    if matches!(message, WorkerRequest::Audio { .. }) != pcm.is_some() {
        return Err(ProtocolError::AudioPayload);
    }
    let json = serde_json::to_vec(message)?;
    check_length(json.len(), MAX_JSON_BYTES)?;
    if let Some(samples) = pcm {
        check_length(samples.len().saturating_mul(2), MAX_PCM_BYTES)?;
    }
    write_block(writer, &json)?;
    if let Some(samples) = pcm {
        let bytes: Vec<_> = samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect();
        write_block(writer, &bytes)?;
    }
    writer.flush()?;
    Ok(())
}

pub fn read_request(reader: &mut impl Read) -> Result<Option<RequestFrame>> {
    let Some(message) = read_json::<WorkerRequest>(reader)? else {
        return Ok(None);
    };
    let pcm = if matches!(message, WorkerRequest::Audio { .. }) {
        let bytes = read_block(reader, MAX_PCM_BYTES)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "missing PCM block"))?;
        if bytes.len() % 2 != 0 {
            return Err(ProtocolError::OddPcm);
        }
        Some(
            bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
                .collect(),
        )
    } else {
        None
    };
    Ok(Some(RequestFrame { message, pcm }))
}

pub fn write_response(writer: &mut impl Write, message: &WorkerResponse) -> Result<()> {
    let json = serde_json::to_vec(message)?;
    check_length(json.len(), MAX_JSON_BYTES)?;
    write_block(writer, &json)?;
    writer.flush()?;
    Ok(())
}

pub fn read_response(reader: &mut impl Read) -> Result<Option<WorkerResponse>> {
    read_json(reader)
}

fn read_json<T: DeserializeOwned>(reader: &mut impl Read) -> Result<Option<T>> {
    read_block(reader, MAX_JSON_BYTES)?
        .map(|bytes| serde_json::from_slice(&bytes).map_err(Into::into))
        .transpose()
}

fn check_length(length: usize, limit: usize) -> Result<()> {
    if length > limit {
        return Err(ProtocolError::Oversized { length, limit });
    }
    Ok(())
}

fn write_block(writer: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    writer.write_all(&(bytes.len() as u32).to_le_bytes())?;
    writer.write_all(bytes)
}

fn read_block(reader: &mut impl Read, limit: usize) -> Result<Option<Vec<u8>>> {
    let mut prefix = [0; 4];
    loop {
        match reader.read(&mut prefix[..1]) {
            Ok(0) => return Ok(None),
            Ok(_) => break,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.into()),
        }
    }
    reader.read_exact(&mut prefix[1..])?;
    let length = u32::from_le_bytes(prefix) as usize;
    check_length(length, limit)?;
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes)?;
    Ok(Some(bytes))
}
