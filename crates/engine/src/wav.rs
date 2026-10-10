use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use voice_core::Result;

pub struct Wav {
    file: File,
    remaining: u64,
}

impl Wav {
    pub fn open(path: &Path) -> Result<Self> {
        let mut file = File::open(path)?;
        let mut header = [0; 12];
        file.read_exact(&mut header)?;
        if &header[..4] != b"RIFF" || &header[8..] != b"WAVE" {
            return Err("recovery requires a RIFF WAV".into());
        }
        let file_size = file.metadata()?.len();
        let mut format = false;
        loop {
            let mut chunk = [0; 8];
            file.read_exact(&mut chunk)?;
            let length = u64::from(u32::from_le_bytes(chunk[4..].try_into()?));
            let start = file.stream_position()?;
            if start + length > file_size {
                return Err("truncated WAV chunk; repair capture before recognition".into());
            }
            match &chunk[..4] {
                b"fmt " => {
                    if length < 16 {
                        return Err("short WAV format".into());
                    }
                    let mut fmt = [0; 16];
                    file.read_exact(&mut fmt)?;
                    if u16::from_le_bytes(fmt[..2].try_into()?) != 1
                        || u16::from_le_bytes(fmt[2..4].try_into()?) != 1
                        || u32::from_le_bytes(fmt[4..8].try_into()?) != 16000
                        || u16::from_le_bytes(fmt[12..14].try_into()?) != 2
                        || u16::from_le_bytes(fmt[14..16].try_into()?) != 16
                    {
                        return Err("recovery requires 16 kHz mono PCM i16".into());
                    }
                    format = true;
                }
                b"data" => {
                    if !format || length % 2 != 0 {
                        return Err("invalid WAV data".into());
                    }
                    return Ok(Self {
                        file,
                        remaining: length,
                    });
                }
                _ => {}
            }
            file.seek(SeekFrom::Start(start + length + length % 2))?;
        }
    }

    pub fn read(&mut self) -> Result<Option<Vec<i16>>> {
        if self.remaining == 0 {
            return Ok(None);
        }
        let count = self.remaining.min(3200) as usize;
        let mut bytes = vec![0; count];
        self.file.read_exact(&mut bytes)?;
        self.remaining -= count as u64;
        Ok(Some(
            bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| i16::from_le_bytes(*pair))
                .collect(),
        ))
    }
}
