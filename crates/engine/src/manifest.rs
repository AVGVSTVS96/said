use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use voice_core::{Result, UtteranceId};

use crate::{
    fake::{FakeEngine, Trace},
    moonshine::{Moonshine, MoonshineModel},
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
    pub architecture: u32,
    pub directory: String,
    pub license: String,
    pub source: String,
    pub files: BTreeMap<String, String>,
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
            "moonshine" => {
                let library = self
                    .runtime
                    .libraries
                    .get(std::env::consts::OS)
                    .ok_or("no runtime library for this platform")?;
                let (engine, output) = Moonshine::new(
                    id,
                    &cache.join(library),
                    &cache.join(&config.directory),
                    config.architecture,
                )?;
                Ok((Box::new(engine), output))
            }
            "fake" => {
                let (engine, output) = FakeEngine::new(
                    id,
                    Trace {
                        finish: vec![voice_core::EngineEvent::Final],
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
            .join(".cache/voice-models"),
    )
}

pub fn worker_main(model: Option<&str>, manifest: &Manifest, cache: &Path) -> Result<()> {
    let model = model.map(Ok).unwrap_or_else(|| manifest.default_model())?;
    let config = manifest.models.get(model).ok_or("unknown model")?;
    if config.adapter == "moonshine" {
        let library = manifest
            .runtime
            .libraries
            .get(std::env::consts::OS)
            .ok_or("no runtime for platform")?;
        let loaded = MoonshineModel::load(
            &cache.join(library),
            &cache.join(&config.directory),
            config.architecture,
        )?;
        crate::worker::run(std::io::stdin().lock(), std::io::stdout().lock(), |id| {
            let (engine, output) = Moonshine::from_model(id, loaded.clone());
            Ok((Box::new(engine), output))
        })
    } else {
        crate::worker::run(std::io::stdin().lock(), std::io::stdout().lock(), |id| {
            manifest.create(id, model, cache)
        })
    }
}
