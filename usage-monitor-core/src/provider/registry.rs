use std::collections::HashMap;

use futures_util::future::join_all;

use crate::config::{AppConfig, ProviderState};
use crate::error::SpendPanelError;

use super::{ProviderContext, ProviderMetadata, UsageProvider};

/// Registry of all available providers.
pub struct ProviderRegistry {
    providers: HashMap<&'static str, Box<dyn UsageProvider>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }

    /// Registry with all built-in providers registered.
    pub fn with_defaults() -> Self {
        let mut reg = Self::new();
        reg.register(Box::new(super::anthropic::AnthropicProvider::new()));
        reg.register(Box::new(super::claude::ClaudeProvider::new()));
        reg.register(Box::new(super::codex::CodexProvider::new()));
        reg.register(Box::new(super::opencode_go::OpenCodeGoProvider::new()));
        reg.register(Box::new(super::openai::OpenAIProvider::new()));
        reg
    }

    /// Registers a provider.
    pub fn register(&mut self, provider: Box<dyn UsageProvider>) {
        let id = provider.metadata().id;
        self.providers.insert(id, provider);
    }

    /// Returns a provider by ID.
    pub fn get(&self, id: &str) -> Option<&dyn UsageProvider> {
        self.providers.get(id).map(|p| p.as_ref())
    }

    /// Lists all registered providers.
    pub fn all(&self) -> Vec<&dyn UsageProvider> {
        self.providers.values().map(|p| p.as_ref()).collect()
    }

    /// Returns metadata for all providers.
    pub fn all_metadata(&self) -> Vec<&ProviderMetadata> {
        self.providers.values().map(|p| p.metadata()).collect()
    }

    /// Fetches usage from a specific provider.
    pub async fn fetch(
        &self,
        id: &str,
        ctx: &ProviderContext,
    ) -> Result<crate::model::UsageSnapshot, SpendPanelError> {
        match self.get(id) {
            Some(provider) => provider.fetch_usage(ctx).await,
            None => Err(SpendPanelError::ProviderNotFound(id.to_string())),
        }
    }

    /// Resolves the enablement state of a provider: explicit config toggle
    /// wins, otherwise credential detection decides.
    pub fn provider_state(&self, id: &str, config: &AppConfig) -> Option<ProviderState> {
        let provider = self.get(id)?;
        Some(config.resolve_state(id, provider.detect_credentials()))
    }

    /// IDs of all enabled providers (explicitly or by credential detection).
    pub fn enabled_ids(&self, config: &AppConfig) -> Vec<String> {
        let mut ids: Vec<String> = self
            .all()
            .iter()
            .filter(|p| {
                config
                    .resolve_state(p.metadata().id, p.detect_credentials())
                    .is_enabled()
            })
            .map(|p| p.metadata().id.to_string())
            .collect();
        ids.sort();
        ids
    }

    /// Fetches usage from all enabled providers concurrently.
    pub async fn fetch_enabled(
        &self,
        config: &AppConfig,
        ctx_overrides: Option<&HashMap<String, ProviderContext>>,
    ) -> Vec<(String, Result<crate::model::UsageSnapshot, SpendPanelError>)> {
        let fetches = self.enabled_ids(config).into_iter().map(|id| {
            let ctx = ctx_overrides
                .and_then(|o| o.get(id.as_str()))
                .cloned()
                .unwrap_or_default();
            async move {
                let result = self.fetch(&id, &ctx).await;
                (id, result)
            }
        });
        join_all(fetches).await
    }

    /// Fetches usage from all registered providers concurrently.
    pub async fn fetch_all(
        &self,
        ctx_overrides: Option<&HashMap<String, ProviderContext>>,
    ) -> Vec<(String, Result<crate::model::UsageSnapshot, SpendPanelError>)> {
        let fetches = self.all().into_iter().map(|provider| {
            let id = provider.metadata().id.to_string();
            let ctx = ctx_overrides
                .and_then(|o| o.get(id.as_str()))
                .cloned()
                .unwrap_or_default();
            async move {
                let result = provider.fetch_usage(&ctx).await;
                (id, result)
            }
        });
        join_all(fetches).await
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// Default context
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::UsageSnapshot;
    use async_trait::async_trait;

    struct MockProvider {
        meta: ProviderMetadata,
        should_fail: bool,
    }

    impl MockProvider {
        fn new(id: &'static str) -> Self {
            Self {
                meta: ProviderMetadata {
                    id,
                    name: id,
                    description: "mock",
                    auth_methods: &["mock"],
                    website: None,
                },
                should_fail: false,
            }
        }

        fn failing(id: &'static str) -> Self {
            Self {
                meta: ProviderMetadata {
                    id,
                    name: id,
                    description: "mock",
                    auth_methods: &["mock"],
                    website: None,
                },
                should_fail: true,
            }
        }
    }

    #[async_trait]
    impl UsageProvider for MockProvider {
        fn metadata(&self) -> &ProviderMetadata {
            &self.meta
        }

        async fn fetch_usage(
            &self,
            _ctx: &ProviderContext,
        ) -> Result<UsageSnapshot, SpendPanelError> {
            if self.should_fail {
                Err(SpendPanelError::ProviderError(
                    self.id().into(),
                    "mock fail".into(),
                ))
            } else {
                Ok(UsageSnapshot::new(self.id()))
            }
        }
    }

    impl MockProvider {
        fn id(&self) -> &'static str {
            self.meta.id
        }
    }

    #[test]
    fn test_registry_new() {
        let reg = ProviderRegistry::new();
        assert!(reg.all().is_empty());
    }

    #[test]
    fn test_registry_register_and_get() {
        let mut reg = ProviderRegistry::new();
        reg.register(Box::new(MockProvider::new("mock-provider")));

        assert!(reg.get("mock-provider").is_some());
        assert!(reg.get("nonexistent").is_none());
    }

    #[test]
    fn test_registry_all_metadata() {
        let mut reg = ProviderRegistry::new();
        reg.register(Box::new(MockProvider::new("p1")));
        reg.register(Box::new(MockProvider::new("p2")));

        let meta = reg.all_metadata();
        assert_eq!(meta.len(), 2);
        let ids: Vec<&str> = meta.iter().map(|m| m.id).collect();
        assert!(ids.contains(&"p1"));
        assert!(ids.contains(&"p2"));
    }

    #[tokio::test]
    async fn test_fetch_success() {
        let mut reg = ProviderRegistry::new();
        reg.register(Box::new(MockProvider::new("ok")));

        let result = reg.fetch("ok", &ProviderContext::new()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().provider_id, "ok");
    }

    #[tokio::test]
    async fn test_fetch_not_found() {
        let reg = ProviderRegistry::new();
        let result = reg.fetch("ghost", &ProviderContext::new()).await;
        assert!(matches!(result, Err(SpendPanelError::ProviderNotFound(_))));
    }

    #[tokio::test]
    async fn test_fetch_failure() {
        let mut reg = ProviderRegistry::new();
        reg.register(Box::new(MockProvider::failing("bad")));

        let result = reg.fetch("bad", &ProviderContext::new()).await;
        assert!(result.is_err());
    }

    struct DetectableProvider {
        meta: ProviderMetadata,
    }

    struct DelayedProvider {
        meta: ProviderMetadata,
        delay: std::time::Duration,
    }

    #[async_trait]
    impl UsageProvider for DetectableProvider {
        fn metadata(&self) -> &ProviderMetadata {
            &self.meta
        }

        fn detect_credentials(&self) -> bool {
            true
        }

        async fn fetch_usage(
            &self,
            _ctx: &ProviderContext,
        ) -> Result<UsageSnapshot, SpendPanelError> {
            Ok(UsageSnapshot::new(self.meta.id))
        }
    }

    fn detectable(id: &'static str) -> DetectableProvider {
        DetectableProvider {
            meta: ProviderMetadata {
                id,
                name: id,
                description: "mock",
                auth_methods: &["mock"],
                website: None,
            },
        }
    }

    fn delayed(id: &'static str, delay: std::time::Duration) -> DelayedProvider {
        DelayedProvider {
            meta: ProviderMetadata {
                id,
                name: id,
                description: "delayed",
                auth_methods: &["mock"],
                website: None,
            },
            delay,
        }
    }

    #[async_trait]
    impl UsageProvider for DelayedProvider {
        fn metadata(&self) -> &ProviderMetadata {
            &self.meta
        }

        fn detect_credentials(&self) -> bool {
            true
        }

        async fn fetch_usage(
            &self,
            _ctx: &ProviderContext,
        ) -> Result<UsageSnapshot, SpendPanelError> {
            tokio::time::sleep(self.delay).await;
            Ok(UsageSnapshot::new(self.meta.id))
        }
    }

    #[test]
    fn test_provider_state_and_enabled_ids() {
        use crate::config::{AppConfig, ProviderState};

        let mut reg = ProviderRegistry::new();
        reg.register(Box::new(detectable("auto-on"))); // detect = true
        reg.register(Box::new(MockProvider::new("auto-off"))); // detect = false
        reg.register(Box::new(MockProvider::new("forced-on")));
        reg.register(Box::new(detectable("forced-off")));

        let mut cfg = AppConfig::default();
        cfg.set_provider_enabled("forced-on", true);
        cfg.set_provider_enabled("forced-off", false);

        assert_eq!(
            reg.provider_state("auto-on", &cfg),
            Some(ProviderState::AutoEnabled)
        );
        assert_eq!(
            reg.provider_state("auto-off", &cfg),
            Some(ProviderState::AutoDisabled)
        );
        assert_eq!(
            reg.provider_state("forced-on", &cfg),
            Some(ProviderState::Enabled)
        );
        assert_eq!(
            reg.provider_state("forced-off", &cfg),
            Some(ProviderState::Disabled)
        );
        assert_eq!(reg.provider_state("ghost", &cfg), None);

        assert_eq!(reg.enabled_ids(&cfg), vec!["auto-on", "forced-on"]);
    }

    #[tokio::test]
    async fn test_fetch_enabled_skips_disabled() {
        use crate::config::AppConfig;

        let mut reg = ProviderRegistry::new();
        reg.register(Box::new(detectable("on")));
        reg.register(Box::new(detectable("off")));

        let mut cfg = AppConfig::default();
        cfg.set_provider_enabled("off", false);

        let results = reg.fetch_enabled(&cfg, None).await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "on");
        assert!(results[0].1.is_ok());
    }

    #[tokio::test]
    async fn test_fetch_enabled_runs_providers_concurrently() {
        let mut reg = ProviderRegistry::new();
        let delay = std::time::Duration::from_millis(250);
        reg.register(Box::new(delayed("slow-a", delay)));
        reg.register(Box::new(delayed("slow-b", delay)));

        let start = std::time::Instant::now();
        let results = reg.fetch_enabled(&AppConfig::default(), None).await;
        let elapsed = start.elapsed();

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|(_, result)| result.is_ok()));
        assert!(
            elapsed < std::time::Duration::from_millis(450),
            "fetch_enabled should run providers concurrently; took {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn test_fetch_all() {
        let mut reg = ProviderRegistry::new();
        reg.register(Box::new(MockProvider::new("ok")));
        reg.register(Box::new(MockProvider::failing("bad")));

        let results = reg.fetch_all(None).await;
        assert_eq!(results.len(), 2);

        let ok_result = results.iter().find(|(id, _)| id == "ok").unwrap();
        assert!(ok_result.1.is_ok());

        let bad_result = results.iter().find(|(id, _)| id == "bad").unwrap();
        assert!(bad_result.1.is_err());
    }
}
