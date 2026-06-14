use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::error::SpendPanelError;
use crate::model::{RateWindow, UsageSnapshot};
use crate::provider::{ProviderContext, ProviderMetadata, UsageProvider};

#[derive(Debug, serde::Deserialize)]
struct KimiUsageResponse {
    #[serde(default)]
    usages: Vec<KimiUsage>,
}

#[derive(Debug, serde::Deserialize)]
struct KimiUsage {
    #[serde(default)]
    scope: String,
    detail: KimiUsageDetail,
    #[serde(default)]
    limits: Option<Vec<KimiRateLimit>>,
}

#[derive(Debug, serde::Deserialize)]
struct KimiRateLimit {
    detail: KimiUsageDetail,
}

#[derive(Debug, serde::Deserialize)]
struct KimiUsageDetail {
    #[serde(default)]
    limit: String,
    #[serde(default)]
    used: Option<String>,
    #[serde(default)]
    remaining: Option<String>,
    #[serde(default, rename = "resetTime")]
    reset_time: Option<String>,
}

impl KimiUsageDetail {
    /// (used, limit) request counts.
    fn counts(&self) -> (u64, u64) {
        let limit = self.limit.parse::<i64>().unwrap_or(0).max(0) as u64;
        let used = match self.used.as_deref().and_then(|s| s.parse::<i64>().ok()) {
            Some(u) => u.max(0) as u64,
            None => {
                let remaining = self
                    .remaining
                    .as_deref()
                    .and_then(|s| s.parse::<i64>().ok())
                    .unwrap_or(0);
                limit.saturating_sub(remaining.max(0) as u64)
            }
        };
        (used, limit)
    }

    fn resets_at(&self) -> Option<DateTime<Utc>> {
        let raw = self.reset_time.as_deref()?;
        if let Ok(secs) = raw.parse::<i64>() {
            let secs = if secs > 1_000_000_000_000 { secs / 1000 } else { secs };
            return chrono::TimeZone::timestamp_opt(&Utc, secs, 0).single();
        }
        DateTime::parse_from_rfc3339(raw)
            .ok()
            .map(|d| d.with_timezone(&Utc))
    }
}

/// Kimi coding usage provider (kimi.com, JWT auth token).
pub struct KimiProvider {
    metadata: ProviderMetadata,
    base_url: Option<String>,
}

impl KimiProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: "kimi",
                name: "Kimi",
                description: "Kimi coding weekly/rate-limit usage monitor",
                auth_methods: &["token", "api_key", "env"],
                website: Some("https://www.kimi.com"),
            },
            base_url: None,
        }
    }

    pub fn with_base_url(url: &str) -> Self {
        let mut p = Self::new();
        p.base_url = Some(url.to_string());
        p
    }

    fn api_base(&self) -> &str {
        self.base_url.as_deref().unwrap_or("https://www.kimi.com")
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

    fn resolve_token(ctx: &ProviderContext) -> Result<String, SpendPanelError> {
        for key in ["token", "api_key", "cookie"] {
            if let Some(v) = ctx.config.get(key) {
                let c = Self::clean(v);
                if !c.is_empty() {
                    return Ok(c);
                }
            }
        }
        for env in ["KIMI_AUTH_TOKEN", "KIMI_API_KEY"] {
            if let Ok(v) = std::env::var(env) {
                let c = Self::clean(&v);
                if !c.is_empty() {
                    return Ok(c);
                }
            }
        }
        Err(SpendPanelError::AuthFailed(
            "kimi".into(),
            "no auth token in token/api_key config, KIMI_AUTH_TOKEN, or KIMI_API_KEY".into(),
        ))
    }

    fn build_client(ctx: &ProviderContext) -> Result<reqwest::Client, SpendPanelError> {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(ctx.timeout_secs))
            .build()
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))
    }

    fn parse(body: &str) -> Result<UsageSnapshot, SpendPanelError> {
        let resp: KimiUsageResponse = serde_json::from_str(body)
            .map_err(|e| SpendPanelError::ParseError("kimi".into(), e.to_string()))?;
        let coding = resp
            .usages
            .iter()
            .find(|u| u.scope == "FEATURE_CODING")
            .or_else(|| resp.usages.first())
            .ok_or_else(|| {
                SpendPanelError::ParseError("kimi".into(), "no usage scope in response".into())
            })?;

        let mut snapshot = UsageSnapshot::new("kimi");
        let (used, limit) = coding.detail.counts();
        let mut weekly = RateWindow::new(used, limit, "Weekly", 0);
        weekly.resets_at = coding.detail.resets_at();
        snapshot.primary_rate_window = Some(weekly);

        if let Some(rate) = coding.limits.as_ref().and_then(|l| l.first()) {
            let (rused, rlimit) = rate.detail.counts();
            let mut window = RateWindow::new(rused, rlimit, "Rate limit", 300);
            window.resets_at = rate.detail.resets_at();
            snapshot.secondary_rate_window = Some(window);
        }
        Ok(snapshot)
    }
}

impl Default for KimiProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl UsageProvider for KimiProvider {
    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    fn detect_credentials(&self) -> bool {
        ["KIMI_AUTH_TOKEN", "KIMI_API_KEY"]
            .iter()
            .any(|e| std::env::var(e).map(|v| !v.trim().is_empty()).unwrap_or(false))
    }

    async fn fetch_usage(&self, ctx: &ProviderContext) -> Result<UsageSnapshot, SpendPanelError> {
        let token = Self::resolve_token(ctx)?;
        let client = Self::build_client(ctx)?;
        let url = format!(
            "{}/apiv2/kimi.gateway.billing.v1.BillingService/GetUsages",
            self.api_base().trim_end_matches('/')
        );
        let resp = client
            .post(url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Cookie", format!("kimi-auth={}", token))
            .header("Content-Type", "application/json")
            .header("Accept", "*/*")
            .header("connect-protocol-version", "1")
            .body(r#"{"scope":["FEATURE_CODING"]}"#)
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
                "kimi".into(),
                format!("invalid auth token (HTTP {})", status.as_u16()),
            ));
        }
        if !status.is_success() {
            return Err(SpendPanelError::ProviderError(
                "kimi".into(),
                format!("HTTP {}: {}", status, body),
            ));
        }
        Self::parse(&body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const SAMPLE: &str = r#"{
      "usages": [
        {"scope": "FEATURE_CODING",
         "detail": {"limit": "1000", "used": "250", "resetTime": "1788000000"},
         "limits": [{"detail": {"limit": "100", "remaining": "40"}}]}
      ]
    }"#;

    #[test]
    fn test_metadata() {
        assert_eq!(KimiProvider::new().metadata().id, "kimi");
    }

    #[test]
    fn test_parse_weekly_and_rate() {
        let snap = KimiProvider::parse(SAMPLE).unwrap();
        let weekly = snap.primary_rate_window.unwrap();
        assert_eq!(weekly.used, Some(250));
        assert_eq!(weekly.limit, Some(1000));
        let rate = snap.secondary_rate_window.unwrap();
        // limit 100, remaining 40 → used 60
        assert_eq!(rate.used, Some(60));
        assert_eq!(rate.window_minutes, 300);
    }

    #[test]
    fn test_no_rate_limit_drops_secondary() {
        let body = r#"{"usages":[{"scope":"FEATURE_CODING",
          "detail":{"limit":"1000","remaining":"600"}}]}"#;
        let snap = KimiProvider::parse(body).unwrap();
        // used = limit - remaining = 400
        assert_eq!(snap.primary_rate_window.unwrap().used, Some(400));
        assert!(snap.secondary_rate_window.is_none());
    }

    #[test]
    fn test_falls_back_to_first_scope() {
        let body = r#"{"usages":[{"scope":"OTHER",
          "detail":{"limit":"10","used":"3"}}]}"#;
        let snap = KimiProvider::parse(body).unwrap();
        assert_eq!(snap.primary_rate_window.unwrap().used, Some(3));
    }

    #[tokio::test]
    async fn test_fetch_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/apiv2/kimi.gateway.billing.v1.BillingService/GetUsages"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(SAMPLE, "application/json"))
            .mount(&server)
            .await;
        let provider = KimiProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("token".into(), "jwt".into());
        let snap = provider.fetch_usage(&ctx).await.unwrap();
        assert_eq!(snap.primary_rate_window.unwrap().used, Some(250));
    }

    #[tokio::test]
    async fn test_fetch_401() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/apiv2/kimi.gateway.billing.v1.BillingService/GetUsages"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let provider = KimiProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("token".into(), "bad".into());
        assert!(matches!(
            provider.fetch_usage(&ctx).await.unwrap_err(),
            SpendPanelError::AuthFailed(_, _)
        ));
    }
}
