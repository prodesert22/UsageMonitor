use async_trait::async_trait;

use crate::error::SpendPanelError;
use crate::model::{RateWindow, RateWindowStatus, UsageSnapshot};
use crate::provider::{ProviderContext, ProviderMetadata, UsageProvider};

#[derive(Debug, serde::Deserialize)]
struct PrometheusResponse {
    status: String,
    data: Option<PrometheusPayload>,
    error: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct PrometheusPayload {
    result: Vec<PrometheusSeries>,
}

#[derive(Debug, serde::Deserialize)]
struct PrometheusSeries {
    value: Option<Vec<PrometheusValue>>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(untagged)]
enum PrometheusValue {
    Number(f64),
    String(String),
}

impl PrometheusValue {
    fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Number(n) => Some(*n),
            Self::String(s) => s.parse().ok(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct GroqUsage {
    request_rate_per_second: f64,
    input_token_rate_per_second: f64,
    output_token_rate_per_second: f64,
    prompt_cache_hit_rate_per_second: f64,
}

impl GroqUsage {
    fn requests_per_minute(&self) -> f64 {
        self.request_rate_per_second * 60.0
    }

    fn tokens_per_minute(&self) -> f64 {
        (self.input_token_rate_per_second + self.output_token_rate_per_second) * 60.0
    }

    fn cache_hits_per_minute(&self) -> f64 {
        self.prompt_cache_hit_rate_per_second * 60.0
    }
}

/// GroqCloud Prometheus metrics provider.
pub struct GroqProvider {
    metadata: ProviderMetadata,
    /// Base URL override for tests.
    base_url: Option<String>,
}

impl GroqProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: "groq",
                name: "GroqCloud",
                description: "GroqCloud Prometheus metrics monitor",
                auth_methods: &["api_key", "env"],
                website: Some("https://console.groq.com"),
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
        for env in ["GROQ_API_KEY", "GROQ_TOKEN"] {
            if let Ok(value) = std::env::var(env) {
                let cleaned = Self::clean(&value);
                if !cleaned.is_empty() {
                    return Ok(cleaned);
                }
            }
        }
        Err(SpendPanelError::AuthFailed(
            "groq".into(),
            "no API key found in config, token, GROQ_API_KEY, or GROQ_TOKEN".into(),
        ))
    }

    fn api_base(&self, ctx: &ProviderContext) -> String {
        let configured = ctx
            .config
            .get("api_url")
            .or_else(|| ctx.config.get("base_url"))
            .map(String::as_str)
            .filter(|v| !v.is_empty())
            .map(Self::clean)
            .or_else(|| std::env::var("GROQ_API_URL").ok().map(|v| Self::clean(&v)))
            .or_else(|| self.base_url.clone())
            .unwrap_or_else(|| "https://api.groq.com/v1".into());

        let base = if configured.starts_with("http://") || configured.starts_with("https://") {
            configured
        } else {
            format!("https://{}", configured)
        };
        format!("{}/metrics/prometheus", base.trim_end_matches('/'))
    }

    fn build_client(ctx: &ProviderContext) -> Result<reqwest::Client, SpendPanelError> {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(ctx.timeout_secs))
            .build()
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))
    }

    fn parse_scalar(body: &str) -> Result<f64, SpendPanelError> {
        let decoded: PrometheusResponse = serde_json::from_str(body)
            .map_err(|e| SpendPanelError::ParseError("groq".into(), e.to_string()))?;
        if decoded.status != "success" {
            return Err(SpendPanelError::ProviderError(
                "groq".into(),
                decoded.error.unwrap_or_else(|| "query failed".into()),
            ));
        }
        Ok(decoded
            .data
            .map(|data| {
                data.result
                    .iter()
                    .filter_map(|series| series.value.as_ref())
                    .filter_map(|values| values.last())
                    .filter_map(PrometheusValue::as_f64)
                    .sum()
            })
            .unwrap_or(0.0))
    }

    async fn query_scalar(
        client: &reqwest::Client,
        base_url: &str,
        api_key: &str,
        query: &str,
    ) -> Result<f64, SpendPanelError> {
        let resp = client
            .get(format!("{}/api/v1/query", base_url))
            .query(&[("query", query)])
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
                "groq".into(),
                format!("metrics access denied (HTTP {})", status.as_u16()),
            ));
        }
        if !status.is_success() {
            return Err(SpendPanelError::ProviderError(
                "groq".into(),
                format!("HTTP {}: {}", status, body),
            ));
        }
        Self::parse_scalar(&body)
    }

    async fn fetch_usage_data(
        &self,
        client: &reqwest::Client,
        api_key: &str,
        ctx: &ProviderContext,
    ) -> Result<GroqUsage, SpendPanelError> {
        let base_url = self.api_base(ctx);
        let (requests, input_tokens, output_tokens, cache_hits) = tokio::try_join!(
            Self::query_scalar(
                client,
                &base_url,
                api_key,
                "sum(model_project_id_status_code:requests:rate5m)"
            ),
            Self::query_scalar(
                client,
                &base_url,
                api_key,
                "sum(model_project_id:tokens_in:rate5m)"
            ),
            Self::query_scalar(
                client,
                &base_url,
                api_key,
                "sum(model_project_id:tokens_out:rate5m)"
            ),
            Self::query_scalar(
                client,
                &base_url,
                api_key,
                "sum(model_project_id:prompt_cache_hits:rate5m)"
            ),
        )?;
        Ok(GroqUsage {
            request_rate_per_second: requests,
            input_token_rate_per_second: input_tokens,
            output_token_rate_per_second: output_tokens,
            prompt_cache_hit_rate_per_second: cache_hits,
        })
    }

    fn format_decimal(value: f64) -> String {
        if value >= 100.0 {
            format!("{:.0}", value)
        } else if value >= 10.0 {
            format!("{:.1}", value)
        } else {
            format!("{:.2}", value)
        }
    }

    fn zero_window(label: impl Into<String>) -> RateWindow {
        RateWindow {
            label: label.into(),
            window_minutes: 5,
            usage_ratio: 0.0,
            limit: None,
            used: None,
            remaining: None,
            resets_at: None,
            status: RateWindowStatus::Normal,
        }
    }

    fn snapshot_from_usage(usage: GroqUsage) -> UsageSnapshot {
        let mut snapshot = UsageSnapshot::new("groq");
        snapshot.primary_rate_window = Some(Self::zero_window(format!(
            "Requests {} req/min",
            Self::format_decimal(usage.requests_per_minute())
        )));
        snapshot.secondary_rate_window = Some(Self::zero_window(format!(
            "Tokens {} tok/min",
            Self::format_decimal(usage.tokens_per_minute())
        )));
        if usage.prompt_cache_hit_rate_per_second > 0.0 {
            snapshot.tertiary_rate_window = Some(Self::zero_window(format!(
                "Cache {} cache/min",
                Self::format_decimal(usage.cache_hits_per_minute())
            )));
        }
        snapshot
    }
}

impl Default for GroqProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl UsageProvider for GroqProvider {
    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    fn detect_credentials(&self) -> bool {
        ["GROQ_API_KEY", "GROQ_TOKEN"]
            .iter()
            .any(|env| std::env::var(env).is_ok_and(|v| !Self::clean(&v).is_empty()))
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
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const SUCCESS: &str = r#"{
      "status":"success",
      "data":{"result":[{"value":[1710000000,"2.5"]},{"value":[1710000000,"1.5"]}]}
    }"#;

    #[test]
    fn test_provider_metadata() {
        let provider = GroqProvider::new();
        let meta = provider.metadata();
        assert_eq!(meta.id, "groq");
        assert_eq!(meta.name, "GroqCloud");
        assert!(meta.auth_methods.contains(&"api_key"));
    }

    #[test]
    fn test_parse_prometheus_scalar_response() {
        assert_eq!(GroqProvider::parse_scalar(SUCCESS).unwrap(), 4.0);
    }

    #[test]
    fn test_parse_prometheus_error_response() {
        let err = GroqProvider::parse_scalar(r#"{"status":"error","error":"nope"}"#).unwrap_err();
        assert!(matches!(err, SpendPanelError::ProviderError(_, _)));
    }

    #[test]
    fn test_snapshot_maps_rates_to_windows() {
        let snapshot = GroqProvider::snapshot_from_usage(GroqUsage {
            request_rate_per_second: 2.0,
            input_token_rate_per_second: 100.0,
            output_token_rate_per_second: 50.0,
            prompt_cache_hit_rate_per_second: 3.0,
        });
        assert_eq!(
            snapshot.primary_rate_window.unwrap().label,
            "Requests 120 req/min"
        );
        assert_eq!(
            snapshot.secondary_rate_window.unwrap().label,
            "Tokens 9000 tok/min"
        );
        assert_eq!(
            snapshot.tertiary_rate_window.unwrap().label,
            "Cache 180 cache/min"
        );
    }

    #[tokio::test]
    async fn test_fetch_usage_success() {
        let server = MockServer::start().await;
        for query in [
            "sum(model_project_id_status_code:requests:rate5m)",
            "sum(model_project_id:tokens_in:rate5m)",
            "sum(model_project_id:tokens_out:rate5m)",
            "sum(model_project_id:prompt_cache_hits:rate5m)",
        ] {
            Mock::given(method("GET"))
                .and(path("/v1/metrics/prometheus/api/v1/query"))
                .and(query_param("query", query))
                .and(header("authorization", "Bearer gsk-test"))
                .and(header("accept", "application/json"))
                .respond_with(ResponseTemplate::new(200).set_body_raw(SUCCESS, "application/json"))
                .mount(&server)
                .await;
        }

        let provider = GroqProvider::with_base_url(&format!("{}/v1", server.uri()));
        let snapshot = provider
            .fetch_usage(&ProviderContext::with_api_key("gsk-test"))
            .await
            .unwrap();
        assert_eq!(
            snapshot.primary_rate_window.unwrap().label,
            "Requests 240 req/min"
        );
        assert_eq!(
            snapshot.secondary_rate_window.unwrap().label,
            "Tokens 480 tok/min"
        );
        assert_eq!(
            snapshot.tertiary_rate_window.unwrap().label,
            "Cache 240 cache/min"
        );
    }

    #[tokio::test]
    async fn test_fetch_usage_401_is_auth_failed() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/metrics/prometheus/api/v1/query"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let provider = GroqProvider::with_base_url(&format!("{}/v1", server.uri()));
        let err = provider
            .fetch_usage(&ProviderContext::with_api_key("bad"))
            .await
            .unwrap_err();
        assert!(matches!(err, SpendPanelError::AuthFailed(_, _)));
    }
}
