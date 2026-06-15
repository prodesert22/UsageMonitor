//! App configuration: per-provider settings persisted as TOML.
//!
//! Each provider can hold one or more named *accounts*. An account carries its
//! own credentials (token, api_key, credentials_path, …) plus an optional label
//! and enable toggle, so the same provider can be monitored for several logins.
//!
//! When a provider has no configured accounts it still works: the registry uses
//! a single implicit `default` account that relies on credential auto-detection
//! (e.g. `~/.claude/.credentials.json`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::SpendPanelError;

/// Name of the implicit/primary account used by the convenience commands.
pub const DEFAULT_ACCOUNT: &str = "default";

/// Per-account settings: credentials plus presentation metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AccountSettings {
    /// Explicit toggle. `None` means "auto": follows the provider/credential state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Human-friendly label shown in output (defaults to the account name).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Auth token/cookie for providers with manual authentication.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// Workspace IDs (providers that support multiple workspaces, e.g. opencode-go).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspaces: Vec<String>,
    /// Other provider-specific keys (api_key, credentials_path, ...), stored
    /// flat in the account's table.
    #[serde(default, flatten)]
    pub config: HashMap<String, String>,
}

impl AccountSettings {
    /// True when the account holds no settings at all (safe to drop).
    pub fn is_empty(&self) -> bool {
        self.enabled.is_none()
            && self.label.is_none()
            && self.token.is_none()
            && self.workspaces.is_empty()
            && self.config.is_empty()
    }
}

/// Per-provider settings: a provider-level toggle plus its named accounts.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ProviderSettings {
    /// Explicit provider-level toggle. `None` means "auto": enabled when
    /// credentials are detected or accounts are configured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Named accounts for this provider.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub accounts: HashMap<String, AccountSettings>,
}

impl ProviderSettings {
    /// True when the provider holds no settings at all (safe to drop).
    pub fn is_empty(&self) -> bool {
        self.enabled.is_none() && self.accounts.is_empty()
    }
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
    /// No explicit setting; enabled because credentials/accounts were detected.
    AutoEnabled,
    /// No explicit setting; disabled because nothing was detected.
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
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME")
            && !xdg.is_empty()
        {
            return Some(PathBuf::from(xdg).join("usage-monitor/config.toml"));
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

    // -----------------------------------------------------------------------
    // Provider-level toggle
    // -----------------------------------------------------------------------

    /// Explicit provider-level toggle, if any.
    pub fn provider_enabled(&self, id: &str) -> Option<bool> {
        self.providers.get(id).and_then(|p| p.enabled)
    }

    /// Sets the explicit provider-level toggle.
    pub fn set_provider_enabled(&mut self, id: &str, enabled: bool) {
        self.providers.entry(id.to_string()).or_default().enabled = Some(enabled);
    }

    /// Clears the explicit provider toggle, returning it to auto-detection.
    pub fn clear_provider_enabled(&mut self, id: &str) {
        if let Some(settings) = self.providers.get_mut(id) {
            settings.enabled = None;
            self.prune_provider(id);
        }
    }

    /// Resolves the state of a provider: explicit setting wins, otherwise falls
    /// back to credential detection or the presence of configured accounts.
    pub fn resolve_state(&self, id: &str, credentials_detected: bool) -> ProviderState {
        match self.provider_enabled(id) {
            Some(true) => ProviderState::Enabled,
            Some(false) => ProviderState::Disabled,
            None if credentials_detected || self.has_accounts(id) => ProviderState::AutoEnabled,
            None => ProviderState::AutoDisabled,
        }
    }

    // -----------------------------------------------------------------------
    // Accounts
    // -----------------------------------------------------------------------

    /// True when the provider has at least one configured account.
    pub fn has_accounts(&self, id: &str) -> bool {
        self.providers
            .get(id)
            .is_some_and(|p| !p.accounts.is_empty())
    }

    /// Sorted account names configured for a provider (empty when none).
    pub fn account_ids(&self, id: &str) -> Vec<String> {
        let mut ids: Vec<String> = self
            .providers
            .get(id)
            .map(|p| p.accounts.keys().cloned().collect())
            .unwrap_or_default();
        ids.sort();
        ids
    }

    /// An account's settings, if configured.
    pub fn account(&self, id: &str, account: &str) -> Option<&AccountSettings> {
        self.providers.get(id).and_then(|p| p.accounts.get(account))
    }

    /// Creates an account (no-op if it already exists), returning whether it was
    /// newly created.
    pub fn add_account(&mut self, id: &str, account: &str, label: Option<&str>) -> bool {
        let accounts = &mut self.providers.entry(id.to_string()).or_default().accounts;
        let created = !accounts.contains_key(account);
        let entry = accounts.entry(account.to_string()).or_default();
        if let Some(label) = label {
            entry.label = Some(label.to_string());
        }
        created
    }

    /// Removes an account entirely. Returns whether it existed.
    pub fn remove_account(&mut self, id: &str, account: &str) -> bool {
        let existed = self
            .providers
            .get_mut(id)
            .is_some_and(|p| p.accounts.remove(account).is_some());
        if existed {
            self.prune_provider(id);
        }
        existed
    }

    /// Sets the account label.
    pub fn set_account_label(&mut self, id: &str, account: &str, label: &str) {
        self.account_entry(id, account).label = Some(label.to_string());
    }

    /// Account label, if set.
    pub fn account_label(&self, id: &str, account: &str) -> Option<&str> {
        self.account(id, account).and_then(|a| a.label.as_deref())
    }

    /// Sets a key in an account's table. The `token` key maps to the typed
    /// field; everything else is stored flat.
    pub fn set_account_config(&mut self, id: &str, account: &str, key: &str, value: &str) {
        let entry = self.account_entry(id, account);
        if key == "token" {
            entry.token = Some(value.to_string());
        } else {
            entry.config.insert(key.to_string(), value.to_string());
        }
    }

    /// Removes a key from an account's table, cleaning up empty entries.
    pub fn unset_account_config(&mut self, id: &str, account: &str, key: &str) {
        if let Some(settings) = self.providers.get_mut(id)
            && let Some(acct) = settings.accounts.get_mut(account)
        {
            if key == "token" {
                acct.token = None;
            } else {
                acct.config.remove(key);
            }
        }
        self.prune_account(id, account);
    }

    /// An account's flat config keys (excluding typed fields), if any.
    pub fn account_config(&self, id: &str, account: &str) -> Option<&HashMap<String, String>> {
        self.account(id, account).map(|a| &a.config)
    }

    /// An account's auth token, if set.
    pub fn account_token(&self, id: &str, account: &str) -> Option<&str> {
        self.account(id, account).and_then(|a| a.token.as_deref())
    }

    /// An account's workspace list (empty when unset).
    pub fn account_workspaces(&self, id: &str, account: &str) -> &[String] {
        self.account(id, account)
            .map(|a| a.workspaces.as_slice())
            .unwrap_or(&[])
    }

    /// Replaces an account's workspace list, cleaning up empty entries.
    pub fn set_account_workspaces(&mut self, id: &str, account: &str, workspaces: Vec<String>) {
        self.account_entry(id, account).workspaces = workspaces;
        self.prune_account(id, account);
    }

    /// Explicit per-account toggle, if any.
    pub fn account_enabled(&self, id: &str, account: &str) -> Option<bool> {
        self.account(id, account).and_then(|a| a.enabled)
    }

    /// Sets the explicit per-account toggle.
    pub fn set_account_enabled(&mut self, id: &str, account: &str, enabled: bool) {
        self.account_entry(id, account).enabled = Some(enabled);
    }

    /// Clears the explicit per-account toggle, cleaning up empty entries.
    pub fn clear_account_enabled(&mut self, id: &str, account: &str) {
        if let Some(settings) = self.providers.get_mut(id)
            && let Some(acct) = settings.accounts.get_mut(account)
        {
            acct.enabled = None;
        }
        self.prune_account(id, account);
    }

    /// True when the account is enabled (explicit toggle wins, else enabled).
    pub fn account_is_enabled(&self, id: &str, account: &str) -> bool {
        self.account_enabled(id, account).unwrap_or(true)
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Gets a mutable account entry, creating provider and account as needed.
    fn account_entry(&mut self, id: &str, account: &str) -> &mut AccountSettings {
        self.providers
            .entry(id.to_string())
            .or_default()
            .accounts
            .entry(account.to_string())
            .or_default()
    }

    /// Drops an account if it became empty, then prunes the provider.
    fn prune_account(&mut self, id: &str, account: &str) {
        if let Some(settings) = self.providers.get_mut(id)
            && settings
                .accounts
                .get(account)
                .is_some_and(AccountSettings::is_empty)
        {
            settings.accounts.remove(account);
        }
        self.prune_provider(id);
    }

    /// Drops a provider entry when it holds no settings.
    fn prune_provider(&mut self, id: &str) {
        if self
            .providers
            .get(id)
            .is_some_and(ProviderSettings::is_empty)
        {
            self.providers.remove(id);
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
        cfg.set_account_config("anthropic", DEFAULT_ACCOUNT, "api_key", "sk-ant-x");

        cfg.set_account_config("opencode-go", DEFAULT_ACCOUNT, "token", "session=abc");
        cfg.set_account_workspaces("opencode-go", DEFAULT_ACCOUNT, vec!["wrk_a".into()]);
        cfg.set_account_config("claude", "work", "credentials_path", "/tmp/work.json");
        cfg.set_account_label("claude", "work", "Work Claude");

        cfg.save_to_path(&path).unwrap();
        let loaded = AppConfig::load_from_path(&path).unwrap();
        std::fs::remove_dir_all(path.parent().unwrap()).ok();

        assert_eq!(loaded, cfg);
        assert_eq!(loaded.provider_enabled("claude"), Some(true));
        assert_eq!(loaded.provider_enabled("openai"), Some(false));
        assert_eq!(loaded.provider_enabled("codex"), None);
        assert_eq!(
            loaded.account_config("anthropic", DEFAULT_ACCOUNT).unwrap()["api_key"],
            "sk-ant-x"
        );
        assert_eq!(
            loaded.account_token("opencode-go", DEFAULT_ACCOUNT),
            Some("session=abc")
        );
        assert_eq!(
            loaded.account_workspaces("opencode-go", DEFAULT_ACCOUNT),
            ["wrk_a".to_string()]
        );
        assert_eq!(loaded.account_label("claude", "work"), Some("Work Claude"));
    }

    #[test]
    fn test_parse_toml() {
        let cfg: AppConfig = toml::from_str(
            r#"
            [providers.claude]
            enabled = true

            [providers.claude.accounts.personal]
            label = "Personal"
            credentials_path = "~/.claude/.credentials.json"

            [providers.claude.accounts.work]
            enabled = false
            credentials_path = "/tmp/work.json"

            [providers.openai]
            enabled = false

            [providers.opencode-go.accounts.default]
            token = "session=abc"
            workspaces = ["wrk_a", "wrk_b"]
            "#,
        )
        .unwrap();
        assert_eq!(cfg.provider_enabled("claude"), Some(true));
        assert_eq!(cfg.provider_enabled("openai"), Some(false));
        assert_eq!(cfg.account_label("claude", "personal"), Some("Personal"));
        assert_eq!(
            cfg.account_config("claude", "personal").unwrap()["credentials_path"],
            "~/.claude/.credentials.json"
        );
        assert_eq!(cfg.account_enabled("claude", "work"), Some(false));
        assert!(!cfg.account_is_enabled("claude", "work"));
        assert!(cfg.account_is_enabled("claude", "personal"));
        assert_eq!(
            cfg.account_token("opencode-go", DEFAULT_ACCOUNT),
            Some("session=abc")
        );
        assert_eq!(
            cfg.account_workspaces("opencode-go", DEFAULT_ACCOUNT),
            ["wrk_a".to_string(), "wrk_b".to_string()]
        );
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

        // Configured accounts auto-enable a provider without an explicit toggle.
        cfg.set_account_config("d", "personal", "api_key", "x");
        assert_eq!(cfg.resolve_state("d", false), ProviderState::AutoEnabled);
    }

    #[test]
    fn test_account_config_set_get_unset() {
        let mut cfg = AppConfig::default();
        // `token` maps to the typed field, not the flat map.
        cfg.set_account_config("opencode-go", DEFAULT_ACCOUNT, "token", "session=abc");
        assert_eq!(
            cfg.account_token("opencode-go", DEFAULT_ACCOUNT),
            Some("session=abc")
        );
        assert!(
            cfg.account_config("opencode-go", DEFAULT_ACCOUNT)
                .unwrap()
                .is_empty()
        );

        cfg.unset_account_config("opencode-go", DEFAULT_ACCOUNT, "token");
        // Empty account and provider are removed entirely.
        assert!(cfg.account("opencode-go", DEFAULT_ACCOUNT).is_none());
        assert!(!cfg.providers.contains_key("opencode-go"));

        // Other keys land in the flat map.
        cfg.set_account_config("anthropic", DEFAULT_ACCOUNT, "api_key", "sk-x");
        assert_eq!(
            cfg.account_config("anthropic", DEFAULT_ACCOUNT).unwrap()["api_key"],
            "sk-x"
        );
        cfg.unset_account_config("anthropic", DEFAULT_ACCOUNT, "api_key");
        assert!(cfg.account("anthropic", DEFAULT_ACCOUNT).is_none());
    }

    #[test]
    fn test_unset_account_config_keeps_other_settings() {
        let mut cfg = AppConfig::default();
        cfg.set_account_label("opencode-go", DEFAULT_ACCOUNT, "Main");
        cfg.set_account_config("opencode-go", DEFAULT_ACCOUNT, "token", "x");
        cfg.unset_account_config("opencode-go", DEFAULT_ACCOUNT, "token");
        assert_eq!(
            cfg.account_label("opencode-go", DEFAULT_ACCOUNT),
            Some("Main")
        );
    }

    #[test]
    fn test_clear_provider_enabled_keeps_accounts() {
        let mut cfg = AppConfig::default();
        cfg.set_provider_enabled("claude", false);
        cfg.set_account_config("claude", "work", "credentials_path", "/tmp/w.json");
        cfg.clear_provider_enabled("claude");
        assert_eq!(cfg.provider_enabled("claude"), None);
        // Provider stays because it still has accounts.
        assert!(cfg.has_accounts("claude"));
    }

    #[test]
    fn test_clear_provider_enabled_removes_empty_provider() {
        let mut cfg = AppConfig::default();
        cfg.set_provider_enabled("claude", false);
        cfg.clear_provider_enabled("claude");
        assert_eq!(cfg.provider_enabled("claude"), None);
        assert!(!cfg.providers.contains_key("claude"));
    }

    #[test]
    fn test_add_remove_account() {
        let mut cfg = AppConfig::default();
        assert!(cfg.add_account("claude", "work", Some("Work")));
        assert!(!cfg.add_account("claude", "work", None)); // already exists
        assert_eq!(cfg.account_label("claude", "work"), Some("Work"));
        assert_eq!(cfg.account_ids("claude"), vec!["work".to_string()]);

        assert!(cfg.remove_account("claude", "work"));
        assert!(!cfg.remove_account("claude", "work"));
        assert!(!cfg.providers.contains_key("claude"));
    }

    #[test]
    fn test_account_enabled_toggle() {
        let mut cfg = AppConfig::default();
        cfg.set_account_config("claude", "work", "credentials_path", "/tmp/w.json");
        assert!(cfg.account_is_enabled("claude", "work"));
        cfg.set_account_enabled("claude", "work", false);
        assert!(!cfg.account_is_enabled("claude", "work"));
        cfg.clear_account_enabled("claude", "work");
        assert_eq!(cfg.account_enabled("claude", "work"), None);
        assert!(cfg.account_is_enabled("claude", "work"));
    }
}
