//! App configuration: per-provider settings persisted as TOML.
//!
//! Mirrors CodexBar's provider toggle system: each provider can be explicitly
//! enabled or disabled; without an explicit setting the provider is enabled
//! when its credentials are detected on the machine.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::SpendPanelError;

/// Per-provider settings.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ProviderSettings {
    /// Explicit toggle. `None` means "auto": enabled when credentials are detected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Provider-specific key-value config (api_key, credentials_path, ...).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub config: HashMap<String, String>,
}

/// App configuration, persisted at `~/.config/usage-monitor/config.toml`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AppConfig {
    #[serde(default)]
    pub providers: HashMap<String, ProviderSettings>,
}

/// Resolved enablement state of a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderState {
    /// Explicitly enabled in the config file.
    Enabled,
    /// Explicitly disabled in the config file.
    Disabled,
    /// No explicit setting; enabled because credentials were detected.
    AutoEnabled,
    /// No explicit setting; disabled because no credentials were detected.
    AutoDisabled,
}

impl ProviderState {
    pub fn is_enabled(self) -> bool {
        matches!(self, Self::Enabled | Self::AutoEnabled)
    }
}

impl AppConfig {
    /// Default config path: `$XDG_CONFIG_HOME/usage-monitor/config.toml`
    /// or `~/.config/usage-monitor/config.toml`.
    pub fn default_path() -> Option<PathBuf> {
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
            if !xdg.is_empty() {
                return Some(PathBuf::from(xdg).join("usage-monitor/config.toml"));
            }
        }
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config/usage-monitor/config.toml"))
    }

    /// Loads the config from a path. A missing file yields the default config.
    pub fn load_from_path(path: &Path) -> Result<Self, SpendPanelError> {
        let raw = match std::fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => {
                return Err(SpendPanelError::ConfigError(format!(
                    "cannot read config at {}: {}",
                    path.display(),
                    e
                )));
            }
        };
        toml::from_str(&raw)
            .map_err(|e| SpendPanelError::ConfigError(format!("invalid config: {}", e)))
    }

    /// Loads the config from the default path (missing file → default config).
    pub fn load() -> Result<Self, SpendPanelError> {
        match Self::default_path() {
            Some(path) => Self::load_from_path(&path),
            None => Ok(Self::default()),
        }
    }

    /// Saves the config as TOML, creating parent directories if needed.
    pub fn save_to_path(&self, path: &Path) -> Result<(), SpendPanelError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| SpendPanelError::ConfigError(format!("create config dir: {}", e)))?;
        }
        let raw = toml::to_string_pretty(self)
            .map_err(|e| SpendPanelError::ConfigError(format!("serialize config: {}", e)))?;
        std::fs::write(path, raw)
            .map_err(|e| SpendPanelError::ConfigError(format!("write config: {}", e)))
    }

    /// Explicit toggle for a provider, if any.
    pub fn provider_enabled(&self, id: &str) -> Option<bool> {
        self.providers.get(id).and_then(|p| p.enabled)
    }

    /// Sets the explicit toggle for a provider.
    pub fn set_provider_enabled(&mut self, id: &str, enabled: bool) {
        self.providers.entry(id.to_string()).or_default().enabled = Some(enabled);
    }

    /// Clears the explicit toggle, returning the provider to auto-detection.
    pub fn clear_provider_enabled(&mut self, id: &str) {
        if let Some(settings) = self.providers.get_mut(id) {
            settings.enabled = None;
            if settings == &ProviderSettings::default() {
                self.providers.remove(id);
            }
        }
    }

    /// Resolves the state of a provider: explicit setting wins, otherwise
    /// falls back to credential detection.
    pub fn resolve_state(&self, id: &str, credentials_detected: bool) -> ProviderState {
        match self.provider_enabled(id) {
            Some(true) => ProviderState::Enabled,
            Some(false) => ProviderState::Disabled,
            None if credentials_detected => ProviderState::AutoEnabled,
            None => ProviderState::AutoDisabled,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_config_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "usage-monitor-config-{}-{}/config.toml",
            name,
            std::process::id()
        ))
    }

    #[test]
    fn test_missing_file_is_default() {
        let cfg = AppConfig::load_from_path(Path::new("/nonexistent/config.toml")).unwrap();
        assert!(cfg.providers.is_empty());
    }

    #[test]
    fn test_roundtrip_save_load() {
        let path = temp_config_path("roundtrip");
        let mut cfg = AppConfig::default();
        cfg.set_provider_enabled("claude", true);
        cfg.set_provider_enabled("openai", false);
        cfg.providers
            .entry("anthropic".into())
            .or_default()
            .config
            .insert("api_key".into(), "sk-ant-x".into());

        cfg.save_to_path(&path).unwrap();
        let loaded = AppConfig::load_from_path(&path).unwrap();
        std::fs::remove_dir_all(path.parent().unwrap()).ok();

        assert_eq!(loaded, cfg);
        assert_eq!(loaded.provider_enabled("claude"), Some(true));
        assert_eq!(loaded.provider_enabled("openai"), Some(false));
        assert_eq!(loaded.provider_enabled("codex"), None);
        assert_eq!(
            loaded.providers["anthropic"].config.get("api_key").unwrap(),
            "sk-ant-x"
        );
    }

    #[test]
    fn test_parse_toml() {
        let cfg: AppConfig = toml::from_str(
            r#"
            [providers.claude]
            enabled = true

            [providers.openai]
            enabled = false

            [providers.anthropic.config]
            api_key = "sk-ant-x"
            "#,
        )
        .unwrap();
        assert_eq!(cfg.provider_enabled("claude"), Some(true));
        assert_eq!(cfg.provider_enabled("openai"), Some(false));
        assert_eq!(cfg.provider_enabled("anthropic"), None);
    }

    #[test]
    fn test_resolve_state() {
        let mut cfg = AppConfig::default();
        cfg.set_provider_enabled("a", true);
        cfg.set_provider_enabled("b", false);

        assert_eq!(cfg.resolve_state("a", false), ProviderState::Enabled);
        assert_eq!(cfg.resolve_state("b", true), ProviderState::Disabled);
        assert_eq!(cfg.resolve_state("c", true), ProviderState::AutoEnabled);
        assert_eq!(cfg.resolve_state("c", false), ProviderState::AutoDisabled);

        assert!(cfg.resolve_state("a", false).is_enabled());
        assert!(!cfg.resolve_state("b", true).is_enabled());
        assert!(cfg.resolve_state("c", true).is_enabled());
        assert!(!cfg.resolve_state("c", false).is_enabled());
    }

    #[test]
    fn test_clear_provider_enabled() {
        let mut cfg = AppConfig::default();
        cfg.set_provider_enabled("claude", false);
        cfg.clear_provider_enabled("claude");
        assert_eq!(cfg.provider_enabled("claude"), None);
        // Entry without remaining settings is removed entirely.
        assert!(!cfg.providers.contains_key("claude"));
    }
}
