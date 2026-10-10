use std::fs::{self, File, OpenOptions};
use std::os::unix::fs::{FileExt, OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use voice_core::{Result, UtteranceId};

pub(crate) struct Spool {
    file: File,
    path: PathBuf,
    written: AtomicU64,
}

impl Spool {
    pub fn new(id: UtteranceId) -> Result<Self> {
        let directory = crate::manifest::model_cache()?.join("audio-spool");
        fs::create_dir_all(&directory)?;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
        let path = directory.join(format!("{}.pcm", id.0));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        Ok(Self {
            file,
            path,
            written: AtomicU64::new(0),
        })
    }

    pub fn append(&self, samples: &[i16]) -> Result<()> {
        let offset = self.written.load(Ordering::Acquire);
        let bytes: Vec<_> = samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect();
        self.file.write_all_at(&bytes, offset)?;
        self.written
            .store(offset + bytes.len() as u64, Ordering::Release);
        Ok(())
    }

    pub fn read(&self, offset: &mut u64) -> Result<Option<Vec<i16>>> {
        let count = (self.written.load(Ordering::Acquire) - *offset).min(3200) as usize;
        if count == 0 {
            return Ok(None);
        }
        let mut bytes = vec![0; count];
        self.file.read_exact_at(&mut bytes, *offset)?;
        *offset += count as u64;
        Ok(Some(
            bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| i16::from_le_bytes(*b))
                .collect(),
        ))
    }

    pub fn consumed(&self, offset: u64) -> bool {
        offset == self.written.load(Ordering::Acquire)
    }
}

impl Drop for Spool {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
