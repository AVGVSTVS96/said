use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::Path,
};

use crate::SAMPLE_RATE;

const HEADER_LEN: u64 = 80;

pub struct WavWriter {
    file: File,
    samples: u64,
}

impl WavWriter {
    pub fn create(path: &Path) -> io::Result<Self> {
        let mut options = OpenOptions::new();
        options.write(true).read(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path)?;
        file.write_all(&header(0))?;
        file.sync_all()?;
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            File::open(parent)?.sync_all()?;
        }
        Ok(Self { file, samples: 0 })
    }

    pub fn append(&mut self, pcm: &[i16]) -> io::Result<()> {
        let mut bytes = [0; 2048];
        for chunk in pcm.chunks(bytes.len() / 2) {
            for (sample, dest) in chunk.iter().zip(bytes.as_chunks_mut::<2>().0) {
                dest.copy_from_slice(&sample.to_le_bytes());
            }
            self.file.write_all(&bytes[..chunk.len() * 2])?;
            self.samples += chunk.len() as u64;
        }
        Ok(())
    }

    pub fn samples(&self) -> u64 {
        self.samples
    }

    pub fn checkpoint(&mut self) -> io::Result<()> {
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(&header(self.samples))?;
        self.file.seek(SeekFrom::End(0))?;
        self.file.sync_data()
    }

    pub fn finish(mut self) -> io::Result<u64> {
        self.checkpoint()?;
        Ok(self.samples)
    }
}

pub fn recover_wav(path: &Path) -> io::Result<u64> {
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    validate(&mut file)?;
    let samples = (file.metadata()?.len() - HEADER_LEN) / 2;
    file.set_len(HEADER_LEN + samples * 2)?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(&header(samples))?;
    file.sync_data()?;
    Ok(samples)
}

pub fn create_audio_dir(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        File::open(parent)?.sync_all()?;
    }
    Ok(())
}

pub struct WavReader {
    file: File,
    remaining: u64,
}

impl WavReader {
    pub fn open(path: &Path) -> io::Result<Self> {
        let mut file = File::open(path)?;
        let samples = validate(&mut file)?;
        let available = (file.metadata()?.len() - HEADER_LEN) / 2;
        if samples > available {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "truncated WAV data",
            ));
        }
        Ok(Self {
            file,
            remaining: samples,
        })
    }

    pub fn read(&mut self, pcm: &mut [i16]) -> io::Result<usize> {
        let take = pcm.len().min(self.remaining as usize);
        let mut bytes = [0; 2048];
        for chunk in pcm[..take].chunks_mut(bytes.len() / 2) {
            self.file.read_exact(&mut bytes[..chunk.len() * 2])?;
            for (dest, sample) in chunk.iter_mut().zip(bytes.as_chunks::<2>().0) {
                *dest = i16::from_le_bytes([sample[0], sample[1]]);
            }
        }
        self.remaining -= take as u64;
        Ok(take)
    }
}

fn validate(file: &mut File) -> io::Result<u64> {
    let mut bytes = [0; HEADER_LEN as usize];
    file.read_exact(&mut bytes)?;
    let reference = header(0);
    if &bytes[8..12] != b"WAVE"
        || bytes[48..76] != reference[48..76]
        || !matches!(&bytes[..4], b"RIFF" | b"RF64")
        || !matches!(&bytes[12..16], b"JUNK" | b"ds64")
        || bytes[16..20] != 28u32.to_le_bytes()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "not a voice capture WAV",
        ));
    }
    if &bytes[..4] == b"RF64" {
        Ok(u64::from_le_bytes(bytes[28..36].try_into().unwrap()) / 2)
    } else {
        Ok(u32::from_le_bytes(bytes[76..80].try_into().unwrap()) as u64 / 2)
    }
}

fn header(samples: u64) -> [u8; HEADER_LEN as usize] {
    let mut bytes = [0; HEADER_LEN as usize];
    let data_len = samples * 2;
    let riff_len = data_len + HEADER_LEN - 8;
    let rf64 = riff_len > u32::MAX as u64;
    bytes[..4].copy_from_slice(if rf64 { b"RF64" } else { b"RIFF" });
    bytes[4..8].copy_from_slice(&(if rf64 { u32::MAX } else { riff_len as u32 }).to_le_bytes());
    bytes[8..12].copy_from_slice(b"WAVE");
    bytes[12..16].copy_from_slice(if rf64 { b"ds64" } else { b"JUNK" });
    bytes[16..20].copy_from_slice(&28u32.to_le_bytes());
    if rf64 {
        bytes[20..28].copy_from_slice(&riff_len.to_le_bytes());
        bytes[28..36].copy_from_slice(&data_len.to_le_bytes());
        bytes[36..44].copy_from_slice(&samples.to_le_bytes());
    }
    bytes[48..52].copy_from_slice(b"fmt ");
    bytes[52..56].copy_from_slice(&16u32.to_le_bytes());
    bytes[56..58].copy_from_slice(&1u16.to_le_bytes());
    bytes[58..60].copy_from_slice(&1u16.to_le_bytes());
    bytes[60..64].copy_from_slice(&SAMPLE_RATE.to_le_bytes());
    bytes[64..68].copy_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    bytes[68..70].copy_from_slice(&2u16.to_le_bytes());
    bytes[70..72].copy_from_slice(&16u16.to_le_bytes());
    bytes[72..76].copy_from_slice(b"data");
    bytes[76..80].copy_from_slice(&(if rf64 { u32::MAX } else { data_len as u32 }).to_le_bytes());
    bytes
}
