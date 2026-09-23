pub mod abacus;
pub mod anthropic;
pub mod antigravity;
pub mod claude;
pub mod codex;
pub mod copilot;
pub mod cursor;
pub mod deepgram;
pub mod deepseek;
pub mod devin;
pub mod elevenlabs;
pub mod gemini;
pub mod gemini_oauth;
pub mod grok;
pub mod groq;
pub mod kimi;
pub mod kimik2;
pub mod llmproxy;
pub mod minimax;
pub mod mistral;
pub mod moonshot;
pub mod ollama;
pub mod openai;
pub mod opencode_go;
pub mod openrouter;
pub mod perplexity;
pub mod proto;
pub mod registry;
pub mod venice;
pub mod windsurf;
pub mod zai;

use async_trait::async_trait;
use std::collections::HashMap;

use crate::error::SpendPanelError;
use crate::model::UsageSnapshot;

/// Context for fetching from a provider.
#[derive(Debug, Clone)]
pub struct ProviderContext {
    /// Provider-specific configuration (key-value).
    pub config: HashMap<String, String>,
    /// Timeout in seconds.
    pub timeout_secs: u64,
}

impl Default for ProviderContext {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderContext {
    pub fn new() -> Self {
        Self {
            config: HashMap::new(),
            timeout_secs: 30,
        }
    }

    pub fn with_api_key(key: impl Into<String>) -> Self {
        let mut ctx = Self::new();
        ctx.config.insert("api_key".into(), key.into());
        ctx
    }
}

/// Expands a user-configured credentials path: trims surrounding whitespace
/// and expands a leading `~` (or `~/`) to `$HOME`. Falls back to the raw
/// (trimmed) value when `HOME` is unavailable.
pub fn expand_credentials_path(raw: &str) -> std::path::PathBuf {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix('~') {
        if let Some(home) = std::env::var_os("HOME") {
            let mut path = std::path::PathBuf::from(home);
            let rest = rest.strip_prefix('/').unwrap_or(rest);
            if !rest.is_empty() {
                path.push(rest);
            }
            return path;
        }
    }
    std::path::PathBuf::from(trimmed)
}

/// Resolves a configured credentials path to a file: expands `~`/whitespace
/// and, when the result is an existing directory, joins `file_name`
/// (e.g. `~/.codex-work` → `~/.codex-work/auth.json`).
pub fn resolve_credentials_file(raw: &str, file_name: &str) -> std::path::PathBuf {
    let path = expand_credentials_path(raw);
    if path.is_dir() {
        path.join(file_name)
    } else {
        path
    }
}

/// Provider metadata.
#[derive(Debug, Clone)]
pub struct ProviderMetadata {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub auth_methods: &'static [&'static str],
    pub website: Option<&'static str>,
}

/// Trait every usage provider must implement.
#[async_trait]
pub trait UsageProvider: Send + Sync {
    /// Returns the provider metadata.
    fn metadata(&self) -> &ProviderMetadata;

    /// Fetches usage data.
    async fn fetch_usage(&self, ctx: &ProviderContext) -> Result<UsageSnapshot, SpendPanelError>;

    /// Whether credentials for this provider are detectable on this machine
    /// (used to auto-enable providers without an explicit toggle).
    fn detect_credentials(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_context_default() {
        let ctx = ProviderContext::new();
        assert!(ctx.config.is_empty());
        assert_eq!(ctx.timeout_secs, 30);
    }

    #[test]
    fn test_provider_context_with_api_key() {
        let ctx = ProviderContext::with_api_key("sk-test");
        assert_eq!(ctx.config.get("api_key").unwrap(), "sk-test");
    }

    #[test]
    fn test_expand_credentials_path_trims_and_expands_tilde() {
        let home = std::env::var_os("HOME").expect("HOME set for test");
        let expanded = expand_credentials_path("  ~/.codex-plus2/auth.json  ");
        assert_eq!(
            expanded,
            std::path::PathBuf::from(home).join(".codex-plus2/auth.json")
        );
        assert_eq!(
            expand_credentials_path("/tmp/auth.json "),
            std::path::PathBuf::from("/tmp/auth.json")
        );
    }

    #[test]
    fn test_resolve_credentials_file_accepts_directory() {
        let dir = std::env::temp_dir().join(format!("usage-monitor-creds-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let resolved = resolve_credentials_file(dir.to_str().unwrap(), "auth.json");
        assert_eq!(resolved, dir.join("auth.json"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
