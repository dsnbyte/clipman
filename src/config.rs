//! Paths and persistent settings.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"))
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(fallback))
}

pub fn config_dir() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config")
}

pub fn data_dir() -> PathBuf {
    xdg("XDG_DATA_HOME", ".local/share").join("clipman")
}

pub fn config_path() -> PathBuf {
    config_dir().join("clipman").join("config.toml")
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    /// Maximum number of unpinned items kept in history.
    pub max_items: usize,
    /// Text larger than this (in KiB) is not recorded.
    pub max_text_kb: usize,
    /// Images larger than this (in MiB) are not recorded.
    pub max_image_mb: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            max_items: 200,
            max_text_kb: 1024,
            max_image_mb: 16,
        }
    }
}

impl Config {
    pub fn load() -> Config {
        std::fs::read_to_string(config_path())
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        let path = config_path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, toml::to_string_pretty(self)?)?;
        Ok(())
    }
}
