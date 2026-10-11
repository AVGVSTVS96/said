use crate::storage::{private_dir, private_file};
use said_core::{DownloadState, Result};
use said_engine::manifest::Manifest;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};

fn hash(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut bytes = [0; 65536];
    loop {
        let count = file.read(&mut bytes)?;
        if count == 0 {
            break;
        }
        digest.update(&bytes[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}
fn fetch(
    client: &reqwest::blocking::Client,
    url: &str,
    path: &Path,
    expected: &str,
    progress: &mut impl FnMut(DownloadState),
) -> Result<()> {
    if path.is_file() && hash(path)? == expected {
        return Ok(());
    }
    private_dir(path.parent().ok_or("download needs a directory")?)?;
    let temporary = path.with_extension("part");
    let result = (|| {
        let mut response = client.get(url).send()?.error_for_status()?;
        let total = response.content_length();
        let mut received = 0;
        let mut file = private_file(&temporary)?;
        file.set_len(0)?;
        let mut buffer = [0; 65536];
        loop {
            let count = response.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            file.write_all(&buffer[..count])?;
            received += count as u64;
            progress(DownloadState {
                state: "downloading".into(),
                file: path.file_name().unwrap().to_string_lossy().into(),
                received,
                total,
                error: None,
            });
        }
        file.sync_all()?;
        if hash(&temporary)? != expected {
            return Err(format!("SHA-256 mismatch for {url}").into());
        }
        fs::rename(&temporary, path)?;
        File::open(path.parent().unwrap())?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}
pub fn install(
    manifest: &Manifest,
    model: &str,
    directory: &Path,
    mut progress: impl FnMut(DownloadState),
) -> Result<()> {
    private_dir(directory)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()?;
    let model = manifest.models.get(model).ok_or("unknown model")?;
    for (name, expected) in &model.files {
        fetch(
            &client,
            &format!("{}{name}", model.source),
            &directory.join(&model.directory).join(name),
            expected,
            &mut progress,
        )?;
    }
    let selected = &manifest.runtime;
    let key = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    let artifact = selected
        .artifacts
        .get(&key)
        .ok_or("no native runtime for this architecture")?;
    let archive = directory.join(format!("runtime-{}.archive", selected.version));
    if let Some(packaged) = packaged_runtime(&std::env::current_exe()?)
        && packaged.is_file()
        && (!archive.is_file() || hash(&archive)? != artifact.sha256)
    {
        stage_runtime(&packaged, &archive, &artifact.sha256)?;
    }
    fetch(
        &client,
        &artifact.url,
        &archive,
        &artifact.sha256,
        &mut progress,
    )?;
    let library = selected
        .libraries
        .get(std::env::consts::OS)
        .ok_or("no native runtime for this platform")?;
    let runtime = directory
        .join(library)
        .parent()
        .ok_or("runtime directory missing")?
        .to_owned();
    private_dir(&runtime)?;
    if artifact.format != "tar.bz2" {
        return Err("unsupported runtime archive".into());
    }
    let mut archive = tar::Archive::new(bzip2::read::BzDecoder::new(File::open(&archive)?));
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if entry.header().entry_type().is_file()
            && (name.ends_with(".so") || name.contains(".so.") || name.ends_with(".dylib"))
        {
            install_library(&mut entry, &runtime.join(name))?;
        }
    }
    if !directory.join(library).is_file() {
        return Err("runtime archive omitted its library".into());
    }
    Ok(())
}

fn packaged_runtime(executable: &Path) -> Option<PathBuf> {
    let prefix = executable.parent()?.parent()?;
    #[cfg(target_os = "linux")]
    let archive = prefix.join("lib/said/runtime.archive");
    #[cfg(target_os = "macos")]
    let archive = prefix.join("Resources/runtime.archive");
    Some(archive)
}

fn stage_runtime(source: &Path, destination: &Path, expected: &str) -> Result<()> {
    let temporary = destination.with_extension("part");
    let result = (|| {
        let mut file = private_file(&temporary)?;
        file.set_len(0)?;
        std::io::copy(&mut File::open(source)?, &mut file)?;
        file.sync_all()?;
        if hash(&temporary)? != expected {
            return Err("Packaged runtime failed SHA-256 verification".into());
        }
        fs::rename(&temporary, destination)?;
        File::open(destination.parent().ok_or("runtime needs a directory")?)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packaged_runtime_is_verified_and_staged_atomically_without_network() {
        let directory =
            std::env::temp_dir().join(format!("said-runtime-stage-{}", std::process::id()));
        private_dir(&directory).unwrap();
        let source = directory.join("runtime.archive");
        let destination = directory.join("cached.archive");
        fs::write(&source, b"synthetic pinned runtime").unwrap();
        stage_runtime(&source, &destination, &hash(&source).unwrap()).unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"synthetic pinned runtime");
        fs::write(&source, b"corrupt archive").unwrap();
        assert!(stage_runtime(&source, &destination, "incorrect hash").is_err());
        assert_eq!(fs::read(&destination).unwrap(), b"synthetic pinned runtime");
        assert!(!destination.with_extension("part").exists());
        fs::remove_dir_all(directory).unwrap();
    }
}

fn install_library(reader: &mut impl Read, path: &Path) -> Result<()> {
    let temporary = path.with_extension(format!("{}.part", said_core::UtteranceId::random().0));
    let result = (|| {
        let mut file = private_file(&temporary)?;
        std::io::copy(reader, &mut file)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        File::open(path.parent().unwrap())?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}
