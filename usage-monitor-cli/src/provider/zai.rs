use async_trait::async_trait;
use chrono::Utc;

use crate::error::SpendPanelError;
use crate::model::{PlanInfo, RateWindow, UsageSnapshot};
use crate::provider::{ProviderContext, ProviderMetadata, UsageProvider};

#[derive(Debug, serde::Deserialize)]
struct ZaiResponse {
    #[serde(default)]
    code: i64,
    #[serde(default)]
    msg: String,
    #[serde(default)]
    success: bool,
    #[serde(default)]
    data: Option<ZaiData>,
}

#[derive(Debug, serde::Deserialize)]
struct ZaiData {
    #[serde(default)]
    limits: Vec<ZaiLimit>,
    #[serde(
        default,
        rename = "planName",
        alias = "plan",
        alias = "plan_type",
        alias = "packageName"
    )]
    plan_name: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct ZaiLimit {
    #[serde(rename = "type")]
    limit_type: String,
    #[serde(default)]
    unit: i64,
    #[serde(default)]
    number: i64,
    #[serde(default)]
    percentage: f64,
    /// Quota size (z.ai reports the limit under `usage`).
    #[serde(default)]
    usage: Option<i64>,
    #[serde(default, rename = "currentValue")]
    current_value: Option<i64>,
    #[serde(default)]
    remaining: Option<i64>,
    #[serde(default, rename = "nextResetTime")]
    next_reset_time: Option<i64>,
}

impl ZaiLimit {
    /// Window length in minutes from unit code (1=days,3=hours,5=minutes,6=weeks).
    fn window_minutes(&self) -> u32 {
        let unit_minutes = match self.unit {
            1 => 24 * 60,
            3 => 60,
            5 => 1,
            6 => 7 * 24 * 60,
            _ => 0,
        };
        (self.number.max(0) as u32).saturating_mul(unit_minutes)
    }

    /// Used percent: computed from the raw quota when present (`usage` is the
    /// limit), else the server-provided `percentage`. Mirrors CodexBar.
    fn used_percent(&self) -> f64 {
        if let Some(limit) = self.usage.filter(|l| *l > 0) {
            let used = match (self.remaining, self.current_value) {
                (Some(remaining), Some(current)) => (limit - remaining).max(current),
                (Some(remaining), None) => limit - remaining,
                (None, Some(current)) => current,
                (None, None) => return self.percentage.clamp(0.0, 100.0),
            };
            return ((used.max(0) as f64) / limit as f64 * 100.0).clamp(0.0, 100.0);
        }
        self.percentage.clamp(0.0, 100.0)
    }

    fn to_window(&self, label: &str) -> RateWindow {
        let used = self.used_percent().round() as u64;
        let mut w = RateWindow::new(used, 100, label.to_string(), self.window_minutes());
        w.resets_at = self
            .next_reset_time
            .and_then(|ms| chrono::TimeZone::timestamp_opt(&Utc, ms / 1000, 0).single());
        w
    }
}

/// z.ai coding-plan quota provider (API-key auth).
pub struct ZaiProvider {
    metadata: ProviderMetadata,
    base_url: Option<String>,
}

impl ZaiProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: "zai",
                name: "z.ai",
                description: "z.ai coding-plan quota monitor",
                auth_methods: &["api_key", "env"],
                website: Some("https://z.ai"),
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

    /// Quota endpoint: explicit base_url/host config or env, else global default.
    fn quota_url(&self, ctx: &ProviderContext) -> String {
        if let Some(base) = self.base_url.as_deref() {
            return format!(
                "{}/api/monitor/usage/quota/limit",
                base.trim_end_matches('/')
            );
        }
        let host = ctx
            .config
            .get("base_url")
            .or_else(|| ctx.config.get("host"))
            .map(|s| Self::clean(s))
            .filter(|s| !s.is_empty())
            .or_else(|| {
                std::env::var("Z_AI_API_HOST")
                    .ok()
                    .filter(|s| !s.is_empty())
            })
            .unwrap_or_else(|| "https://api.z.ai".to_string());
        let host = if host.starts_with("http") {
            host
        } else {
            format!("https://{}", host)
        };
        format!(
            "{}/api/monitor/usage/quota/limit",
            host.trim_end_matches('/')
        )
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
        if let Ok(v) = std::env::var("Z_AI_API_KEY") {
            let c = Self::clean(&v);
            if !c.is_empty() {
                return Ok(c);
            }
        }
        Err(SpendPanelError::AuthFailed(
            "zai".into(),
            "no API key in api_key/token config or Z_AI_API_KEY".into(),
        ))
    }

    fn build_client(ctx: &ProviderContext) -> Result<reqwest::Client, SpendPanelError> {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(ctx.timeout_secs))
            .build()
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))
    }

    fn parse(body: &str) -> Result<UsageSnapshot, SpendPanelError> {
        let resp: ZaiResponse = serde_json::from_str(body)
            .map_err(|e| SpendPanelError::ParseError("zai".into(), e.to_string()))?;
        if !(resp.success && resp.code == 200) {
            return Err(SpendPanelError::ProviderError(
                "zai".into(),
                format!("API error (code {}): {}", resp.code, resp.msg),
            ));
        }
        let data = resp.data.ok_or_else(|| {
            SpendPanelError::ParseError("zai".into(), "missing data in response".into())
        })?;

        let mut token_limits: Vec<&ZaiLimit> = data
            .limits
            .iter()
            .filter(|l| l.limit_type == "TOKENS_LIMIT")
            .collect();
        let time_limit = data.limits.iter().find(|l| l.limit_type == "TIME_LIMIT");

        let mut snapshot = UsageSnapshot::new("zai");

        // Multiple token limits: shortest window → tertiary (session), longest → primary.
        token_limits.sort_by_key(|l| l.window_minutes());
        if token_limits.len() >= 2 {
            snapshot.tertiary_rate_window =
                Some(token_limits.first().unwrap().to_window("Session tokens"));
            snapshot.primary_rate_window = Some(token_limits.last().unwrap().to_window("Tokens"));
        } else if let Some(only) = token_limits.first() {
            snapshot.primary_rate_window = Some(only.to_window("Tokens"));
        }

        if let Some(time) = time_limit {
            snapshot.secondary_rate_window = Some(time.to_window("Prompts"));
        }

        if snapshot.primary_rate_window.is_none() && snapshot.secondary_rate_window.is_none() {
            return Err(SpendPanelError::ParseError(
                "zai".into(),
                "no usable limits in response".into(),
            ));
        }

        if let Some(plan) = data.plan_name.filter(|s| !s.is_empty()) {
            snapshot.plan = Some(PlanInfo {
                name: plan,
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

impl Default for ZaiProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl UsageProvider for ZaiProvider {
    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    fn detect_credentials(&self) -> bool {
        std::env::var("Z_AI_API_KEY")
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false)
    }

    async fn fetch_usage(&self, ctx: &ProviderContext) -> Result<UsageSnapshot, SpendPanelError> {
        let key = Self::resolve_key(ctx)?;
        let client = Self::build_client(ctx)?;
        let resp = client
            .get(self.quota_url(ctx))
            .header("authorization", format!("Bearer {}", key))
            .header("accept", "application/json")
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
                "zai".into(),
                format!("invalid API key (HTTP {})", status.as_u16()),
            ));
        }
        if !status.is_success() {
            return Err(SpendPanelError::ProviderError(
                "zai".into(),
                format!("HTTP {}: {}", status, body),
            ));
        }
        if body.trim().is_empty() {
            return Err(SpendPanelError::ParseError(
                "zai".into(),
                "empty response (check region: Global vs BigModel CN)".into(),
            ));
        }
        Self::parse(&body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const SAMPLE: &str = r#"{
      "code": 200, "msg": "ok", "success": true,
      "data": {
        "planName": "Coding Pro",
        "limits": [
          {"type": "TOKENS_LIMIT", "unit": 6, "number": 1, "percentage": 30, "nextResetTime": 1788000000000},
          {"type": "TOKENS_LIMIT", "unit": 3, "number": 5, "percentage": 70},
          {"type": "TIME_LIMIT", "unit": 3, "number": 5, "percentage": 10}
        ]
      }
    }"#;

    #[test]
    fn test_metadata() {
        assert_eq!(ZaiProvider::new().metadata().id, "zai");
    }

    #[test]
    fn test_parse_token_window_split() {
        let snap = ZaiProvider::parse(SAMPLE).unwrap();
        // shortest window (5h) → tertiary, longest (1 week) → primary
        assert_eq!(snap.primary_rate_window.unwrap().used, Some(30));
        assert_eq!(snap.tertiary_rate_window.unwrap().used, Some(70));
        assert_eq!(snap.secondary_rate_window.unwrap().used, Some(10));
        assert_eq!(snap.plan.unwrap().name, "Coding Pro");
    }

    #[test]
    fn test_used_percent_computed_from_raw_quota() {
        // usage = limit (200), remaining 50 → used 150 → 75%.
        let body = r#"{
          "code": 200, "msg": "ok", "success": true,
          "data": {"limits": [
            {"type": "TOKENS_LIMIT", "unit": 6, "number": 1, "percentage": 10,
             "usage": 200, "remaining": 50}
          ]}
        }"#;
        let snap = ZaiProvider::parse(body).unwrap();
        // computed (75) wins over the server `percentage` (10).
        assert_eq!(snap.primary_rate_window.unwrap().used, Some(75));
    }

    #[test]
    fn test_used_percent_falls_back_to_percentage() {
        // No raw quota fields → use the server-provided percentage.
        let body = r#"{
          "code": 200, "msg": "ok", "success": true,
          "data": {"limits": [
            {"type": "TOKENS_LIMIT", "unit": 6, "number": 1, "percentage": 42}
          ]}
        }"#;
        let snap = ZaiProvider::parse(body).unwrap();
        assert_eq!(snap.primary_rate_window.unwrap().used, Some(42));
    }

    #[test]
    fn test_api_error() {
        let body = r#"{"code": 401, "msg": "bad token", "success": false}"#;
        assert!(matches!(
            ZaiProvider::parse(body).unwrap_err(),
            SpendPanelError::ProviderError(_, _)
        ));
    }

    #[tokio::test]
    async fn test_fetch_success() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/monitor/usage/quota/limit"))
            .and(header("authorization", "Bearer z"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(SAMPLE, "application/json"))
            .mount(&server)
            .await;
        let provider = ZaiProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("api_key".into(), "z".into());
        let snap = provider.fetch_usage(&ctx).await.unwrap();
        assert_eq!(snap.primary_rate_window.unwrap().used, Some(30));
    }

    #[tokio::test]
    async fn test_fetch_401() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/monitor/usage/quota/limit"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let provider = ZaiProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("api_key".into(), "bad".into());
        assert!(matches!(
            provider.fetch_usage(&ctx).await.unwrap_err(),
            SpendPanelError::AuthFailed(_, _)
        ));
    }
}
