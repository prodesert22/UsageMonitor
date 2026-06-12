use async_trait::async_trait;
use chrono::Utc;

use crate::error::SpendPanelError;
use crate::model::{CostSnapshot, DailyCost, PlanInfo, RateWindow, SpendLimit, UsageSnapshot};
use crate::provider::{ProviderContext, ProviderMetadata, UsageProvider};

/// Rate limit headers returned by OpenAI.
#[derive(Debug, Default)]
struct OpenAIRateLimitHeaders {
    limit_requests: Option<u64>,
    remaining_requests: Option<u64>,
    limit_tokens: Option<u64>,
    remaining_tokens: Option<u64>,
}

impl OpenAIRateLimitHeaders {
    fn from_headers(headers: &reqwest::header::HeaderMap) -> Self {
        let parse_u64 = |name: &str| -> Option<u64> {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse().ok())
        };

        Self {
            limit_requests: parse_u64("x-ratelimit-limit-requests"),
            remaining_requests: parse_u64("x-ratelimit-remaining-requests"),
            limit_tokens: parse_u64("x-ratelimit-limit-tokens"),
            remaining_tokens: parse_u64("x-ratelimit-remaining-tokens"),
        }
    }
}

/// OpenAI cost API response structure.
#[derive(serde::Deserialize, Debug)]
struct OpenAICostResponse {
    data: Vec<OpenAICostItem>,
}

#[derive(serde::Deserialize, Debug)]
struct OpenAICostItem {
    amount: OpenAICostAmount,
    #[allow(dead_code)]
    line_item: Option<String>,
}

#[derive(serde::Deserialize, Debug)]
struct OpenAICostAmount {
    value: f64,
    #[allow(dead_code)]
    currency: String,
}

/// OpenAI usage API response structure.
#[derive(serde::Deserialize, Debug)]
struct OpenAIUsageResponse {
    data: Vec<OpenAIUsageItem>,
}

#[derive(serde::Deserialize, Debug)]
struct OpenAIUsageItem {
    #[allow(dead_code)]
    model: Option<String>,
    #[allow(dead_code)]
    num_requests: Option<u64>,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    #[allow(dead_code)]
    cached_input_tokens: Option<u64>,
}

/// OpenAI provider.
pub struct OpenAIProvider {
    metadata: ProviderMetadata,
    /// Base URL override for tests.
    base_url: Option<String>,
}

impl OpenAIProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: "openai",
                name: "OpenAI",
                description: "OpenAI API usage monitor",
                auth_methods: &["api_key", "env"],
                website: Some("https://platform.openai.com"),
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

    fn api_base(&self) -> &str {
        self.base_url.as_deref().unwrap_or("https://api.openai.com")
    }

    /// Extracts the API key from context or environment variable.
    fn resolve_api_key(ctx: &ProviderContext) -> Result<String, SpendPanelError> {
        if let Some(key) = ctx.config.get("api_key") {
            if !key.is_empty() {
                return Ok(key.clone());
            }
        }
        if let Ok(key) = std::env::var("OPENAI_API_KEY") {
            if !key.is_empty() {
                return Ok(key);
            }
        }
        Err(SpendPanelError::AuthFailed(
            "openai".into(),
            "no API key found in config or OPENAI_API_KEY env var".into(),
        ))
    }

    /// Fetches rate limits (available in response headers).
    async fn fetch_rate_limits(
        base_url: &str,
        client: &reqwest::Client,
        api_key: &str,
    ) -> Result<OpenAIRateLimitHeaders, SpendPanelError> {
        // Light request just to capture rate limit headers
        let resp = client
            .get(format!("{}/v1/models", base_url))
            .header("Authorization", format!("Bearer {}", api_key))
            .send()
            .await
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;

        if resp.status().is_success() || resp.status().as_u16() == 429 {
            Ok(OpenAIRateLimitHeaders::from_headers(resp.headers()))
        } else if resp.status().as_u16() == 401 {
            Err(SpendPanelError::AuthFailed(
                "openai".into(),
                "invalid API key".into(),
            ))
        } else {
            Err(SpendPanelError::ProviderError(
                "openai".into(),
                format!("unexpected status: {}", resp.status()),
            ))
        }
    }

    /// Fetches organization costs (requires admin key; failures become an empty list).
    async fn fetch_costs(
        base_url: &str,
        client: &reqwest::Client,
        api_key: &str,
    ) -> Result<Vec<DailyCost>, SpendPanelError> {
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let week_ago = (chrono::Utc::now() - chrono::Duration::days(7))
            .format("%Y-%m-%d")
            .to_string();

        let resp = client
            .get(format!("{}/v1/organization/costs", base_url))
            .query(&[("start_time", &*week_ago), ("end_time", &*today)])
            .header("Authorization", format!("Bearer {}", api_key))
            .send()
            .await
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;

        if !resp.status().is_success() {
            // Admin key required for costs; fail silently
            return Ok(Vec::new());
        }

        let body = resp
            .text()
            .await
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;

        // Try to parse, but fail silently if not admin
        if let Ok(cost_resp) = serde_json::from_str::<OpenAICostResponse>(&body) {
            // Group by date (simplified: API does not return per-item dates yet)
            let mut daily = std::collections::HashMap::new();
            for item in cost_resp.data {
                let date = chrono::Utc::now().date_naive();
                let entry = daily.entry(date).or_insert(DailyCost {
                    date,
                    cost: 0.0,
                    tokens_input: None,
                    tokens_output: None,
                    requests: None,
                });
                entry.cost += item.amount.value;
            }
            Ok(daily.into_values().collect())
        } else {
            Ok(Vec::new())
        }
    }

    /// Fetches token usage (requires admin key; failures become an empty list).
    async fn fetch_usage_report(
        base_url: &str,
        client: &reqwest::Client,
        api_key: &str,
    ) -> Result<Vec<DailyCost>, SpendPanelError> {
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let week_ago = (chrono::Utc::now() - chrono::Duration::days(7))
            .format("%Y-%m-%d")
            .to_string();

        let resp = client
            .get(format!("{}/v1/organization/usage/completions", base_url))
            .query(&[
                ("start_time", &*week_ago),
                ("end_time", &*today),
                ("bucket_width", "1d"),
            ])
            .header("Authorization", format!("Bearer {}", api_key))
            .send()
            .await
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;

        if !resp.status().is_success() {
            return Ok(Vec::new());
        }

        let body = resp
            .text()
            .await
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;

        if let Ok(usage_resp) = serde_json::from_str::<OpenAIUsageResponse>(&body) {
            // Group by date
            let mut daily = std::collections::HashMap::new();
            let today_naive = chrono::Utc::now().date_naive();
            for item in usage_resp.data {
                let date = today_naive;
                let entry = daily.entry(date).or_insert(DailyCost {
                    date,
                    cost: 0.0,
                    tokens_input: None,
                    tokens_output: None,
                    requests: None,
                });
                entry.tokens_input =
                    Some(entry.tokens_input.unwrap_or(0) + item.input_tokens.unwrap_or(0));
                entry.tokens_output =
                    Some(entry.tokens_output.unwrap_or(0) + item.output_tokens.unwrap_or(0));
            }
            Ok(daily.into_values().collect())
        } else {
            Ok(Vec::new())
        }
    }
}

impl Default for OpenAIProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl UsageProvider for OpenAIProvider {
    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &ProviderContext) -> Result<UsageSnapshot, SpendPanelError> {
        let api_key = Self::resolve_api_key(ctx)?;
        let base = self.api_base();

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(ctx.timeout_secs))
            .build()
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;

        // Fetch rate limits, costs, and usage in parallel
        let (rate_limits, costs, usage) = tokio::join!(
            Self::fetch_rate_limits(base, &client, &api_key),
            Self::fetch_costs(base, &client, &api_key),
            Self::fetch_usage_report(base, &client, &api_key),
        );

        let mut snapshot = UsageSnapshot::new("openai");
        snapshot.collected_at = Utc::now();

        // Rate limits
        if let Ok(rl) = rate_limits {
            if let (Some(limit), Some(remaining)) = (rl.limit_requests, rl.remaining_requests) {
                let used = limit.saturating_sub(remaining);
                snapshot.primary_rate_window = Some(RateWindow::new(used, limit, "RPM", 1));
            }
            if let (Some(limit), Some(remaining)) = (rl.limit_tokens, rl.remaining_tokens) {
                let used = limit.saturating_sub(remaining);
                snapshot.secondary_rate_window = Some(RateWindow::new(used, limit, "TPM", 1));
            }
        }

        // Costs (with usage report tokens merged by date)
        if let Ok(mut cost_list) = costs {
            if let Ok(usage_list) = usage {
                for u in usage_list {
                    if let Some(entry) = cost_list.iter_mut().find(|c| c.date == u.date) {
                        entry.tokens_input = u.tokens_input;
                        entry.tokens_output = u.tokens_output;
                    } else {
                        cost_list.push(u);
                    }
                }
            }

            if !cost_list.is_empty() {
                let total_cost: f64 = cost_list.iter().map(|c| c.cost).sum();

                snapshot.cost = Some(CostSnapshot {
                    total_cost: Some(total_cost),
                    currency: "USD".into(),
                    daily_costs: cost_list,
                    spend_limit: Some(SpendLimit {
                        limit: 100.0, // Tier 2 default
                        used: total_cost,
                        period: "monthly".into(),
                    }),
                });

                snapshot.plan = Some(PlanInfo {
                    name: "API".into(),
                    tier: None,
                    features: vec![],
                    price: None,
                    currency: Some("USD".into()),
                    billing_period: Some("monthly".into()),
                });
            }
        }

        Ok(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Builds mocked rate limit headers.
    fn rate_limit_headers() -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("x-ratelimit-limit-requests", "100".parse().unwrap());
        headers.insert("x-ratelimit-remaining-requests", "55".parse().unwrap());
        headers.insert("x-ratelimit-limit-tokens", "40000".parse().unwrap());
        headers.insert("x-ratelimit-remaining-tokens", "38800".parse().unwrap());
        headers
    }

    #[test]
    fn test_resolve_api_key_from_context() {
        let mut ctx = ProviderContext::new();
        ctx.config.insert("api_key".into(), "sk-test-123".into());
        assert_eq!(
            OpenAIProvider::resolve_api_key(&ctx).unwrap(),
            "sk-test-123"
        );
    }

    #[test]
    fn test_resolve_api_key_empty_context_fails() {
        let ctx = ProviderContext::new();
        let result = OpenAIProvider::resolve_api_key(&ctx);
        assert!(matches!(result, Err(SpendPanelError::AuthFailed(_, _))));
    }

    #[test]
    fn test_rate_limit_headers_parsing() {
        let rl = OpenAIRateLimitHeaders::from_headers(&rate_limit_headers());
        assert_eq!(rl.limit_requests, Some(100));
        assert_eq!(rl.remaining_requests, Some(55));
        assert_eq!(rl.limit_tokens, Some(40000));
        assert_eq!(rl.remaining_tokens, Some(38800));
    }

    #[test]
    fn test_rate_limit_headers_empty() {
        let headers = reqwest::header::HeaderMap::new();
        let rl = OpenAIRateLimitHeaders::from_headers(&headers);
        assert!(rl.limit_requests.is_none());
    }

    #[tokio::test]
    async fn test_fetch_rate_limits_success() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .and(header("Authorization", "Bearer sk-test"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"data": []}))
                    .insert_header("x-ratelimit-limit-requests", "100")
                    .insert_header("x-ratelimit-remaining-requests", "55"),
            )
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let rl = OpenAIProvider::fetch_rate_limits(&server.uri(), &client, "sk-test")
            .await
            .unwrap();
        assert_eq!(rl.limit_requests, Some(100));
        assert_eq!(rl.remaining_requests, Some(55));
    }

    #[tokio::test]
    async fn test_fetch_rate_limits_401() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let result = OpenAIProvider::fetch_rate_limits(&server.uri(), &client, "bad").await;
        assert!(matches!(result, Err(SpendPanelError::AuthFailed(_, _))));
    }

    #[tokio::test]
    async fn test_fetch_costs_success() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/v1/organization/costs"))
            .and(header("Authorization", "Bearer sk-test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{
                    "amount": { "value": 1.20, "currency": "usd" },
                    "line_item": "gpt-4o"
                }]
            })))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let costs = OpenAIProvider::fetch_costs(&server.uri(), &client, "sk-test")
            .await
            .unwrap();
        assert_eq!(costs.len(), 1);
        assert!((costs[0].cost - 1.20).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn test_fetch_costs_non_admin_returns_empty() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/organization/costs"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let costs = OpenAIProvider::fetch_costs(&server.uri(), &client, "non-admin")
            .await
            .unwrap();
        assert!(costs.is_empty());
    }

    #[tokio::test]
    async fn test_integrated_fetch_with_mocks() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"data": []}))
                    .insert_header("x-ratelimit-limit-requests", "100")
                    .insert_header("x-ratelimit-remaining-requests", "55")
                    .insert_header("x-ratelimit-limit-tokens", "40000")
                    .insert_header("x-ratelimit-remaining-tokens", "38800"),
            )
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/v1/organization/costs"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"amount": {"value": 2.40, "currency": "usd"}, "line_item": "gpt-4o"}]
            })))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/v1/organization/usage/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"model": "gpt-4o", "num_requests": 450, "input_tokens": 45000, "output_tokens": 12000}]
            })))
            .mount(&server)
            .await;

        let provider = OpenAIProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("api_key".into(), "sk-test".into());

        let snap = provider.fetch_usage(&ctx).await.unwrap();
        assert_eq!(snap.provider_id, "openai");

        let primary = snap.primary_rate_window.unwrap();
        assert_eq!(primary.used, Some(45)); // 100 - 55

        let secondary = snap.secondary_rate_window.unwrap();
        assert_eq!(secondary.used, Some(1200)); // 40000 - 38800

        let cost = snap.cost.unwrap();
        assert!((cost.total_cost.unwrap() - 2.40).abs() < f64::EPSILON);
        assert_eq!(cost.daily_costs[0].tokens_input, Some(45000));
    }
}
