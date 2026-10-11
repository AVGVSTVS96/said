use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use said_core::{Result, UtteranceId};
use serde::Deserialize;

use crate::{
    fake::{FakeEngine, Trace},
    parakeet::{Parakeet, ParakeetModel},
    worker::Adapter,
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub defaults: BTreeMap<String, String>,
    pub runtime: Runtime,
    pub models: BTreeMap<String, Model>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Runtime {
    pub version: String,
    pub libraries: BTreeMap<String, String>,
    pub artifacts: BTreeMap<String, Artifact>,
    pub source: Source,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub url: String,
    pub sha256: String,
    pub revision: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub url: String,
    pub sha256: String,
    pub format: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    pub adapter: String,
    pub update_samples: usize,
    pub threads: i32,
    pub provider: String,
    pub directory: String,
    pub license: String,
    pub source: String,
    pub files: BTreeMap<String, String>,
}

impl Model {
    fn transducer_files(&self) -> Result<[&str; 4]> {
        let mut files = Vec::new();
        for stem in ["encoder", "decoder", "joiner"] {
            let mut matches = self
                .files
                .keys()
                .filter(|name| name.starts_with(&format!("{stem}.")) && name.ends_with(".onnx"));
            let name = matches.next().ok_or("missing transducer ONNX file")?;
            if matches.next().is_some() {
                return Err("ambiguous transducer ONNX files".into());
            }
            files.push(name.as_str());
        }
        if !self.files.contains_key("tokens.txt") {
            return Err("missing transducer tokens".into());
        }
        Ok([files[0], files[1], files[2], "tokens.txt"])
    }
}

impl Manifest {
    pub fn bundled() -> Result<Self> {
        Ok(serde_json::from_str(include_str!("../models.json"))?)
    }

    pub fn read(path: &Path) -> Result<Self> {
        Ok(serde_json::from_reader(std::fs::File::open(path)?)?)
    }

    pub fn default_model(&self) -> Result<&str> {
        self.defaults
            .get(std::env::consts::OS)
            .map(String::as_str)
            .ok_or_else(|| "no engine default for this platform".into())
    }

    pub fn create(&self, id: UtteranceId, model: &str, cache: &Path) -> Result<Adapter> {
        let config = self.models.get(model).ok_or("unknown model")?;
        match config.adapter.as_str() {
            "parakeet" => {
                let library = self
                    .runtime
                    .libraries
                    .get(std::env::consts::OS)
                    .ok_or("no Parakeet runtime for this platform")?;
                let loaded = ParakeetModel::load(
                    &cache.join(library),
                    &cache.join(&config.directory),
                    config.transducer_files()?,
                    config.threads,
                    &config.provider,
                    config.update_samples,
                )?;
                let (engine, output) = Parakeet::from_model(id, loaded);
                Ok((Box::new(engine), output))
            }
            "fake" => {
                let (engine, output) = FakeEngine::new(
                    id,
                    Trace {
                        finish: vec![said_core::EngineEvent::Final],
                        ..Trace::default()
                    },
                );
                Ok((Box::new(engine), output))
            }
            _ => Err("unknown adapter".into()),
        }
    }
}

pub fn model_cache() -> Result<PathBuf> {
    Ok(
        PathBuf::from(std::env::var_os("HOME").ok_or("HOME is not set")?)
            .join(".cache/said-models"),
    )
}

pub fn worker_main(model: Option<&str>, manifest: &Manifest, cache: &Path) -> Result<()> {
    let model = model.map(Ok).unwrap_or_else(|| manifest.default_model())?;
    let config = manifest.models.get(model).ok_or("unknown model")?;
    if config.adapter == "parakeet" {
        let library = manifest
            .runtime
            .libraries
            .get(std::env::consts::OS)
            .ok_or("no Parakeet runtime for this platform")?;
        let loaded = ParakeetModel::load(
            &cache.join(library),
            &cache.join(&config.directory),
            config.transducer_files()?,
            config.threads,
            &config.provider,
            config.update_samples,
        )?;
        crate::worker::run(std::io::stdin().lock(), std::io::stdout().lock(), |id| {
            let (engine, output) = Parakeet::from_model(id, loaded.clone());
            Ok((Box::new(engine), output))
        })
    } else {
        crate::worker::run(std::io::stdin().lock(), std::io::stdout().lock(), |id| {
            manifest.create(id, model, cache)
        })
    }
}
