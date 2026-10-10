use std::{collections::BTreeMap, fs, path::Path};

use crate::{AppSettings, Result, Settings};

impl Settings {
    pub fn defaults(engine: impl Into<String>) -> Self {
        let apps = [
            "com.mitchellh.ghostty",
            "kitty",
            "Alacritty",
            "org.wezfurlong.wezterm",
            "org.gnome.Terminal",
            "foot",
        ]
        .into_iter()
        .map(|id| {
            (
                id.to_owned(),
                AppSettings {
                    paste: if cfg!(target_os = "macos") {
                        "cmd+v"
                    } else {
                        "ctrl+shift+v"
                    }
                    .into(),
                    send: false,
                },
            )
        })
        .collect();
        Self {
            hotkey: "ctrl+alt+space".into(),
            engine: engine.into(),
            vocabulary: false,
            microphone: None,
            apps,
        }
    }

    pub fn load(path: &Path, default_engine: &str) -> Result<Self> {
        let content = match fs::read_to_string(path) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::defaults(default_engine));
            }
            Err(error) => return Err(error.into()),
        };
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct FileSettings {
            hotkey: Option<String>,
            engine: Option<String>,
            vocabulary: Option<bool>,
            microphone: Option<String>,
            apps: Option<BTreeMap<String, AppSettings>>,
        }
        let file: FileSettings = toml::from_str(&content)?;
        let mut settings = Self::defaults(default_engine);
        if let Some(value) = file.hotkey {
            settings.hotkey = value;
        }
        if let Some(value) = file.engine {
            settings.engine = value;
        }
        if let Some(value) = file.vocabulary {
            settings.vocabulary = value;
        }
        settings.microphone = file.microphone;
        if let Some(value) = file.apps {
            settings.apps = value;
        }
        settings.validate()?;
        Ok(settings)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        crate::storage::atomic_write(path, toml::to_string(self)?.as_bytes())
    }

    fn validate(&self) -> Result<()> {
        if self.hotkey.is_empty()
            || self.engine.is_empty()
            || self.apps.values().any(|app| app.paste.is_empty())
        {
            return Err("hotkey, engine and paste chords must not be empty".into());
        }
        Ok(())
    }
}
