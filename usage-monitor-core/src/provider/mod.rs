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
}
