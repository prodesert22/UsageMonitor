use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::error::SpendPanelError;
use crate::model::{CreditsSnapshot, PlanInfo, RateWindow, UsageSnapshot};
use crate::provider::{ProviderContext, ProviderMetadata, UsageProvider};

// MARK: - usage-summary response (modern token-based plans)

#[derive(Debug, Default, serde::Deserialize)]
struct CursorUsageSummary {
    #[serde(default, rename = "membershipType")]
    membership_type: Option<String>,
    #[serde(default, rename = "billingCycleEnd")]
    billing_cycle_end: Option<String>,
    #[serde(default, rename = "individualUsage")]
    individual_usage: Option<CursorIndividualUsage>,
    #[serde(default, rename = "teamUsage")]
    team_usage: Option<CursorTeamUsage>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct CursorIndividualUsage {
    #[serde(default)]
    plan: Option<CursorPlanUsage>,
    #[serde(default, rename = "onDemand")]
    on_demand: Option<CursorMoneyUsage>,
    #[serde(default)]
    overall: Option<CursorMoneyUsage>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct CursorPlanUsage {
    /// Usage in cents.
    #[serde(default)]
    used: Option<i64>,
    /// Limit in cents.
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default, rename = "autoPercentUsed")]
    auto_percent_used: Option<f64>,
    #[serde(default, rename = "apiPercentUsed")]
    api_percent_used: Option<f64>,
    #[serde(default, rename = "totalPercentUsed")]
    total_percent_used: Option<f64>,
}

/// Cents-based usage block shared by on-demand / overall / pooled.
#[derive(Debug, Default, serde::Deserialize)]
struct CursorMoneyUsage {
    #[serde(default)]
    used: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
    /// Remaining cents — accepted from the API but not currently surfaced.
    #[serde(default)]
    #[allow(dead_code)]
    remaining: Option<i64>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct CursorTeamUsage {
    #[serde(default, rename = "onDemand")]
    on_demand: Option<CursorMoneyUsage>,
    #[serde(default)]
    pooled: Option<CursorMoneyUsage>,
}

// MARK: - /api/auth/me + legacy /api/usage

#[derive(Debug, Default, serde::Deserialize)]
struct CursorUserInfo {
    #[serde(default)]
    sub: Option<String>,
    /// Account email — parsed for future identity surfacing.
    #[serde(default)]
    #[allow(dead_code)]
    email: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct CursorUsageResponse {
    #[serde(default, rename = "gpt-4")]
    gpt4: Option<CursorModelUsage>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct CursorModelUsage {
    #[serde(default, rename = "numRequests")]
    num_requests: Option<i64>,
    #[serde(default, rename = "maxRequestUsage")]
    max_request_usage: Option<i64>,
}

impl CursorUsageSummary {
    /// Headline plan percent, mirroring CodexBar's precedence.
    fn plan_percent(&self) -> f64 {
        let clamp = |v: f64| v.clamp(0.0, 100.0);
        let plan = self.individual_usage.as_ref().and_then(|u| u.plan.as_ref());
        if let Some(total) = plan.and_then(|p| p.total_percent_used) {
            return clamp(total);
        }
        let auto = plan.and_then(|p| p.auto_percent_used).map(clamp);
        let api = plan.and_then(|p| p.api_percent_used).map(clamp);
        match (auto, api) {
            (Some(a), Some(b)) => return clamp((a + b) / 2.0),
            (Some(a), None) | (None, Some(a)) => return clamp(a),
            (None, None) => {}
        }
        // Fall through to cents ratios: plan → overall → pooled.
        let ratio = |used: Option<i64>, limit: Option<i64>| -> Option<f64> {
            match (used, limit) {
                (Some(u), Some(l)) if l > 0 => Some(clamp((u as f64 / l as f64) * 100.0)),
                _ => None,
            }
        };
        if let Some(r) = plan.and_then(|p| ratio(p.used, p.limit)) {
            return r;
        }
        let overall = self.individual_usage.as_ref().and_then(|u| u.overall.as_ref());
        if let Some(r) = overall.and_then(|o| ratio(o.used, o.limit)) {
            return r;
        }
        let pooled = self.team_usage.as_ref().and_then(|t| t.pooled.as_ref());
        if let Some(r) = pooled.and_then(|p| ratio(p.used, p.limit)) {
            return r;
        }
        0.0
    }

    /// On-demand spend (used, limit) in USD, when present.
    fn on_demand_usd(&self) -> Option<(f64, Option<f64>)> {
        let block = self
            .individual_usage
            .as_ref()
            .and_then(|u| u.on_demand.as_ref())
            .or_else(|| self.team_usage.as_ref().and_then(|t| t.on_demand.as_ref()))?;
        let used = block.used? as f64 / 100.0;
        let limit = block.limit.map(|l| l as f64 / 100.0);
        Some((used, limit))
    }
}

fn parse_iso(s: &Option<String>) -> Option<DateTime<Utc>> {
    let raw = s.as_deref()?;
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

/// Cursor usage provider (browser-cookie auth).
pub struct CursorProvider {
    metadata: ProviderMetadata,
    base_url: Option<String>,
}

impl CursorProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: "cursor",
                name: "Cursor",
                description: "Cursor usage monitor (browser cookie)",
                auth_methods: &["cookie", "token", "env"],
                website: Some("https://cursor.com"),
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
        self.base_url.as_deref().unwrap_or("https://cursor.com")
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

    /// Builds the `Cookie` header. A full `cookie` value is sent verbatim; a bare
    /// session `token` is wrapped in Cursor's WorkOS session cookie name.
    fn resolve_cookie(ctx: &ProviderContext) -> Result<String, SpendPanelError> {
        if let Some(cookie) = ctx.config.get("cookie") {
            let cleaned = Self::clean(cookie);
            if !cleaned.is_empty() {
                return Ok(cleaned);
            }
        }
        for key in ["token", "session_token", "api_key"] {
            if let Some(value) = ctx.config.get(key) {
                let cleaned = Self::clean(value);
                if !cleaned.is_empty() {
                    return Ok(format!("WorkosCursorSessionToken={}", cleaned));
                }
            }
        }
        if let Ok(value) = std::env::var("CURSOR_SESSION_TOKEN") {
            let cleaned = Self::clean(&value);
            if !cleaned.is_empty() {
                return Ok(format!("WorkosCursorSessionToken={}", cleaned));
            }
        }
        Err(SpendPanelError::AuthFailed(
            "cursor".into(),
            "no session cookie found in cookie/token config or CURSOR_SESSION_TOKEN".into(),
        ))
    }

    fn build_client(ctx: &ProviderContext) -> Result<reqwest::Client, SpendPanelError> {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(ctx.timeout_secs))
            .build()
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))
    }

    async fn get_json<T: serde::de::DeserializeOwned>(
        client: &reqwest::Client,
        url: String,
        cookie: &str,
    ) -> Result<T, SpendPanelError> {
        let resp = client
            .get(url)
            .header("Accept", "application/json")
            .header("Cookie", cookie)
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
                "cursor".into(),
                format!("not logged in (HTTP {})", status.as_u16()),
            ));
        }
        if !status.is_success() {
            return Err(SpendPanelError::ProviderError(
                "cursor".into(),
                format!("HTTP {}: {}", status, body),
            ));
        }
        serde_json::from_str(&body)
            .map_err(|e| SpendPanelError::ParseError("cursor".into(), e.to_string()))
    }

    fn snapshot_from(
        summary: &CursorUsageSummary,
        user: Option<&CursorUserInfo>,
        legacy: Option<&CursorUsageResponse>,
    ) -> UsageSnapshot {
        let mut snapshot = UsageSnapshot::new("cursor");

        // Legacy request-based plan takes the headline window when present.
        let legacy_window = legacy
            .and_then(|r| r.gpt4.as_ref())
            .and_then(|m| match (m.num_requests, m.max_request_usage) {
                (Some(used), Some(limit)) if limit > 0 => Some(RateWindow::new(
                    used.max(0) as u64,
                    limit as u64,
                    "Requests",
                    0,
                )),
                _ => None,
            });

        if let Some(window) = legacy_window {
            snapshot.primary_rate_window = Some(window);
        } else {
            let mut window =
                RateWindow::new(summary.plan_percent().round() as u64, 100, "Plan", 0);
            window.resets_at = parse_iso(&summary.billing_cycle_end);
            snapshot.primary_rate_window = Some(window);
        }

        // On-demand spend surfaces as a credits/spend pool in USD.
        if let Some((used, limit)) = summary.on_demand_usd() {
            let balance = limit.map(|l| (l - used).max(0.0)).unwrap_or(0.0);
            let mut credits = CreditsSnapshot::new(balance, "USD");
            credits.used = Some(used);
            credits.total = limit;
            snapshot.credits = Some(credits);
        }

        if let Some(membership) = summary.membership_type.as_deref().filter(|m| !m.is_empty()) {
            snapshot.plan = Some(PlanInfo {
                name: format_membership(membership),
                tier: None,
                features: Vec::new(),
                price: None,
                currency: None,
                billing_period: None,
            });
        }

        let _ = user; // email reserved for future identity surfacing
        snapshot
    }
}

fn format_membership(raw: &str) -> String {
    match raw.to_lowercase().as_str() {
        "enterprise" => "Enterprise".into(),
        "pro" => "Pro".into(),
        "hobby" => "Hobby".into(),
        "team" => "Team".into(),
        other => {
            let mut chars = other.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        }
    }
}

impl Default for CursorProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl UsageProvider for CursorProvider {
    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    fn detect_credentials(&self) -> bool {
        std::env::var("CURSOR_SESSION_TOKEN")
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false)
    }

    async fn fetch_usage(&self, ctx: &ProviderContext) -> Result<UsageSnapshot, SpendPanelError> {
        let cookie = Self::resolve_cookie(ctx)?;
        let client = Self::build_client(ctx)?;
        let base = self.api_base().trim_end_matches('/');

        let summary: CursorUsageSummary =
            Self::get_json(&client, format!("{}/api/usage-summary", base), &cookie).await?;

        // Identity + legacy request quota are best-effort; not all plans expose them.
        let user: Option<CursorUserInfo> =
            Self::get_json(&client, format!("{}/api/auth/me", base), &cookie)
                .await
                .ok();
        let legacy = match user.as_ref().and_then(|u| u.sub.as_deref()) {
            Some(sub) => Self::get_json::<CursorUsageResponse>(
                &client,
                format!("{}/api/usage?user={}", base, sub),
                &cookie,
            )
            .await
            .ok(),
            None => None,
        };

        Ok(Self::snapshot_from(&summary, user.as_ref(), legacy.as_ref()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const SUMMARY: &str = r#"{
      "membershipType": "pro",
      "billingCycleEnd": "2026-07-12T00:00:00Z",
      "individualUsage": {
        "plan": {"used": 1500, "limit": 2000, "totalPercentUsed": 75.0},
        "onDemand": {"used": 250, "limit": 1000, "remaining": 750}
      }
    }"#;

    fn summary(body: &str) -> CursorUsageSummary {
        serde_json::from_str(body).unwrap()
    }

    #[test]
    fn test_metadata() {
        let p = CursorProvider::new();
        assert_eq!(p.metadata().id, "cursor");
        assert!(p.metadata().auth_methods.contains(&"cookie"));
    }

    #[test]
    fn test_resolve_cookie_token_wrapped() {
        let mut ctx = ProviderContext::new();
        ctx.config.insert("token".into(), "tok".into());
        assert_eq!(
            CursorProvider::resolve_cookie(&ctx).unwrap(),
            "WorkosCursorSessionToken=tok"
        );
    }

    #[test]
    fn test_plan_percent_prefers_total() {
        assert_eq!(summary(SUMMARY).plan_percent(), 75.0);
    }

    #[test]
    fn test_plan_percent_avg_auto_api() {
        let s = summary(
            r#"{"individualUsage":{"plan":{"autoPercentUsed":40.0,"apiPercentUsed":60.0}}}"#,
        );
        assert_eq!(s.plan_percent(), 50.0);
    }

    #[test]
    fn test_plan_percent_cents_ratio_fallback() {
        let s = summary(r#"{"individualUsage":{"plan":{"used":300,"limit":1200}}}"#);
        assert_eq!(s.plan_percent(), 25.0);
    }

    #[test]
    fn test_plan_percent_team_pooled_fallback() {
        // No individual usage → fall through to the shared team pool ratio.
        let s = summary(r#"{"teamUsage":{"pooled":{"used":300,"limit":1000}}}"#);
        assert_eq!(s.plan_percent(), 30.0);
    }

    #[test]
    fn test_plan_percent_overall_fallback() {
        let s = summary(r#"{"individualUsage":{"overall":{"used":7384,"limit":10000}}}"#);
        assert!((s.plan_percent() - 73.84).abs() < 1e-6);
    }

    #[test]
    fn test_on_demand_from_team_usage() {
        let s = summary(r#"{"teamUsage":{"onDemand":{"used":150,"limit":500}}}"#);
        let (used, limit) = s.on_demand_usd().unwrap();
        assert_eq!(used, 1.5);
        assert_eq!(limit, Some(5.0));
    }

    #[test]
    fn test_on_demand_usd() {
        let (used, limit) = summary(SUMMARY).on_demand_usd().unwrap();
        assert_eq!(used, 2.5);
        assert_eq!(limit, Some(10.0));
    }

    #[test]
    fn test_snapshot_plan_window_and_credits() {
        let snapshot = CursorProvider::snapshot_from(&summary(SUMMARY), None, None);
        let primary = snapshot.primary_rate_window.unwrap();
        assert_eq!(primary.used, Some(75));
        assert_eq!(primary.limit, Some(100));
        let credits = snapshot.credits.unwrap();
        assert_eq!(credits.used, Some(2.5));
        assert_eq!(credits.balance, 7.5);
        assert_eq!(snapshot.plan.unwrap().name, "Pro");
    }

    #[test]
    fn test_legacy_requests_override() {
        let legacy: CursorUsageResponse =
            serde_json::from_str(r#"{"gpt-4":{"numRequests":120,"maxRequestUsage":500}}"#).unwrap();
        let snapshot = CursorProvider::snapshot_from(&summary(SUMMARY), None, Some(&legacy));
        let primary = snapshot.primary_rate_window.unwrap();
        assert_eq!(primary.used, Some(120));
        assert_eq!(primary.limit, Some(500));
        assert_eq!(primary.label, "Requests");
    }

    #[tokio::test]
    async fn test_fetch_usage_success() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/usage-summary"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(SUMMARY, "application/json"))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/auth/me"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_raw(r#"{"sub":"user_1","email":"a@b.c"}"#, "application/json"),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/usage"))
            .and(query_param("user", "user_1"))
            .respond_with(ResponseTemplate::new(200).set_body_raw("{}", "application/json"))
            .mount(&server)
            .await;

        let provider = CursorProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("token".into(), "tok".into());
        let snapshot = provider.fetch_usage(&ctx).await.unwrap();
        assert_eq!(snapshot.primary_rate_window.unwrap().used, Some(75));
    }

    #[tokio::test]
    async fn test_fetch_usage_401_is_auth_failed() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/usage-summary"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;

        let provider = CursorProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("token".into(), "bad".into());
        let err = provider.fetch_usage(&ctx).await.unwrap_err();
        assert!(matches!(err, SpendPanelError::AuthFailed(_, _)));
    }
}
