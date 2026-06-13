use std::collections::HashMap;

use futures_util::future::join_all;

use crate::config::{AppConfig, DEFAULT_ACCOUNT, ProviderState};
use crate::error::SpendPanelError;

use super::{ProviderContext, ProviderMetadata, UsageProvider};

/// One unit of fetch work: a single account of a provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountTarget {
    /// Provider this account belongs to.
    pub provider_id: String,
    /// Account name (`"default"` for the implicit single account).
    pub account_id: String,
    /// Human-friendly label, when configured.
    pub label: Option<String>,
    /// Whether the account is explicitly configured (vs. the implicit default).
    pub explicit: bool,
}

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

    /// Account targets for a single provider.
    ///
    /// Each configured (enabled) account becomes a target. In addition, the
    /// implicit auto-detected `default` account is included — so a named
    /// account lives *alongside* the auto-detected login rather than replacing
    /// it — unless an explicit `default` account is configured. The implicit
    /// default is added when the provider detects credentials, or when no other
    /// account is configured (so a bare `fetch <provider>` still tries it).
    ///
    /// To drop the auto-detected default while keeping named accounts, disable
    /// it: `<provider> account disable default`.
    pub fn provider_targets(&self, id: &str, config: &AppConfig) -> Vec<AccountTarget> {
        let mut targets: Vec<AccountTarget> = config
            .account_ids(id)
            .into_iter()
            .filter(|acct| config.account_is_enabled(id, acct))
            .map(|acct| AccountTarget {
                label: config.account_label(id, &acct).map(str::to_string),
                provider_id: id.to_string(),
                account_id: acct,
                explicit: true,
            })
            .collect();

        // An explicit `default` account (even if disabled) takes over the
        // default slot; otherwise add the implicit auto-detected one.
        if config.account(id, DEFAULT_ACCOUNT).is_none() {
            let detected = self.get(id).is_some_and(|p| p.detect_credentials());
            if detected || targets.is_empty() {
                targets.insert(
                    0,
                    AccountTarget {
                        provider_id: id.to_string(),
                        account_id: DEFAULT_ACCOUNT.to_string(),
                        label: None,
                        explicit: false,
                    },
                );
            }
        }
        targets
    }

    /// All account targets across enabled providers.
    pub fn enabled_targets(&self, config: &AppConfig) -> Vec<AccountTarget> {
        self.enabled_ids(config)
            .into_iter()
            .flat_map(|id| self.provider_targets(&id, config))
            .collect()
    }

    /// Fetches a list of account targets concurrently. `ctx_for` builds the
    /// fetch context for each target. Successful snapshots are stamped with the
    /// target's account id/label.
    pub async fn fetch_targets<F>(
        &self,
        targets: Vec<AccountTarget>,
        ctx_for: F,
    ) -> Vec<(AccountTarget, Result<crate::model::UsageSnapshot, SpendPanelError>)>
    where
        F: Fn(&AccountTarget) -> ProviderContext,
    {
        let fetches = targets.into_iter().map(|target| {
            let ctx = ctx_for(&target);
            async move {
                let mut result = self.fetch(&target.provider_id, &ctx).await;
                if let Ok(snapshot) = &mut result {
                    if target.explicit {
                        snapshot.account_id = Some(target.account_id.clone());
                    }
                    snapshot.account_label = target.label.clone();
                }
                (target, result)
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
    async fn test_enabled_targets_skips_disabled() {
        use crate::config::AppConfig;

        let mut reg = ProviderRegistry::new();
        reg.register(Box::new(detectable("on")));
        reg.register(Box::new(detectable("off")));

        let mut cfg = AppConfig::default();
        cfg.set_provider_enabled("off", false);

        let targets = reg.enabled_targets(&cfg);
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].provider_id, "on");
        assert_eq!(targets[0].account_id, "default");
        assert!(!targets[0].explicit);

        let results = reg
            .fetch_targets(targets, |_| ProviderContext::new())
            .await;
        assert_eq!(results.len(), 1);
        assert!(results[0].1.is_ok());
    }

    #[tokio::test]
    async fn test_provider_targets_expand_accounts() {
        use crate::config::AppConfig;

        // Non-detecting provider: no implicit default is added.
        let mut reg = ProviderRegistry::new();
        reg.register(Box::new(MockProvider::new("p")));

        let mut cfg = AppConfig::default();
        cfg.set_account_label("p", "work", "Work");
        cfg.set_account_config("p", "home", "api_key", "x");
        cfg.set_account_enabled("p", "home", false);

        let targets = reg.provider_targets("p", &cfg);
        // Only the enabled "work" account remains (no creds → no auto default).
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].account_id, "work");
        assert_eq!(targets[0].label.as_deref(), Some("Work"));
        assert!(targets[0].explicit);

        let results = reg
            .fetch_targets(targets, |_| ProviderContext::new())
            .await;
        let snap = results[0].1.as_ref().unwrap();
        assert_eq!(snap.account_id.as_deref(), Some("work"));
        assert_eq!(snap.account_label.as_deref(), Some("Work"));
    }

    #[test]
    fn test_auto_default_coexists_with_named_accounts() {
        use crate::config::AppConfig;

        // Detecting provider: implicit default is added alongside named accounts.
        let mut reg = ProviderRegistry::new();
        reg.register(Box::new(detectable("p")));

        let mut cfg = AppConfig::default();
        cfg.set_account_config("p", "work", "credentials_path", "/tmp/w.json");

        let targets = reg.provider_targets("p", &cfg);
        assert_eq!(targets.len(), 2);
        // Auto default comes first, then the named account.
        assert_eq!(targets[0].account_id, "default");
        assert!(!targets[0].explicit);
        assert_eq!(targets[1].account_id, "work");
        assert!(targets[1].explicit);
    }

    #[test]
    fn test_explicit_default_account_replaces_auto() {
        use crate::config::AppConfig;

        let mut reg = ProviderRegistry::new();
        reg.register(Box::new(detectable("p")));

        // An explicit `default` account takes over the default slot — no
        // duplicate implicit target.
        let mut cfg = AppConfig::default();
        cfg.set_account_config("p", "default", "credentials_path", "/tmp/d.json");
        cfg.set_account_config("p", "work", "credentials_path", "/tmp/w.json");

        let targets = reg.provider_targets("p", &cfg);
        assert_eq!(targets.len(), 2, "no duplicate default");
        assert!(targets.iter().all(|t| t.explicit));

        // Disabling the explicit default drops it without re-adding the auto one.
        cfg.set_account_enabled("p", "default", false);
        let targets = reg.provider_targets("p", &cfg);
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].account_id, "work");
    }

    #[tokio::test]
    async fn test_fetch_targets_runs_concurrently() {
        let mut reg = ProviderRegistry::new();
        let delay = std::time::Duration::from_millis(250);
        reg.register(Box::new(delayed("slow-a", delay)));
        reg.register(Box::new(delayed("slow-b", delay)));

        let start = std::time::Instant::now();
        let targets = reg.enabled_targets(&AppConfig::default());
        let results = reg
            .fetch_targets(targets, |_| ProviderContext::new())
            .await;
        let elapsed = start.elapsed();

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|(_, result)| result.is_ok()));
        assert!(
            elapsed < std::time::Duration::from_millis(450),
            "fetch_targets should run concurrently; took {:?}",
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
