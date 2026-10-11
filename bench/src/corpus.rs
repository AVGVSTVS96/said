use std::collections::BTreeSet;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

use said_core::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::score::{Term, normalized_tokens};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Clip {
    pub id: String,
    pub audio: PathBuf,
    pub sha256: String,
    pub reference: String,
    pub tags: Vec<String>,
    pub license: String,
    pub source: String,
    pub reference_status: String,
    #[serde(default)]
    pub terms: Vec<Term>,
    #[serde(default)]
    pub ci: bool,
    #[serde(default)]
    pub pair: Option<String>,
    #[serde(default)]
    pub polarity: Option<String>,
}

impl Clip {
    pub fn non_speech(&self) -> bool {
        self.tags
            .iter()
            .any(|tag| matches!(tag.as_str(), "silence" | "noise"))
    }

    pub fn path(&self, manifest: &Path, audio_root: Option<&Path>) -> PathBuf {
        if self.audio.is_absolute() {
            return self.audio.clone();
        }
        audio_root
            .unwrap_or_else(|| manifest.parent().unwrap_or(Path::new(".")))
            .join(&self.audio)
    }
}

pub fn read(path: &Path, ci: bool) -> Result<Vec<Clip>> {
    let mut clips = Vec::new();
    let mut ids = BTreeSet::new();
    let mut pairs = std::collections::BTreeMap::<String, Vec<String>>::new();
    for (line, json) in BufReader::new(File::open(path)?).lines().enumerate() {
        let json = json?;
        if json.trim().is_empty() {
            continue;
        }
        let clip: Clip = serde_json::from_str(&json)
            .map_err(|error| format!("manifest line {}: {error}", line + 1))?;
        if clip.id.is_empty() || !ids.insert(clip.id.clone()) {
            return Err(format!("manifest line {}: empty or duplicate id", line + 1).into());
        }
        if clip.sha256.len() != 64 || !clip.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!("{}: expected SHA-256", clip.id).into());
        }
        if clip.license.is_empty() || clip.source.is_empty() || clip.reference_status.is_empty() {
            return Err(format!(
                "{}: license, source and reference_status are required",
                clip.id
            )
            .into());
        }
        if clip.non_speech() && !clip.reference.is_empty() {
            return Err(format!("{}: non-speech reference must be empty", clip.id).into());
        }
        let mut terms = BTreeSet::new();
        for term in &clip.terms {
            if !matches!(
                term.kind.as_str(),
                "technical" | "identifier" | "path" | "flag"
            ) || term.text.is_empty()
                || !terms.insert((term.kind.clone(), term.text.clone()))
            {
                return Err(format!("{}: invalid or duplicate term", clip.id).into());
            }
            let found = if term.kind == "technical" {
                let text = normalized_tokens(&clip.reference);
                let words = normalized_tokens(&term.text);
                !words.is_empty() && text.windows(words.len()).any(|w| w == words)
            } else {
                let text: Vec<_> = clip.reference.split_whitespace().collect();
                let words: Vec<_> = term.text.split_whitespace().collect();
                !words.is_empty() && text.windows(words.len()).any(|w| w == words)
            };
            if !found {
                return Err(
                    format!("{}: term absent from reference: {}", clip.id, term.text).into(),
                );
            }
        }
        match (&clip.pair, &clip.polarity) {
            (Some(pair), Some(polarity))
                if matches!(polarity.as_str(), "negative" | "affirmative") =>
            {
                pairs
                    .entry(pair.clone())
                    .or_default()
                    .push(polarity.clone());
            }
            (None, None) => {}
            _ => {
                return Err(format!(
                    "{}: pair requires negative or affirmative polarity",
                    clip.id
                )
                .into());
            }
        }
        if !ci || clip.ci {
            clips.push(clip);
        }
    }
    for (pair, mut polarities) in pairs {
        polarities.sort();
        if polarities != ["affirmative", "negative"] {
            return Err(
                format!("pair {pair} requires one negative and one affirmative twin").into(),
            );
        }
    }
    if clips.is_empty() {
        return Err("manifest contains no selected clips".into());
    }
    Ok(clips)
}

pub fn hash(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub fn verify(clips: &[Clip], manifest: &Path, audio_root: Option<&Path>) -> Result<()> {
    for clip in clips {
        let path = clip.path(manifest, audio_root);
        if hash(&path)? != clip.sha256.to_lowercase() {
            return Err(format!("{}: audio hash mismatch", clip.id).into());
        }
        let _ = said_engine::wav::Wav::open(&path)?;
    }
    Ok(())
}
