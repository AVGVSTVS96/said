use crate::storage::{private_dir, private_file};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::Path,
};
use voice_core::{DownloadState, Result};
use voice_engine::manifest::Manifest;

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
    let selected = manifest.runtime(model)?;
    let key = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    let artifact = selected
        .artifacts
        .get(&key)
        .ok_or("no native runtime for this architecture")?;
    let archive = directory.join(format!("runtime-{}.archive", selected.version));
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
    if matches!(artifact.format.as_str(), "tar" | "tar.bz2") {
        let input = File::open(&archive)?;
        let decoder: Box<dyn Read> = if artifact.format == "tar.bz2" {
            Box::new(bzip2::read::BzDecoder::new(input))
        } else {
            Box::new(flate2::read::GzDecoder::new(input))
        };
        let mut archive = tar::Archive::new(decoder);
        for entry in archive.entries()? {
            let mut entry = entry?;
            let path = entry.path()?.into_owned();
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if !entry.header().entry_type().is_file()
                || !(name.ends_with(".so") || name.contains(".so.") || name.ends_with(".dylib"))
            {
                continue;
            }
            install_library(&mut entry, &runtime.join(name))?;
        }
    } else if artifact.format == "wheel" {
        let mut archive = zip::ZipArchive::new(File::open(&archive)?)?;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index)?;
            let path = Path::new(entry.name());
            if !entry.name().starts_with("moonshine_voice/")
                || path.extension().and_then(|s| s.to_str()) != Some("dylib")
            {
                continue;
            }
            let destination = runtime.join(path.file_name().ok_or("missing library filename")?);
            install_library(&mut entry, &destination)?;
        }
    } else {
        return Err("unsupported runtime archive".into());
    }
    if !directory.join(library).is_file() {
        return Err("runtime archive omitted its library".into());
    }
    Ok(())
}

fn install_library(reader: &mut impl Read, path: &Path) -> Result<()> {
    let temporary = path.with_extension(format!("{}.part", voice_core::UtteranceId::random().0));
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
