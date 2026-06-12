use async_trait::async_trait;

use crate::error::SpendPanelError;
use crate::model::{
    CostSnapshot, DailyCost, PlanInfo, RateWindow, RateWindowStatus, SpendLimit,
    UsageSnapshot,
};
use crate::provider::{ProviderContext, ProviderMetadata, UsageProvider};

// ---------------------------------------------------------------------------
// Helper types for deserializing Anthropic responses
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize, Debug)]
struct UsageReportItem {
    date: Option<String>,
    #[allow(dead_code)]
    model: Option<String>,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

#[derive(serde::Deserialize, Debug)]
struct UsageReportResponse {
    data: Vec<UsageReportItem>,
}

#[derive(serde::Deserialize, Debug)]
struct CostReportItem {
    date: Option<String>,
    cost: Option<CostValue>,
}

#[derive(serde::Deserialize, Debug)]
struct CostValue {
    value: String,
    #[allow(dead_code)]
    currency: String,
}

#[derive(serde::Deserialize, Debug)]
struct CostReportResponse {
    data: Vec<CostReportItem>,
}

// ---------------------------------------------------------------------------
// Provider
// ---------------------------------------------------------------------------

pub struct AnthropicProvider {
    metadata: ProviderMetadata,
    /// Base URL override for tests.
    base_url: Option<String>,
}

impl AnthropicProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: "anthropic",
                name: "Anthropic (Claude)",
                description: "Anthropic Claude API usage monitor",
                auth_methods: &["api_key", "oauth", "cli"],
                website: Some("https://docs.anthropic.com"),
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
        self.base_url.as_deref().unwrap_or("https://api.anthropic.com")
    }

    fn resolve_api_key(ctx: &ProviderContext) -> Result<String, SpendPanelError> {
        if let Some(key) = ctx.config.get("api_key") {
            if !key.is_empty() {
                return Ok(key.clone());
            }
        }
        if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
            if !key.is_empty() {
                return Ok(key);
            }
        }
        Err(SpendPanelError::AuthFailed(
            "anthropic".into(),
            "no API key in config or ANTHROPIC_API_KEY env var".into(),
        ))
    }

    fn build_client(ctx: &ProviderContext) -> Result<reqwest::Client, SpendPanelError> {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(ctx.timeout_secs))
            .build()
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))
    }

    /// Fetch /v1/organizations/usage_report/messages
    async fn fetch_usage_report(
        base_url: &str,
        client: &reqwest::Client,
        api_key: &str,
    ) -> Result<Vec<UsageReportItem>, SpendPanelError> {
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let week_ago = (chrono::Utc::now() - chrono::Duration::days(7))
            .format("%Y-%m-%d")
            .to_string();

        let url = format!("{}/v1/organizations/usage_report/messages", base_url);
        let resp = client
            .get(&url)
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .query(&[("start_date", &*week_ago), ("end_date", &*today), ("bucket_width", "1d")])
            .send()
            .await
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;

        let status = resp.status();
        let body = resp.text().await.map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;

        if status == 401 {
            return Err(SpendPanelError::AuthFailed("anthropic".into(), "invalid API key".into()));
        }
        if !status.is_success() {
            return Err(SpendPanelError::ProviderError(
                "anthropic".into(),
                format!("HTTP {}: {}", status, body),
            ));
        }

        let report: UsageReportResponse = serde_json::from_str(&body)
            .map_err(|e| SpendPanelError::ParseError("anthropic".into(), format!("usage report: {}", e)))?;
        Ok(report.data)
    }

    /// Fetch /v1/organizations/cost_report
    async fn fetch_cost_report(
        base_url: &str,
        client: &reqwest::Client,
        api_key: &str,
    ) -> Result<Vec<CostReportItem>, SpendPanelError> {
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let week_ago = (chrono::Utc::now() - chrono::Duration::days(7))
            .format("%Y-%m-%d")
            .to_string();

        let url = format!("{}/v1/organizations/cost_report", base_url);
        let resp = client
            .get(&url)
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .query(&[("start_date", &*week_ago), ("end_date", &*today)])
            .send()
            .await
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;

        if !resp.status().is_success() {
            return Ok(Vec::new());
        }

        let body = resp.text().await.map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;
        let report: CostReportResponse = serde_json::from_str(&body)
            .map_err(|e| SpendPanelError::ParseError("anthropic".into(), format!("cost report: {}", e)))?;
        Ok(report.data)
    }

    /// Probes POST /v1/messages to capture rate limit headers.
    async fn probe_rate_limits(
        base_url: &str,
        client: &reqwest::Client,
        api_key: &str,
    ) -> Result<Option<RateWindow>, SpendPanelError> {
        let url = format!("{}/v1/messages", base_url);
        let resp = client
            .post(&url)
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .body(r#"{"model":"claude-sonnet-4-20250514","max_tokens":1,"messages":[{"role":"user","content":"hi"}]}"#)
            .send()
            .await
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;

        let headers = resp.headers();

        if let (Some(limit_str), Some(rem_str)) = (
            headers.get("x-ratelimit-limit-requests").and_then(|v| v.to_str().ok()),
            headers.get("x-ratelimit-remaining-requests").and_then(|v| v.to_str().ok()),
        ) {
            if let (Ok(limit), Ok(remaining)) = (limit_str.parse::<u64>(), rem_str.parse::<u64>()) {
                let used = limit.saturating_sub(remaining);
                return Ok(Some(RateWindow::new(used, limit, "RPM", 1)));
            }
        }

        Ok(None)
    }
}

impl Default for AnthropicProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl UsageProvider for AnthropicProvider {
    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &ProviderContext) -> Result<UsageSnapshot, SpendPanelError> {
        let api_key = Self::resolve_api_key(ctx)?;
        let client = Self::build_client(ctx)?;
        let base = self.api_base();

        let (rate_limit_res, usage_res, cost_res) = tokio::join!(
            Self::probe_rate_limits(base, &client, &api_key),
            Self::fetch_usage_report(base, &client, &api_key),
            Self::fetch_cost_report(base, &client, &api_key),
        );

        let mut snapshot = UsageSnapshot::new("anthropic");
        snapshot.collected_at = chrono::Utc::now();
        let mut usage_ok = false;

        // Rate limit
        if let Ok(Some(rl)) = rate_limit_res {
            snapshot.primary_rate_window = Some(rl);
        }

        // Usage report
        if let Ok(ref items) = usage_res {
            usage_ok = true;
            let mut total_in = 0u64;
            let mut total_out = 0u64;
            let mut daily: std::collections::HashMap<String, (u64, u64)> = std::collections::HashMap::new();

            for item in items {
                total_in += item.input_tokens.unwrap_or(0);
                total_out += item.output_tokens.unwrap_or(0);
                let date = item.date.clone().unwrap_or_default();
                let e = daily.entry(date).or_default();
                e.0 += item.input_tokens.unwrap_or(0);
                e.1 += item.output_tokens.unwrap_or(0);
            }

            if total_in + total_out > 0 {
                snapshot.secondary_rate_window = Some(RateWindow {
                    label: "7-day Token Usage".into(),
                    window_minutes: 10080,
                    usage_ratio: 0.0,
                    limit: None,
                    used: Some(total_in + total_out),
                    remaining: None,
                    resets_at: None,
                    status: RateWindowStatus::Normal,
                });
            }

            let daily_costs: Vec<DailyCost> = daily
                .into_iter()
                .filter_map(|(ds, (inp, out))| {
                    let date = chrono::NaiveDate::parse_from_str(&ds, "%Y-%m-%d").ok()?;
                    Some(DailyCost { date, cost: 0.0, tokens_input: Some(inp), tokens_output: Some(out), requests: None })
                })
                .collect();

            if !daily_costs.is_empty() {
                snapshot.cost = Some(CostSnapshot {
                    total_cost: None,
                    currency: "USD".into(),
                    daily_costs,
                    spend_limit: None,
                });
            }
        }

        // Cost report
        if let Ok(cost_items) = cost_res {
            if !cost_items.is_empty() {
                let mut daily: Vec<DailyCost> = cost_items
                    .into_iter()
                    .filter_map(|item| {
                        let date = chrono::NaiveDate::parse_from_str(&item.date.unwrap_or_default(), "%Y-%m-%d").ok()?;
                        let cost = item.cost.as_ref().and_then(|c| c.value.parse::<f64>().ok()).unwrap_or(0.0);
                        Some(DailyCost { date, cost, tokens_input: None, tokens_output: None, requests: None })
                    })
                    .collect();
                daily.sort_by(|a, b| a.date.cmp(&b.date));

                let total: f64 = daily.iter().map(|d| d.cost).sum();
                snapshot.cost = Some(CostSnapshot {
                    total_cost: Some(total),
                    currency: "USD".into(),
                    daily_costs: daily,
                    spend_limit: Some(SpendLimit { limit: 50.0, used: total, period: "monthly".into() }),
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

        if snapshot.plan.is_none() && usage_ok {
            snapshot.plan = Some(PlanInfo {
                name: "API".into(),
                tier: None,
                features: vec![],
                price: None,
                currency: Some("USD".into()),
                billing_period: Some("monthly".into()),
            });
        }

        Ok(snapshot)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn test_resolve_api_key_from_context() {
        let mut ctx = ProviderContext::new();
        ctx.config.insert("api_key".into(), "sk-ant-test".into());
        assert_eq!(
            AnthropicProvider::resolve_api_key(&ctx).unwrap(),
            "sk-ant-test"
        );
    }

    #[test]
    fn test_resolve_api_key_missing_is_error() {
        let ctx = ProviderContext::new();
        assert!(AnthropicProvider::resolve_api_key(&ctx).is_err());
    }

    #[test]
    fn test_provider_metadata() {
        let p = AnthropicProvider::new();
        let m = p.metadata();
        assert_eq!(m.id, "anthropic");
        assert!(m.auth_methods.contains(&"api_key"));
    }

    #[tokio::test]
    async fn test_fetch_usage_report_success() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/v1/organizations/usage_report/messages"))
            .and(header("x-api-key", "sk-ant-test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [
                    {"date": "2026-06-12", "model": "claude-sonnet-4", "input_tokens": 250000, "output_tokens": 75000},
                    {"date": "2026-06-11", "model": "claude-sonnet-4", "input_tokens": 120000, "output_tokens": 35000},
                ]
            })))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let items = AnthropicProvider::fetch_usage_report(&server.uri(), &client, "sk-ant-test").await;
        assert!(items.is_ok());
        assert_eq!(items.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn test_fetch_usage_report_401() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/organizations/usage_report/messages"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let result = AnthropicProvider::fetch_usage_report(&server.uri(), &client, "bad").await;
        assert!(matches!(result, Err(SpendPanelError::AuthFailed(_, _))));
    }

    #[tokio::test]
    async fn test_probe_rate_limits_parses_headers() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"id":"m","type":"message","content":[]}))
                    .insert_header("x-ratelimit-limit-requests", "100")
                    .insert_header("x-ratelimit-remaining-requests", "72"),
            )
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let result = AnthropicProvider::probe_rate_limits(&server.uri(), &client, "sk-ant-test").await;
        assert!(result.is_ok());
        let rl = result.unwrap().unwrap();
        assert_eq!(rl.used, Some(28));
        assert!((rl.usage_ratio - 0.28).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn test_probe_rate_limits_no_headers() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id":"m","type":"message","content":[]})))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let result = AnthropicProvider::probe_rate_limits(&server.uri(), &client, "key").await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_fetch_cost_report_success() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/v1/organizations/cost_report"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"date": "2026-06-12", "cost": {"value": "3.50", "currency": "USD"}}]
            })))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let items = AnthropicProvider::fetch_cost_report(&server.uri(), &client, "sk-ant-admin").await;
        assert!(items.is_ok());
        assert_eq!(items.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_fetch_cost_report_non_admin_returns_empty() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/organizations/cost_report"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let items = AnthropicProvider::fetch_cost_report(&server.uri(), &client, "non-admin").await;
        assert!(items.is_ok());
        assert!(items.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_integrated_fetch_with_mocks() {
        let server = MockServer::start().await;

        // Mock usage report
        Mock::given(method("GET"))
            .and(path("/v1/organizations/usage_report/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"date": "2026-06-12", "model": "claude-sonnet-4", "input_tokens": 100000, "output_tokens": 30000}]
            })))
            .mount(&server)
            .await;

        // Mock probe
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"id":"m","type":"message","content":[]}))
                    .insert_header("x-ratelimit-limit-requests", "50")
                    .insert_header("x-ratelimit-remaining-requests", "40"),
            )
            .mount(&server)
            .await;

        // Mock cost report
        Mock::given(method("GET"))
            .and(path("/v1/organizations/cost_report"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"date": "2026-06-12", "cost": {"value": "2.50", "currency": "USD"}}]
            })))
            .mount(&server)
            .await;

        let provider = AnthropicProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("api_key".into(), "sk-ant-test".into());

        let result = provider.fetch_usage(&ctx).await;
        assert!(result.is_ok());
        let snap = result.unwrap();

        assert_eq!(snap.provider_id, "anthropic");
        assert!(snap.primary_rate_window.is_some());
        assert_eq!(snap.primary_rate_window.unwrap().used, Some(10)); // 50-40

        assert!(snap.cost.is_some());
        let cost = snap.cost.unwrap();
        assert!(cost.total_cost.is_some());
        assert!((cost.total_cost.unwrap() - 2.50).abs() < f64::EPSILON);
    }
}
