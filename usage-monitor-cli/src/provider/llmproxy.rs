use std::collections::HashMap;

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::error::SpendPanelError;
use crate::model::{CostSnapshot, NamedRateWindow, RateWindow, RateWindowStatus, UsageSnapshot};
use crate::provider::{ProviderContext, ProviderMetadata, UsageProvider};

#[derive(Debug, serde::Deserialize)]
struct QuotaStatsResponse {
    providers: HashMap<String, ProviderStats>,
    summary: Option<Summary>,
}

#[derive(Debug, serde::Deserialize)]
struct Summary {
    total_requests: Option<u64>,
    total_tokens: Option<u64>,
    approx_cost: Option<f64>,
}

#[derive(Debug, serde::Deserialize)]
struct ProviderStats {
    credential_count: Option<u64>,
    active_count: Option<u64>,
    exhausted_count: Option<u64>,
    total_requests: Option<u64>,
    tokens: Option<TokenStats>,
    approx_cost: Option<f64>,
    quota_groups: Option<QuotaGroups>,
}

#[derive(Debug, serde::Deserialize)]
struct TokenStats {
    input_cached: Option<u64>,
    input_uncached: Option<u64>,
    output: Option<u64>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(untagged)]
enum QuotaGroups {
    List(Vec<QuotaGroup>),
    Map(HashMap<String, QuotaGroup>),
}

impl QuotaGroups {
    fn values(&self) -> Vec<&QuotaGroup> {
        match self {
            Self::List(items) => items.iter().collect(),
            Self::Map(map) => map.values().collect(),
        }
    }
}

#[derive(Debug, serde::Deserialize)]
struct QuotaGroup {
    remaining_percent: Option<f64>,
    reset_time: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct ProviderSummary {
    name: String,
    requests: u64,
    tokens: u64,
    approximate_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
struct LlmProxyUsage {
    provider_count: usize,
    credential_count: u64,
    active_credential_count: u64,
    exhausted_credential_count: u64,
    total_requests: u64,
    total_tokens: u64,
    approximate_cost_usd: Option<f64>,
    minimum_remaining_percent: Option<f64>,
    next_reset_at: Option<DateTime<Utc>>,
    top_providers: Vec<ProviderSummary>,
}

/// LLM Proxy quota-stats provider.
pub struct LlmProxyProvider {
    metadata: ProviderMetadata,
    base_url: Option<String>,
}

impl LlmProxyProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: "llmproxy",
                name: "LLM Proxy",
                description: "LLM Proxy quota-stats monitor",
                auth_methods: &["api_key", "base_url", "env"],
                website: None,
            },
            base_url: None,
        }
    }

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

    fn resolve_api_key(ctx: &ProviderContext) -> Result<String, SpendPanelError> {
        for key in ["api_key", "token"] {
            if let Some(value) = ctx.config.get(key) {
                let cleaned = Self::clean(value);
                if !cleaned.is_empty() {
                    return Ok(cleaned);
                }
            }
        }
        if let Ok(value) = std::env::var("LLM_PROXY_API_KEY") {
            let cleaned = Self::clean(&value);
            if !cleaned.is_empty() {
                return Ok(cleaned);
            }
        }
        Err(SpendPanelError::AuthFailed(
            "llmproxy".into(),
            "no API key found in config, token, or LLM_PROXY_API_KEY".into(),
        ))
    }

    fn resolve_base_url(&self, ctx: &ProviderContext) -> Result<String, SpendPanelError> {
        let value = ctx
            .config
            .get("base_url")
            .or_else(|| ctx.config.get("enterprise_host"))
            .map(String::as_str)
            .filter(|v| !v.is_empty())
            .map(Self::clean)
            .or_else(|| {
                std::env::var("LLM_PROXY_BASE_URL")
                    .ok()
                    .map(|v| Self::clean(&v))
            })
            .or_else(|| self.base_url.clone())
            .ok_or_else(|| {
                SpendPanelError::ConfigError(
                    "llmproxy requires base_url/enterprise_host or LLM_PROXY_BASE_URL".into(),
                )
            })?;

        let base = if value.starts_with("http://") || value.starts_with("https://") {
            value
        } else {
            format!("https://{}", value)
        };
        Ok(base.trim_end_matches('/').to_string())
    }

    fn quota_stats_url(base_url: &str) -> String {
        let base = base_url.trim_end_matches('/');
        if base.ends_with("/v1") {
            format!("{}/quota-stats", base)
        } else {
            format!("{}/v1/quota-stats", base)
        }
    }

    fn build_client(ctx: &ProviderContext) -> Result<reqwest::Client, SpendPanelError> {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(ctx.timeout_secs))
            .build()
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))
    }

    async fn fetch_stats(
        client: &reqwest::Client,
        url: String,
        api_key: &str,
    ) -> Result<QuotaStatsResponse, SpendPanelError> {
        let resp = client
            .get(url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Accept", "application/json")
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
                "llmproxy".into(),
                format!("invalid API key (HTTP {})", status.as_u16()),
            ));
        }
        if !status.is_success() {
            return Err(SpendPanelError::ProviderError(
                "llmproxy".into(),
                format!("HTTP {}: {}", status, body),
            ));
        }
        serde_json::from_str(&body)
            .map_err(|e| SpendPanelError::ParseError("llmproxy".into(), e.to_string()))
    }

    fn token_total(tokens: Option<&TokenStats>) -> u64 {
        tokens
            .map(|t| {
                t.input_cached.unwrap_or(0) + t.input_uncached.unwrap_or(0) + t.output.unwrap_or(0)
            })
            .unwrap_or(0)
    }

    fn parse_reset(raw: Option<&str>) -> Option<DateTime<Utc>> {
        raw.and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc))
    }

    fn parse_usage(resp: QuotaStatsResponse) -> LlmProxyUsage {
        let mut top_providers: Vec<ProviderSummary> = resp
            .providers
            .iter()
            .map(|(name, stats)| ProviderSummary {
                name: name.clone(),
                requests: stats.total_requests.unwrap_or(0),
                tokens: Self::token_total(stats.tokens.as_ref()),
                approximate_cost_usd: stats.approx_cost,
            })
            .collect();
        top_providers.sort_by(|a, b| {
            b.requests
                .cmp(&a.requests)
                .then_with(|| a.name.cmp(&b.name))
        });

        let total_requests = resp
            .summary
            .as_ref()
            .and_then(|s| s.total_requests)
            .unwrap_or_else(|| top_providers.iter().map(|p| p.requests).sum());
        let total_tokens = resp
            .summary
            .as_ref()
            .and_then(|s| s.total_tokens)
            .unwrap_or_else(|| top_providers.iter().map(|p| p.tokens).sum());
        let approximate_cost_usd =
            resp.summary
                .as_ref()
                .and_then(|s| s.approx_cost)
                .or_else(|| {
                    let sum: f64 = top_providers
                        .iter()
                        .filter_map(|p| p.approximate_cost_usd)
                        .sum();
                    (sum > 0.0).then_some(sum)
                });

        let quota_groups: Vec<&QuotaGroup> = resp
            .providers
            .values()
            .filter_map(|stats| stats.quota_groups.as_ref())
            .flat_map(QuotaGroups::values)
            .collect();
        let minimum_remaining_percent = quota_groups
            .iter()
            .filter_map(|g| g.remaining_percent)
            .min_by(|a, b| a.total_cmp(b));
        let next_reset_at = quota_groups
            .iter()
            .filter_map(|g| Self::parse_reset(g.reset_time.as_deref()))
            .min();

        LlmProxyUsage {
            provider_count: resp.providers.len(),
            credential_count: resp
                .providers
                .values()
                .map(|s| s.credential_count.unwrap_or(0))
                .sum(),
            active_credential_count: resp
                .providers
                .values()
                .map(|s| s.active_count.unwrap_or(0))
                .sum(),
            exhausted_credential_count: resp
                .providers
                .values()
                .map(|s| s.exhausted_count.unwrap_or(0))
                .sum(),
            total_requests,
            total_tokens,
            approximate_cost_usd,
            minimum_remaining_percent,
            next_reset_at,
            top_providers,
        }
    }

    fn format_int(value: u64) -> String {
        let s = value.to_string();
        let mut out = String::new();
        for (i, ch) in s.chars().rev().enumerate() {
            if i > 0 && i % 3 == 0 {
                out.push(',');
            }
            out.push(ch);
        }
        out.chars().rev().collect()
    }

    fn zero_window(label: impl Into<String>) -> RateWindow {
        RateWindow {
            label: label.into(),
            window_minutes: 0,
            usage_ratio: 0.0,
            limit: None,
            used: None,
            remaining: None,
            resets_at: None,
            status: RateWindowStatus::Normal,
        }
    }

    fn snapshot_from_usage(usage: LlmProxyUsage) -> UsageSnapshot {
        let mut snapshot = UsageSnapshot::new("llmproxy");
        if let Some(remaining) = usage.minimum_remaining_percent {
            let ratio = ((100.0 - remaining) / 100.0).clamp(0.0, 1.0);
            snapshot.primary_rate_window = Some(RateWindow {
                label: format!("Quota ({} active keys)", usage.active_credential_count),
                window_minutes: 0,
                usage_ratio: ratio,
                limit: Some(100),
                used: Some((ratio * 100.0).round() as u64),
                remaining: Some(remaining.round().max(0.0) as u64),
                resets_at: usage.next_reset_at,
                status: RateWindowStatus::from_ratio(ratio),
            });
        }
        snapshot.secondary_rate_window = Some(Self::zero_window(format!(
            "Requests {}",
            Self::format_int(usage.total_requests)
        )));
        snapshot.tertiary_rate_window = Some(Self::zero_window(format!(
            "Tokens {}",
            Self::format_int(usage.total_tokens)
        )));
        snapshot.extra_rate_windows = usage
            .top_providers
            .iter()
            .take(3)
            .map(|p| NamedRateWindow {
                id: p.name.clone(),
                label: p.name.clone(),
                window: Self::zero_window(format!(
                    "{}: {} req · {} tok{}",
                    p.name,
                    Self::format_int(p.requests),
                    Self::format_int(p.tokens),
                    p.approximate_cost_usd
                        .map(|c| format!(" · ${:.2}", c))
                        .unwrap_or_default()
                )),
            })
            .collect();
        if let Some(cost) = usage.approximate_cost_usd {
            snapshot.cost = Some(CostSnapshot {
                total_cost: Some(cost),
                currency: "USD".into(),
                daily_costs: Vec::new(),
                spend_limit: None,
            });
        }
        snapshot.plan = Some(crate::model::PlanInfo {
            name: format!(
                "{} providers, {}/{} active keys",
                usage.provider_count, usage.active_credential_count, usage.credential_count
            ),
            tier: None,
            features: vec![format!(
                "{} exhausted keys",
                usage.exhausted_credential_count
            )],
            price: None,
            currency: None,
            billing_period: None,
        });
        snapshot
    }
}

impl Default for LlmProxyProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl UsageProvider for LlmProxyProvider {
    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    fn detect_credentials(&self) -> bool {
        std::env::var("LLM_PROXY_API_KEY").is_ok_and(|v| !Self::clean(&v).is_empty())
            && std::env::var("LLM_PROXY_BASE_URL").is_ok_and(|v| !Self::clean(&v).is_empty())
    }

    async fn fetch_usage(&self, ctx: &ProviderContext) -> Result<UsageSnapshot, SpendPanelError> {
        let api_key = Self::resolve_api_key(ctx)?;
        let base_url = self.resolve_base_url(ctx)?;
        let client = Self::build_client(ctx)?;
        let stats = Self::fetch_stats(&client, Self::quota_stats_url(&base_url), &api_key).await?;
        Ok(Self::snapshot_from_usage(Self::parse_usage(stats)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const SAMPLE: &str = r#"{
      "providers": {
        "openai": {
          "credential_count": 3,
          "active_count": 2,
          "exhausted_count": 1,
          "total_requests": 120,
          "tokens": {"input_cached": 1000, "input_uncached": 2000, "output": 3000},
          "approx_cost": 12.5,
          "quota_groups": {"default": {"remaining_percent": 42, "reset_time": "2026-05-18T12:00:00Z"}}
        },
        "anthropic": {
          "credential_count": 1,
          "active_count": 1,
          "exhausted_count": 0,
          "total_requests": 40,
          "tokens": {"input_cached": 0, "input_uncached": 500, "output": 500},
          "approx_cost": 3.0,
          "quota_groups": [{"remaining_percent": 80}]
        }
      },
      "summary": {"total_requests":160, "total_tokens":7000, "approx_cost":15.5}
    }"#;

    fn parsed_sample() -> QuotaStatsResponse {
        serde_json::from_str(SAMPLE).unwrap()
    }

    #[test]
    fn test_provider_metadata() {
        let meta = LlmProxyProvider::new().metadata().clone();
        assert_eq!(meta.id, "llmproxy");
        assert_eq!(meta.name, "LLM Proxy");
    }

    #[test]
    fn test_quota_stats_url_accepts_versioned_or_root_base_urls() {
        assert_eq!(
            LlmProxyProvider::quota_stats_url("https://proxy.example.com"),
            "https://proxy.example.com/v1/quota-stats"
        );
        assert_eq!(
            LlmProxyProvider::quota_stats_url("https://proxy.example.com/v1"),
            "https://proxy.example.com/v1/quota-stats"
        );
    }

    #[test]
    fn test_parse_quota_stats_summary() {
        let usage = LlmProxyProvider::parse_usage(parsed_sample());
        assert_eq!(usage.provider_count, 2);
        assert_eq!(usage.credential_count, 4);
        assert_eq!(usage.active_credential_count, 3);
        assert_eq!(usage.exhausted_credential_count, 1);
        assert_eq!(usage.total_requests, 160);
        assert_eq!(usage.total_tokens, 7000);
        assert_eq!(usage.approximate_cost_usd, Some(15.5));
        assert_eq!(usage.minimum_remaining_percent, Some(42.0));
        assert_eq!(usage.top_providers[0].name, "openai");
    }

    #[test]
    fn test_snapshot_from_usage() {
        let snapshot =
            LlmProxyProvider::snapshot_from_usage(LlmProxyProvider::parse_usage(parsed_sample()));
        assert_eq!(
            snapshot.primary_rate_window.as_ref().unwrap().usage_ratio,
            0.58
        );
        assert_eq!(
            snapshot.secondary_rate_window.unwrap().label,
            "Requests 160"
        );
        assert_eq!(snapshot.tertiary_rate_window.unwrap().label, "Tokens 7,000");
        assert_eq!(snapshot.cost.unwrap().total_cost, Some(15.5));
        assert_eq!(snapshot.extra_rate_windows.len(), 2);
    }

    #[tokio::test]
    async fn test_fetch_usage_success() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/quota-stats"))
            .and(header("authorization", "Bearer proxy-key"))
            .and(header("accept", "application/json"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(SAMPLE, "application/json"))
            .mount(&server)
            .await;

        let provider = LlmProxyProvider::with_base_url(&server.uri());
        let snapshot = provider
            .fetch_usage(&ProviderContext::with_api_key("proxy-key"))
            .await
            .unwrap();
        assert_eq!(snapshot.primary_rate_window.unwrap().usage_ratio, 0.58);
        assert_eq!(snapshot.cost.unwrap().total_cost, Some(15.5));
    }

    #[tokio::test]
    async fn test_fetch_usage_401_is_auth_failed() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/quota-stats"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let provider = LlmProxyProvider::with_base_url(&server.uri());
        let err = provider
            .fetch_usage(&ProviderContext::with_api_key("bad"))
            .await
            .unwrap_err();
        assert!(matches!(err, SpendPanelError::AuthFailed(_, _)));
    }
}
