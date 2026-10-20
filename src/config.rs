use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

/// ~/.config/vfdtune (or $XDG_CONFIG_HOME/vfdtune)
pub fn dir() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("vfdtune")
}

/// ~/.cache/vfdtune (or $XDG_CACHE_HOME/vfdtune)
pub fn cache_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("vfdtune")
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    /// Client ID of YOUR Spotify app (developer.spotify.com/dashboard). Required; see README.
    pub client_id: Option<String>,
    /// Force an audio backend by name ("pulseaudio", "alsa", "rodio"). `null` = first compiled in.
    pub audio_backend: Option<String>,
    /// 96, 160 or 320
    pub bitrate: u16,
    /// Results per search request (Spotify caps this at 10 for dev-mode apps)
    pub search_limit: u32,
    pub volume: u8,
    pub theme: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            client_id: None,
            audio_backend: None,
            bitrate: 320,
            search_limit: 10,
            volume: 60,
            theme: 0,
        }
    }
}

impl Config {
    fn path() -> PathBuf {
        dir().join("config.json")
    }

    pub fn load() -> Self {
        fs::read_to_string(Self::path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let _ = fs::create_dir_all(dir());
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = fs::write(Self::path(), s);
        }
    }
}
