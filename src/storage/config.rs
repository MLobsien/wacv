use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Application configuration stored on disk as JSON.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Config {
    /// The user's display name, used to identify their own messages in group chats.
    /// When `None`, the app falls back to the 1:1 heuristic in `Chat::my_name()`.
    pub user_name: Option<String>,
    /// Whether the UI uses the dark theme.
    pub dark_mode: bool,
}

impl Config {
    /// Path to the config file.
    /// On Android, uses `getFilesDir() / "wacv/config.json"`.
    /// On desktop, uses `$XDG_CONFIG_HOME/wacv/config.json`.
    pub fn path() -> PathBuf {
        #[cfg(target_os = "android")]
        if let Some(dir) = crate::android::android_data_dir() {
            return dir.join("wacv").join("config.json");
        }
        dirs::config_dir()
.unwrap_or_else(|| PathBuf::from("."))
.join("wacv")
.join("config.json")
    }

    /// Load config from disk, returning defaults on error.
    pub fn load() -> Self {
        let path = Self::path();
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|content| serde_json::from_str(&content).ok())
            .unwrap_or_default()
    }

    /// Save config to disk, creating parent directories if needed.
    pub fn save(&self) {
        let path = Self::path();
        eprintln!("[WACV] Config::save() to {path:?}");
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(content) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(&path, content);
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self { user_name: None, dark_mode: false }
    }
}
