use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::error::SpendPanelError;
use crate::model::{PlanInfo, RateWindow, UsageSnapshot};
use crate::provider::{ProviderContext, ProviderMetadata, UsageProvider};

/// Devin quota usage provider (app.devin.ai, Bearer token + organization).
pub struct DevinProvider {
    metadata: ProviderMetadata,
    base_url: Option<String>,
}

impl DevinProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: "devin",
                name: "Devin",
                description: "Devin daily/weekly quota monitor",
                auth_methods: &["token", "api_key", "env"],
                website: Some("https://devin.ai"),
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
        self.base_url.as_deref().unwrap_or("https://app.devin.ai")
    }

    fn clean(raw: &str) -> String {
        let mut v = raw.trim();
        if v.len() >= 2
            && ((v.starts_with('"') && v.ends_with('"'))
                || (v.starts_with('\'') && v.ends_with('\'')))
        {
            v = &v[1..v.len() - 1];
        }
        // Allow pasting a full "Bearer <token>" or "Authorization: Bearer <token>".
        let mut s = v.trim().to_string();
        if let Some(rest) = s.strip_prefix("Authorization:") {
            s = rest.trim().to_string();
        }
        if let Some(rest) = s.strip_prefix("Bearer ") {
            s = rest.trim().to_string();
        }
        s
    }

    fn resolve_token(ctx: &ProviderContext) -> Result<String, SpendPanelError> {
        for key in ["token", "api_key"] {
            if let Some(v) = ctx.config.get(key) {
                let c = Self::clean(v);
                if !c.is_empty() {
                    return Ok(c);
                }
            }
        }
        for env in ["DEVIN_TOKEN", "DEVIN_API_TOKEN"] {
            if let Ok(v) = std::env::var(env) {
                let c = Self::clean(&v);
                if !c.is_empty() {
                    return Ok(c);
                }
            }
        }
        Err(SpendPanelError::AuthFailed(
            "devin".into(),
            "no Bearer token in token/api_key config, DEVIN_TOKEN, or DEVIN_API_TOKEN".into(),
        ))
    }

    fn resolve_org(ctx: &ProviderContext) -> Result<String, SpendPanelError> {
        for key in ["organization", "org", "organization_id"] {
            if let Some(v) = ctx
                .config
                .get(key)
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
            {
                let trimmed = v.trim_matches('/');
                return Ok(trimmed.to_string());
            }
        }
        if let Ok(v) = std::env::var("DEVIN_ORG") {
            let t = v.trim().trim_matches('/');
            if !t.is_empty() {
                return Ok(t.to_string());
            }
        }
        Err(SpendPanelError::ProviderError(
            "devin".into(),
            "no organization configured; set `devin set organization <slug>` or DEVIN_ORG".into(),
        ))
    }

    fn build_client(ctx: &ProviderContext) -> Result<reqwest::Client, SpendPanelError> {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(ctx.timeout_secs))
            .build()
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))
    }

    fn window(
        percent: Option<f64>,
        reset: Option<&serde_json::Value>,
        minutes: u32,
        label: &str,
    ) -> Option<RateWindow> {
        let p = percent?;
        let used = if p <= 1.0 { p * 100.0 } else { p };
        let mut w = RateWindow::new(
            used.clamp(0.0, 100.0).round() as u64,
            100,
            label.to_string(),
            minutes,
        );
        w.resets_at = reset.and_then(parse_reset);
        Some(w)
    }

    fn parse(body: &str) -> Result<UsageSnapshot, SpendPanelError> {
        let json: serde_json::Value = serde_json::from_str(body)
            .map_err(|e| SpendPanelError::ParseError("devin".into(), e.to_string()))?;

        let daily = Self::window(
            json.get("daily_percentage").and_then(num),
            json.get("daily_reset_at"),
            24 * 60,
            "Daily",
        );
        let weekly = Self::window(
            json.get("weekly_percentage").and_then(num),
            json.get("weekly_reset_at"),
            7 * 24 * 60,
            "Weekly",
        );

        if daily.is_none() && weekly.is_none() {
            return Err(SpendPanelError::ParseError(
                "devin".into(),
                "no daily/weekly quota in response".into(),
            ));
        }

        let mut snapshot = UsageSnapshot::new("devin");
        snapshot.primary_rate_window = daily;
        snapshot.secondary_rate_window = weekly;
        if let Some(plan) = json
            .get("plan_name")
            .or_else(|| json.get("plan"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            snapshot.plan = Some(PlanInfo {
                name: plan.to_string(),
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

fn num(v: &serde_json::Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

fn parse_reset(v: &serde_json::Value) -> Option<DateTime<Utc>> {
    if let Some(s) = v.as_str() {
        if let Ok(secs) = s.parse::<i64>() {
            let secs = if secs > 1_000_000_000_000 {
                secs / 1000
            } else {
                secs
            };
            return chrono::TimeZone::timestamp_opt(&Utc, secs, 0).single();
        }
        return DateTime::parse_from_rfc3339(s)
            .ok()
            .map(|d| d.with_timezone(&Utc));
    }
    if let Some(secs) = v.as_i64() {
        let secs = if secs > 1_000_000_000_000 {
            secs / 1000
        } else {
            secs
        };
        return chrono::TimeZone::timestamp_opt(&Utc, secs, 0).single();
    }
    None
}

impl Default for DevinProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl UsageProvider for DevinProvider {
    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    fn detect_credentials(&self) -> bool {
        ["DEVIN_TOKEN", "DEVIN_API_TOKEN"].iter().any(|e| {
            std::env::var(e)
                .map(|v| !v.trim().is_empty())
                .unwrap_or(false)
        })
    }

    async fn fetch_usage(&self, ctx: &ProviderContext) -> Result<UsageSnapshot, SpendPanelError> {
        let token = Self::resolve_token(ctx)?;
        let org = Self::resolve_org(ctx)?;
        let client = Self::build_client(ctx)?;
        let url = format!(
            "{}/api/{}/billing/quota/usage",
            self.api_base().trim_end_matches('/'),
            org
        );
        let resp = client
            .get(url)
            .header("Authorization", format!("Bearer {}", token))
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
                "devin".into(),
                format!("invalid Bearer token (HTTP {})", status.as_u16()),
            ));
        }
        if !status.is_success() {
            return Err(SpendPanelError::ProviderError(
                "devin".into(),
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

    const SAMPLE: &str =
        r#"{"daily_percentage": 0.6, "weekly_percentage": 35, "plan_name": "Team"}"#;

    #[test]
    fn test_metadata() {
        assert_eq!(DevinProvider::new().metadata().id, "devin");
    }

    #[test]
    fn test_clean_strips_bearer() {
        assert_eq!(DevinProvider::clean("Bearer abc"), "abc");
        assert_eq!(DevinProvider::clean("Authorization: Bearer xyz"), "xyz");
    }

    #[test]
    fn test_org_required() {
        assert!(matches!(
            DevinProvider::resolve_org(&ProviderContext::new()).unwrap_err(),
            SpendPanelError::ProviderError(_, _)
        ));
    }

    #[test]
    fn test_parse_fraction_and_percent() {
        let snap = DevinProvider::parse(SAMPLE).unwrap();
        // 0.6 fraction → 60% used
        assert_eq!(snap.primary_rate_window.unwrap().used, Some(60));
        // 35 already a percent
        assert_eq!(snap.secondary_rate_window.unwrap().used, Some(35));
        assert_eq!(snap.plan.unwrap().name, "Team");
    }

    #[test]
    fn test_weekly_only() {
        let snap = DevinProvider::parse(r#"{"weekly_percentage": 0.5}"#).unwrap();
        assert!(snap.primary_rate_window.is_none());
        assert_eq!(snap.secondary_rate_window.unwrap().used, Some(50));
    }

    #[test]
    fn test_no_windows_is_error() {
        assert!(matches!(
            DevinProvider::parse(r#"{"plan_name":"X"}"#).unwrap_err(),
            SpendPanelError::ParseError(_, _)
        ));
    }

    #[tokio::test]
    async fn test_fetch_success() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/myorg/billing/quota/usage"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(SAMPLE, "application/json"))
            .mount(&server)
            .await;
        let provider = DevinProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("token".into(), "t".into());
        ctx.config.insert("organization".into(), "myorg".into());
        let snap = provider.fetch_usage(&ctx).await.unwrap();
        assert_eq!(snap.primary_rate_window.unwrap().used, Some(60));
    }

    #[tokio::test]
    async fn test_fetch_401() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/myorg/billing/quota/usage"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        let provider = DevinProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("token".into(), "bad".into());
        ctx.config.insert("organization".into(), "myorg".into());
        assert!(matches!(
            provider.fetch_usage(&ctx).await.unwrap_err(),
            SpendPanelError::AuthFailed(_, _)
        ));
    }
}
