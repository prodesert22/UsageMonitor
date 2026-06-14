use async_trait::async_trait;

use crate::error::SpendPanelError;
use crate::model::{PlanInfo, RateWindow, UsageSnapshot};
use crate::provider::{ProviderContext, ProviderMetadata, UsageProvider};

#[derive(Debug, serde::Deserialize)]
struct MiniMaxResponse {
    #[serde(default)]
    data: Option<MiniMaxData>,
    #[serde(default, rename = "base_resp")]
    base_resp: Option<MiniMaxBaseResp>,
}

#[derive(Debug, serde::Deserialize)]
struct MiniMaxData {
    #[serde(default, rename = "base_resp")]
    base_resp: Option<MiniMaxBaseResp>,
    #[serde(default, rename = "model_remains")]
    model_remains: Vec<MiniMaxModelRemains>,
}

#[derive(Debug, serde::Deserialize)]
struct MiniMaxBaseResp {
    #[serde(default, rename = "status_code")]
    status_code: i64,
    #[serde(default, rename = "status_msg")]
    status_msg: String,
}

#[derive(Debug, serde::Deserialize)]
struct MiniMaxModelRemains {
    #[serde(default, rename = "model_name")]
    model_name: Option<String>,
    #[serde(default, rename = "current_interval_remaining_percent")]
    interval_remaining: Option<f64>,
    #[serde(default, rename = "current_weekly_remaining_percent")]
    weekly_remaining: Option<f64>,
}

/// `current_*_remaining_percent` is already 0–100 (per CodexBar), so used is
/// simply `100 − remaining`.
fn used_from_remaining(remaining: f64) -> u64 {
    (100.0 - remaining).clamp(0.0, 100.0).round() as u64
}

/// MiniMax coding/token-plan quota provider (API-key auth).
pub struct MiniMaxProvider {
    metadata: ProviderMetadata,
    base_url: Option<String>,
}

impl MiniMaxProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: "minimax",
                name: "MiniMax",
                description: "MiniMax coding/token-plan quota monitor",
                auth_methods: &["api_key", "env"],
                website: Some("https://www.minimax.io"),
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
        let mut v = raw.trim();
        if v.len() >= 2
            && ((v.starts_with('"') && v.ends_with('"'))
                || (v.starts_with('\'') && v.ends_with('\'')))
        {
            v = &v[1..v.len() - 1];
        }
        v.trim().to_string()
    }

    /// Candidate remains endpoints (token-plan first, then coding-plan).
    fn endpoints(&self, ctx: &ProviderContext) -> Vec<String> {
        let base = self
            .base_url
            .clone()
            .or_else(|| ctx.config.get("base_url").map(|s| Self::clean(s)).filter(|s| !s.is_empty()))
            .unwrap_or_else(|| "https://api.minimax.io".to_string());
        let base = base.trim_end_matches('/');
        vec![
            format!("{}/v1/token_plan/remains", base),
            format!("{}/v1/api/openplatform/coding_plan/remains", base),
        ]
    }

    fn resolve_key(ctx: &ProviderContext) -> Result<String, SpendPanelError> {
        for key in ["api_key", "token"] {
            if let Some(v) = ctx.config.get(key) {
                let c = Self::clean(v);
                if !c.is_empty() {
                    return Ok(c);
                }
            }
        }
        for env in ["MINIMAX_CODING_API_KEY", "MINIMAX_API_KEY"] {
            if let Ok(v) = std::env::var(env) {
                let c = Self::clean(&v);
                if !c.is_empty() {
                    return Ok(c);
                }
            }
        }
        Err(SpendPanelError::AuthFailed(
            "minimax".into(),
            "no API key in api_key/token config, MINIMAX_CODING_API_KEY, or MINIMAX_API_KEY".into(),
        ))
    }

    fn build_client(ctx: &ProviderContext) -> Result<reqwest::Client, SpendPanelError> {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(ctx.timeout_secs))
            .build()
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))
    }

    fn parse(body: &str) -> Result<UsageSnapshot, SpendPanelError> {
        let resp: MiniMaxResponse = serde_json::from_str(body)
            .map_err(|e| SpendPanelError::ParseError("minimax".into(), e.to_string()))?;
        let data = resp.data.ok_or_else(|| {
            SpendPanelError::ParseError("minimax".into(), "missing data in response".into())
        })?;

        let base = data.base_resp.as_ref().or(resp.base_resp.as_ref());
        if let Some(b) = base.filter(|b| b.status_code != 0) {
            let lower = b.status_msg.to_lowercase();
            if b.status_code == 1004 || lower.contains("login") || lower.contains("cookie") {
                return Err(SpendPanelError::AuthFailed("minimax".into(), b.status_msg.clone()));
            }
            return Err(SpendPanelError::ProviderError("minimax".into(), b.status_msg.clone()));
        }

        if data.model_remains.is_empty() {
            return Err(SpendPanelError::ParseError(
                "minimax".into(),
                "no model_remains in response".into(),
            ));
        }

        // Headline: the most-consumed model per window.
        let interval = data
            .model_remains
            .iter()
            .filter_map(|m| m.interval_remaining)
            .min_by(|a, b| a.total_cmp(b));
        let weekly = data
            .model_remains
            .iter()
            .filter_map(|m| m.weekly_remaining)
            .min_by(|a, b| a.total_cmp(b));

        let mut snapshot = UsageSnapshot::new("minimax");
        if let Some(r) = interval {
            snapshot.primary_rate_window =
                Some(RateWindow::new(used_from_remaining(r), 100, "Interval", 0));
        }
        if let Some(r) = weekly {
            snapshot.secondary_rate_window =
                Some(RateWindow::new(used_from_remaining(r), 100, "Weekly", 7 * 24 * 60));
        }
        if snapshot.primary_rate_window.is_none() && snapshot.secondary_rate_window.is_none() {
            return Err(SpendPanelError::ParseError(
                "minimax".into(),
                "no interval/weekly remaining percentages in response".into(),
            ));
        }

        if let Some(name) = data.model_remains.iter().find_map(|m| m.model_name.clone()) {
            snapshot.plan = Some(PlanInfo {
                name,
                tier: None,
                features: Vec::new(),
                price: None,
                currency: None,
                billing_period: None,
            });
        }
        Ok(snapshot)
    }
}

impl Default for MiniMaxProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl UsageProvider for MiniMaxProvider {
    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    fn detect_credentials(&self) -> bool {
        ["MINIMAX_CODING_API_KEY", "MINIMAX_API_KEY"]
            .iter()
            .any(|e| std::env::var(e).map(|v| !v.trim().is_empty()).unwrap_or(false))
    }

    async fn fetch_usage(&self, ctx: &ProviderContext) -> Result<UsageSnapshot, SpendPanelError> {
        let key = Self::resolve_key(ctx)?;
        let client = Self::build_client(ctx)?;
        let mut last_err = None;
        for url in self.endpoints(ctx) {
            let resp = client
                .get(&url)
                .header("Authorization", format!("Bearer {}", key))
                .header("Accept", "application/json")
                .header("MM-API-Source", "UsageMonitor")
                .send()
                .await
                .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;
            let status = resp.status();
            let body = resp
                .text()
                .await
                .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;
            if status == reqwest::StatusCode::UNAUTHORIZED
                || status == reqwest::StatusCode::FORBIDDEN
            {
                return Err(SpendPanelError::AuthFailed(
                    "minimax".into(),
                    format!("invalid API key (HTTP {})", status.as_u16()),
                ));
            }
            if !status.is_success() {
                last_err = Some(SpendPanelError::ProviderError(
                    "minimax".into(),
                    format!("HTTP {}: {}", status, body),
                ));
                continue;
            }
            match Self::parse(&body) {
                Ok(snap) => return Ok(snap),
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err.unwrap_or_else(|| {
            SpendPanelError::ProviderError("minimax".into(), "no remains endpoint succeeded".into())
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const SAMPLE: &str = r#"{
      "data": {
        "base_resp": {"status_code": 0, "status_msg": "success"},
        "model_remains": [
          {"model_name": "MiniMax-M2", "current_interval_remaining_percent": 25, "current_weekly_remaining_percent": 80}
        ]
      }
    }"#;

    #[test]
    fn test_metadata() {
        assert_eq!(MiniMaxProvider::new().metadata().id, "minimax");
    }

    #[test]
    fn test_parse_remaining_to_used() {
        let snap = MiniMaxProvider::parse(SAMPLE).unwrap();
        // 25% remaining → 75% used
        assert_eq!(snap.primary_rate_window.unwrap().used, Some(75));
        // 80% remaining → 20% used
        assert_eq!(snap.secondary_rate_window.unwrap().used, Some(20));
        assert_eq!(snap.plan.unwrap().name, "MiniMax-M2");
    }

    #[test]
    fn test_remaining_percent_not_rescaled() {
        // A remaining of 1(%) must read as 99% used, not 0% (regression: the
        // value is already a percent, not a 0–1 fraction).
        let body = r#"{"data":{"model_remains":[
          {"model_name":"M","current_interval_remaining_percent":1,"current_weekly_remaining_percent":99}
        ]}}"#;
        let snap = MiniMaxProvider::parse(body).unwrap();
        assert_eq!(snap.primary_rate_window.unwrap().used, Some(99));
        assert_eq!(snap.secondary_rate_window.unwrap().used, Some(1));
    }

    #[test]
    fn test_lowest_remaining_across_models() {
        // Two models: primary window uses the most-consumed (lowest remaining).
        let body = r#"{"data":{"model_remains":[
          {"model_name":"A","current_interval_remaining_percent":60},
          {"model_name":"B","current_interval_remaining_percent":10}
        ]}}"#;
        let snap = MiniMaxProvider::parse(body).unwrap();
        assert_eq!(snap.primary_rate_window.unwrap().used, Some(90));
    }

    #[test]
    fn test_base_resp_login_error() {
        let body = r#"{"data":{"base_resp":{"status_code":1004,"status_msg":"please login"},"model_remains":[]}}"#;
        assert!(matches!(
            MiniMaxProvider::parse(body).unwrap_err(),
            SpendPanelError::AuthFailed(_, _)
        ));
    }

    #[tokio::test]
    async fn test_fetch_success() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/token_plan/remains"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(SAMPLE, "application/json"))
            .mount(&server)
            .await;
        let provider = MiniMaxProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("api_key".into(), "mm".into());
        let snap = provider.fetch_usage(&ctx).await.unwrap();
        assert_eq!(snap.primary_rate_window.unwrap().used, Some(75));
    }

    #[tokio::test]
    async fn test_fetch_401() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/token_plan/remains"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let provider = MiniMaxProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("api_key".into(), "bad".into());
        assert!(matches!(
            provider.fetch_usage(&ctx).await.unwrap_err(),
            SpendPanelError::AuthFailed(_, _)
        ));
    }
}
