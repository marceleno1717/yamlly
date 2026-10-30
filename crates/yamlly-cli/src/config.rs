use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum EditMode {
    #[default]
    #[serde(alias = "basic")]
    Express,
    Advanced,
}

impl EditMode {
    pub fn toggle(self) -> Self {
        match self {
            Self::Express => Self::Advanced,
            Self::Advanced => Self::Express,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Express => "express",
            Self::Advanced => "advanced",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SshMode {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserConfig {
    #[serde(default)]
    pub mode: EditMode,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_namespace")]
    pub default_namespace: String,
    #[serde(default)]
    pub ssh_mode: SshMode,
}

fn default_theme() -> String {
    "Dark".to_string()
}

fn default_namespace() -> String {
    "default".to_string()
}

impl Default for UserConfig {
    fn default() -> Self {
        Self {
            mode: EditMode::Express,
            theme: default_theme(),
            default_namespace: default_namespace(),
            ssh_mode: SshMode::Auto,
        }
    }
}

impl UserConfig {
    /// Return the standard config directory `~/.config/yamlly`.
    pub fn config_dir() -> Option<PathBuf> {
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
            let p = PathBuf::from(xdg);
            if p.is_absolute() {
                return Some(p.join("yamlly"));
            }
        }
        if let Some(home) = std::env::var_os("HOME") {
            return Some(PathBuf::from(home).join(".config").join("yamlly"));
        }
        #[cfg(windows)]
        if let Some(appdata) = std::env::var_os("APPDATA") {
            return Some(PathBuf::from(appdata).join("yamlly"));
        }
        None
    }

    /// Return the full path to `~/.config/yamlly/config.toml`.
    pub fn config_path() -> Option<PathBuf> {
        Self::config_dir().map(|d| d.join("config.toml"))
    }

    /// Load user configuration from disk, creating the default file if it does not exist.
    /// If the file is corrupted or cannot be read, falls back safely to default settings.
    pub fn load_or_init() -> Self {
        let Some(path) = Self::config_path() else {
            return Self::default();
        };

        if !path.exists() {
            let default_cfg = Self::default();
            let _ = default_cfg.save_to_disk();
            return default_cfg;
        }

        match fs::read_to_string(&path) {
            Ok(content) => toml::from_str::<Self>(&content).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Save current configuration to `~/.config/yamlly/config.toml`.
    pub fn save_to_disk(&self) -> io::Result<()> {
        let Some(path) = Self::config_path() else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self).map_err(io::Error::other)?;
        fs::write(path, content)
    }

    /// Determine whether SSH optimizations should be active given the environment.
    pub fn is_ssh_active(&self) -> bool {
        match self.ssh_mode {
            SshMode::Always => true,
            SshMode::Never => false,
            SshMode::Auto => {
                std::env::var_os("SSH_CONNECTION").is_some()
                    || std::env::var_os("SSH_CLIENT").is_some()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_config_defaults() {
        let cfg = UserConfig::default();
        assert_eq!(cfg.mode, EditMode::Express);
        assert_eq!(cfg.theme, "Dark");
        assert_eq!(cfg.default_namespace, "default");
        assert_eq!(cfg.ssh_mode, SshMode::Auto);
    }

    #[test]
    fn test_edit_mode_toggle() {
        assert_eq!(EditMode::Express.toggle(), EditMode::Advanced);
        assert_eq!(EditMode::Advanced.toggle(), EditMode::Express);
    }

    #[test]
    fn test_edit_mode_backward_compat_deserialization() {
        let raw = "mode = \"basic\"\n";
        let parsed: UserConfig = toml::from_str(raw).expect("deserialize");
        assert_eq!(parsed.mode, EditMode::Express);
    }

    #[test]
    fn test_toml_roundtrip() {
        let cfg = UserConfig {
            mode: EditMode::Advanced,
            theme: "Nord".to_string(),
            default_namespace: "prod".to_string(),
            ssh_mode: SshMode::Always,
        };
        let s = toml::to_string_pretty(&cfg).expect("serialize");
        let parsed: UserConfig = toml::from_str(&s).expect("deserialize");
        assert_eq!(parsed, cfg);
    }

    #[test]
    fn test_toml_partial_missing_fields_use_defaults() {
        let raw = "mode = \"advanced\"\n";
        let parsed: UserConfig = toml::from_str(raw).expect("deserialize");
        assert_eq!(parsed.mode, EditMode::Advanced);
        assert_eq!(parsed.theme, "Dark");
        assert_eq!(parsed.default_namespace, "default");
        assert_eq!(parsed.ssh_mode, SshMode::Auto);
    }
}
