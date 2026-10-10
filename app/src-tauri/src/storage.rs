use fs2::FileExt;
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};
use voice_core::Result;

#[derive(Clone)]
pub struct Paths {
    pub data: PathBuf,
    pub config: PathBuf,
    pub models: PathBuf,
}
impl Paths {
    pub fn discover() -> Result<Self> {
        let home = PathBuf::from(std::env::var_os("HOME").ok_or("HOME is required")?);
        let key = env!("PRODUCT_STORAGE_KEY");
        #[cfg(target_os = "linux")]
        let (data, config) = (
            std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".local/share"))
                .join(key),
            std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".config"))
                .join(key),
        );
        #[cfg(target_os = "macos")]
        let (data, config) = {
            let data = home.join("Library/Application Support").join(key);
            (data.clone(), data)
        };
        Ok(Self::new(data, config))
    }
    pub fn new(data: PathBuf, config: PathBuf) -> Self {
        Self {
            models: data.join("models"),
            data,
            config,
        }
    }
    pub fn lock(&self) -> Result<File> {
        private_dir(&self.data)?;
        private_dir(&self.config)?;
        let file = private_file(&self.data.join("app.lock"))?;
        file.try_lock_exclusive()
            .map_err(|_| "another voice process owns this data directory")?;
        Ok(file)
    }
}
pub fn private_dir(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir_all(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}
pub fn private_file(path: &Path) -> Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    Ok(OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)?)
}
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or("file needs a parent directory")?;
    private_dir(parent)?;
    let temporary = parent.join(format!(".{}.tmp", voice_core::UtteranceId::random().0));
    let result = (|| {
        let mut file = private_file(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}
pub fn disk_bytes(path: &Path) -> Result<u64> {
    let mut size = 0;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            size += disk_bytes(&entry.path())?;
        } else {
            size += metadata.len();
        }
    }
    Ok(size)
}
