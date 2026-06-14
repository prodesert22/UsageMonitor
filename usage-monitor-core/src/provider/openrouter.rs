use async_trait::async_trait;

use crate::error::SpendPanelError;
use crate::model::{
    CostSnapshot, CreditsSnapshot, RateWindow, RateWindowStatus, SpendLimit, UsageSnapshot,
};
use crate::provider::{ProviderContext, ProviderMetadata, UsageProvider};

#[derive(Debug, serde::Deserialize)]
struct OpenRouterCreditsResponse {
    data: OpenRouterCreditsData,
}

#[derive(Debug, serde::Deserialize)]
struct OpenRouterCreditsData {
    total_credits: f64,
    total_usage: f64,
}

impl OpenRouterCreditsData {
    fn balance(&self) -> f64 {
        (self.total_credits - self.total_usage).max(0.0)
    }
}

#[derive(Debug, serde::Deserialize)]
struct OpenRouterKeyResponse {
    data: OpenRouterKeyData,
}

#[derive(Debug, serde::Deserialize, Clone, PartialEq)]
struct OpenRouterKeyData {
    limit: Option<f64>,
    usage: Option<f64>,
    usage_daily: Option<f64>,
    usage_weekly: Option<f64>,
    usage_monthly: Option<f64>,
    rate_limit: Option<OpenRouterRateLimit>,
}

#[derive(Debug, serde::Deserialize, Clone, PartialEq)]
struct OpenRouterRateLimit {
    requests: u64,
    interval: String,
}

#[derive(Debug, Clone, PartialEq)]
struct OpenRouterUsage {
    total_credits: f64,
    total_usage: f64,
    balance: f64,
    key_data_fetched: bool,
    key_limit: Option<f64>,
    key_usage: Option<f64>,
    key_usage_daily: Option<f64>,
    key_usage_weekly: Option<f64>,
    key_usage_monthly: Option<f64>,
    rate_limit: Option<OpenRouterRateLimit>,
}

impl OpenRouterUsage {
    fn has_valid_key_quota(&self) -> bool {
        matches!((self.key_limit, self.key_usage), (Some(limit), Some(usage)) if limit > 0.0 && usage >= 0.0)
    }

    fn key_used_ratio(&self) -> Option<f64> {
        if !self.has_valid_key_quota() {
            return None;
        }
        Some((self.key_usage.unwrap() / self.key_limit.unwrap()).clamp(0.0, 1.0))
    }
}

/// OpenRouter API credits provider.
pub struct OpenRouterProvider {
    metadata: ProviderMetadata,
    /// Base URL override for tests.
    base_url: Option<String>,
}

impl OpenRouterProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: "openrouter",
                name: "OpenRouter",
                description: "OpenRouter credits and API-key usage monitor",
                auth_methods: &["api_key", "env"],
                website: Some("https://openrouter.ai"),
            },
            base_url: None,
        }
    }

    /// Creates a provider with a custom base URL (for tests).
    pub fn with_base_url(url: &str) -> Self {
        let mut p = Self::new();
        p.base_url = Some(url.to_string());
        p
    }

    fn clean(raw: &str) -> String {
        let mut value = raw.trim();
        if value.len() >= 2
            && ((value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\'')))
        {
            value = &value[1..value.len() - 1];
        }
        value.trim().to_string()
    }

    fn detect_credentials_from(key: Option<&str>) -> bool {
        key.map(Self::clean).is_some_and(|key| !key.is_empty())
    }

    fn resolve_api_key(ctx: &ProviderContext) -> Result<String, SpendPanelError> {
        for key in ["api_key", "token"] {
            if let Some(value) = ctx.config.get(key) {
                let cleaned = Self::clean(value);
                if !cleaned.is_empty() {
                    return Ok(cleaned);
                }
            }
        }
        if let Ok(value) = std::env::var("OPENROUTER_API_KEY") {
            let cleaned = Self::clean(&value);
            if !cleaned.is_empty() {
                return Ok(cleaned);
            }
        }
        Err(SpendPanelError::AuthFailed(
            "openrouter".into(),
            "no API key found in config, token, or OPENROUTER_API_KEY".into(),
        ))
    }

    fn api_base(&self, ctx: &ProviderContext) -> String {
        let configured = ctx
            .config
            .get("api_url")
            .or_else(|| ctx.config.get("base_url"))
            .map(String::as_str)
            .filter(|value| !value.is_empty())
            .map(Self::clean)
            .or_else(|| {
                std::env::var("OPENROUTER_API_URL")
                    .ok()
                    .map(|v| Self::clean(&v))
            })
            .or_else(|| self.base_url.clone())
            .unwrap_or_else(|| "https://openrouter.ai/api/v1".into());

        if configured.starts_with("http://") || configured.starts_with("https://") {
            configured.trim_end_matches('/').to_string()
        } else {
            format!("https://{}", configured.trim_end_matches('/'))
        }
    }

    fn build_client(ctx: &ProviderContext) -> Result<reqwest::Client, SpendPanelError> {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(ctx.timeout_secs))
            .build()
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))
    }

    fn add_headers(
        builder: reqwest::RequestBuilder,
        api_key: &str,
        ctx: &ProviderContext,
    ) -> reqwest::RequestBuilder {
        let builder = builder
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Accept", "application/json");

        let referer = ctx
            .config
            .get("http_referer")
            .map(|v| Self::clean(v))
            .filter(|v| !v.is_empty())
            .or_else(|| {
                std::env::var("OPENROUTER_HTTP_REFERER")
                    .ok()
                    .map(|v| Self::clean(&v))
            });
        let builder = if let Some(referer) = referer {
            builder.header("HTTP-Referer", referer)
        } else {
            builder
        };

        let title = ctx
            .config
            .get("x_title")
            .map(|v| Self::clean(v))
            .filter(|v| !v.is_empty())
            .or_else(|| {
                std::env::var("OPENROUTER_X_TITLE")
                    .ok()
                    .map(|v| Self::clean(&v))
            })
            .unwrap_or_else(|| "UsageMonitor".into());
        builder.header("X-Title", title)
    }

    async fn get_json<T: serde::de::DeserializeOwned>(
        client: &reqwest::Client,
        url: String,
        api_key: &str,
        ctx: &ProviderContext,
    ) -> Result<T, SpendPanelError> {
        let resp = Self::add_headers(client.get(url), api_key, ctx)
            .send()
            .await
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;
        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;

        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(SpendPanelError::AuthFailed(
                "openrouter".into(),
                format!("invalid API key (HTTP {})", status.as_u16()),
            ));
        }
        if !status.is_success() {
            return Err(SpendPanelError::ProviderError(
                "openrouter".into(),
                format!("HTTP {}", status),
            ));
        }

        serde_json::from_str(&body)
            .map_err(|e| SpendPanelError::ParseError("openrouter".into(), e.to_string()))
    }

    async fn fetch_key_data(
        client: &reqwest::Client,
        base_url: &str,
        api_key: &str,
        ctx: &ProviderContext,
    ) -> Option<OpenRouterKeyData> {
        let result = Self::get_json::<OpenRouterKeyResponse>(
            client,
            format!("{}/key", base_url),
            api_key,
            ctx,
        )
        .await;
        result.ok().map(|response| response.data)
    }

    async fn fetch_usage_data(
        &self,
        client: &reqwest::Client,
        api_key: &str,
        ctx: &ProviderContext,
    ) -> Result<OpenRouterUsage, SpendPanelError> {
        let base_url = self.api_base(ctx);
        let credits = Self::get_json::<OpenRouterCreditsResponse>(
            client,
            format!("{}/credits", base_url),
            api_key,
            ctx,
        )
        .await?;
        let key_data = Self::fetch_key_data(client, &base_url, api_key, ctx).await;

        Ok(OpenRouterUsage {
            total_credits: credits.data.total_credits,
            total_usage: credits.data.total_usage,
            balance: credits.data.balance(),
            key_data_fetched: key_data.is_some(),
            key_limit: key_data.as_ref().and_then(|data| data.limit),
            key_usage: key_data.as_ref().and_then(|data| data.usage),
            key_usage_daily: key_data.as_ref().and_then(|data| data.usage_daily),
            key_usage_weekly: key_data.as_ref().and_then(|data| data.usage_weekly),
            key_usage_monthly: key_data.as_ref().and_then(|data| data.usage_monthly),
            rate_limit: key_data.and_then(|data| data.rate_limit),
        })
    }

    fn snapshot_from_usage(usage: OpenRouterUsage) -> UsageSnapshot {
        let mut snapshot = UsageSnapshot::new("openrouter");

        let mut credits = CreditsSnapshot::new(usage.balance, "USD");
        credits.total = Some(usage.total_credits);
        credits.used = Some(usage.total_usage);
        snapshot.credits = Some(credits);

        if let Some(ratio) = usage.key_used_ratio() {
            snapshot.primary_rate_window = Some(RateWindow {
                label: "API key limit".into(),
                window_minutes: 0,
                usage_ratio: ratio,
                limit: None,
                used: None,
                remaining: None,
                resets_at: None,
                status: RateWindowStatus::from_ratio(ratio),
            });
            snapshot.cost = Some(CostSnapshot {
                total_cost: usage.key_usage_monthly.or(usage.key_usage),
                currency: "USD".into(),
                daily_costs: Vec::new(),
                spend_limit: Some(SpendLimit {
                    limit: usage.key_limit.unwrap(),
                    used: usage.key_usage.unwrap(),
                    period: "api-key".into(),
                }),
            });
        } else if usage.key_usage_monthly.is_some() || usage.key_usage.is_some() {
            snapshot.cost = Some(CostSnapshot {
                total_cost: usage.key_usage_monthly.or(usage.key_usage),
                currency: "USD".into(),
                daily_costs: Vec::new(),
                spend_limit: None,
            });
        }

        if let Some(rate_limit) = usage.rate_limit {
            snapshot.plan = Some(crate::model::PlanInfo {
                name: "OpenRouter API".into(),
                tier: None,
                features: vec![format!(
                    "{} requests per {}",
                    rate_limit.requests, rate_limit.interval
                )],
                price: None,
                currency: None,
                billing_period: None,
            });
        }

        snapshot
    }
}

impl Default for OpenRouterProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl UsageProvider for OpenRouterProvider {
    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    fn detect_credentials(&self) -> bool {
        Self::detect_credentials_from(std::env::var("OPENROUTER_API_KEY").ok().as_deref())
    }

    async fn fetch_usage(&self, ctx: &ProviderContext) -> Result<UsageSnapshot, SpendPanelError> {
        let api_key = Self::resolve_api_key(ctx)?;
        let client = Self::build_client(ctx)?;
        let usage = self.fetch_usage_data(&client, &api_key, ctx).await?;
        Ok(Self::snapshot_from_usage(usage))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn test_provider_metadata() {
        let provider = OpenRouterProvider::new();
        let meta = provider.metadata();
        assert_eq!(meta.id, "openrouter");
        assert_eq!(meta.name, "OpenRouter");
        assert!(meta.auth_methods.contains(&"api_key"));
    }

    #[test]
    fn test_clean_trims_and_unquotes() {
        assert_eq!(OpenRouterProvider::clean("  sk-or  "), "sk-or");
        assert_eq!(OpenRouterProvider::clean("\"sk-or\""), "sk-or");
        assert_eq!(OpenRouterProvider::clean("'sk-or'"), "sk-or");
    }

    #[test]
    fn test_resolve_api_key_from_context_token() {
        let mut ctx = ProviderContext::new();
        ctx.config.insert("token".into(), "sk-or-token".into());
        assert_eq!(
            OpenRouterProvider::resolve_api_key(&ctx).unwrap(),
            "sk-or-token"
        );
    }

    #[test]
    fn test_api_base_accepts_bare_host() {
        let mut ctx = ProviderContext::new();
        ctx.config
            .insert("api_url".into(), "openrouter.example/api/v1/".into());
        assert_eq!(
            OpenRouterProvider::new().api_base(&ctx),
            "https://openrouter.example/api/v1"
        );
    }

    #[test]
    fn test_snapshot_with_key_quota() {
        let snapshot = OpenRouterProvider::snapshot_from_usage(OpenRouterUsage {
            total_credits: 100.0,
            total_usage: 40.0,
            balance: 60.0,
            key_data_fetched: true,
            key_limit: Some(20.0),
            key_usage: Some(5.0),
            key_usage_daily: Some(0.12),
            key_usage_weekly: Some(0.74),
            key_usage_monthly: Some(4.56),
            rate_limit: Some(OpenRouterRateLimit {
                requests: 120,
                interval: "10s".into(),
            }),
        });
        assert_eq!(snapshot.credits.as_ref().unwrap().balance, 60.0);
        assert_eq!(snapshot.credits.as_ref().unwrap().used, Some(40.0));
        assert_eq!(snapshot.primary_rate_window.unwrap().usage_ratio, 0.25);
        assert_eq!(snapshot.cost.as_ref().unwrap().total_cost, Some(4.56));
        assert_eq!(snapshot.cost.unwrap().spend_limit.unwrap().used, 5.0);
        assert_eq!(snapshot.plan.unwrap().features[0], "120 requests per 10s");
    }

    #[test]
    fn test_snapshot_without_key_quota_omits_primary_window() {
        let snapshot = OpenRouterProvider::snapshot_from_usage(OpenRouterUsage {
            total_credits: 50.0,
            total_usage: 45.0,
            balance: 5.0,
            key_data_fetched: false,
            key_limit: None,
            key_usage: None,
            key_usage_daily: None,
            key_usage_weekly: None,
            key_usage_monthly: None,
            rate_limit: None,
        });
        assert!(snapshot.primary_rate_window.is_none());
        assert_eq!(snapshot.credits.unwrap().balance, 5.0);
    }

    #[tokio::test]
    async fn test_fetch_usage_success_with_key_enrichment() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/credits"))
            .and(header("authorization", "Bearer sk-or-test"))
            .and(header("accept", "application/json"))
            .and(header("http-referer", "https://usage.example"))
            .and(header("x-title", "UsageMonitor QA"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{"data":{"total_credits":100,"total_usage":40}}"#,
                "application/json",
            ))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v1/key"))
            .and(header("authorization", "Bearer sk-or-test"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{"data":{"limit":20,"usage":0.5,"usage_daily":0.12,"usage_weekly":0.74,"usage_monthly":4.56,"rate_limit":{"requests":120,"interval":"10s"}}}"#,
                "application/json",
            ))
            .mount(&server)
            .await;

        let provider = OpenRouterProvider::with_base_url(&format!("{}/api/v1", server.uri()));
        let mut ctx = ProviderContext::with_api_key("sk-or-test");
        ctx.config
            .insert("http_referer".into(), " https://usage.example ".into());
        ctx.config
            .insert("x_title".into(), "UsageMonitor QA".into());
        let snapshot = provider.fetch_usage(&ctx).await.unwrap();

        assert_eq!(snapshot.credits.as_ref().unwrap().balance, 60.0);
        assert_eq!(snapshot.primary_rate_window.unwrap().usage_ratio, 0.025);
        assert_eq!(snapshot.cost.unwrap().total_cost, Some(4.56));
    }

    #[tokio::test]
    async fn test_fetch_usage_key_failure_keeps_credits() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/credits"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{"data":{"total_credits":100,"total_usage":40}}"#,
                "application/json",
            ))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v1/key"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let provider = OpenRouterProvider::with_base_url(&format!("{}/api/v1", server.uri()));
        let snapshot = provider
            .fetch_usage(&ProviderContext::with_api_key("sk-or-test"))
            .await
            .unwrap();
        assert_eq!(snapshot.credits.unwrap().balance, 60.0);
        assert!(snapshot.primary_rate_window.is_none());
    }

    #[tokio::test]
    async fn test_fetch_usage_401_is_auth_failed() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/credits"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let provider = OpenRouterProvider::with_base_url(&format!("{}/api/v1", server.uri()));
        let err = provider
            .fetch_usage(&ProviderContext::with_api_key("bad"))
            .await
            .unwrap_err();
        assert!(matches!(err, SpendPanelError::AuthFailed(_, _)));
    }
}
