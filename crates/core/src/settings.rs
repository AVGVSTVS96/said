use std::{collections::BTreeMap, fs, path::Path};

use crate::{AppSettings, Result, Settings};

impl Settings {
    pub fn saved_hint(&self) -> String {
        let key = self
            .insert_last_hotkey
            .split('+')
            .map(|part| match part.to_ascii_lowercase().as_str() {
                "alt" if cfg!(target_os = "macos") => "Option".into(),
                "ctrl" => "Ctrl".into(),
                "alt" => "Alt".into(),
                "super" | "meta" => "Super".into(),
                "cmd" => "Command".into(),
                "shift" => "Shift".into(),
                "space" => "Space".into(),
                _ => part.to_ascii_uppercase(),
            })
            .collect::<Vec<_>>()
            .join("+");
        format!("Saved. Press {key} to insert")
    }

    pub fn defaults(engine: impl Into<String>) -> Self {
        let apps = [
            "com.mitchellh.ghostty",
            "kitty",
            "Alacritty",
            "org.wezfurlong.wezterm",
            "org.gnome.Terminal",
            "foot",
            "footclient",
            "org.gnome.Console",
            "konsole",
            "org.kde.konsole",
            "wezterm",
            "rio",
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
            hotkey: if cfg!(target_os = "macos") {
                "alt+space"
            } else {
                "super+z"
            }
            .into(),
            interactive_hotkey: if cfg!(target_os = "macos") {
                "alt+x"
            } else {
                "super+x"
            }
            .into(),
            insert_last_hotkey: if cfg!(target_os = "macos") {
                "alt+a"
            } else {
                "super+a"
            }
            .into(),
            engine: engine.into(),
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
        struct FileSettings {
            hotkey: Option<String>,
            interactive_hotkey: Option<String>,
            insert_last_hotkey: Option<String>,
            engine: Option<String>,
            microphone: Option<String>,
            apps: Option<BTreeMap<String, AppSettings>>,
        }
        let file: FileSettings = toml::from_str(&content)?;
        let mut settings = Self::defaults(default_engine);
        if let Some(value) = file.hotkey {
            settings.hotkey = value;
        }
        if let Some(value) = file.interactive_hotkey {
            settings.interactive_hotkey = value;
        }
        if let Some(value) = file.insert_last_hotkey {
            settings.insert_last_hotkey = value;
        }
        if let Some(value) = file.engine {
            settings.engine = value;
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
            || self.interactive_hotkey.is_empty()
            || self.insert_last_hotkey.is_empty()
            || self.hotkey.eq_ignore_ascii_case(&self.interactive_hotkey)
            || self.hotkey.eq_ignore_ascii_case(&self.insert_last_hotkey)
            || self
                .interactive_hotkey
                .eq_ignore_ascii_case(&self.insert_last_hotkey)
            || self.engine.is_empty()
            || self.apps.values().any(|app| app.paste.is_empty())
        {
            return Err(
                "shortcuts must be distinct; shortcuts, engine and paste chords must not be empty"
                    .into(),
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_insert_key_drives_the_saved_hint() {
        let mut settings = Settings::defaults("local-model");
        settings.insert_last_hotkey = "super+g".into();
        assert_eq!(settings.saved_hint(), "Saved. Press Super+G to insert");
        settings.insert_last_hotkey = "alt+x".into();
        assert_eq!(
            settings.saved_hint(),
            if cfg!(target_os = "macos") {
                "Saved. Press Option+X to insert"
            } else {
                "Saved. Press Alt+X to insert"
            }
        );
    }
}
