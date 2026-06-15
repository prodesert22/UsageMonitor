use async_trait::async_trait;
use chrono::{TimeZone, Utc};

use crate::error::SpendPanelError;
use crate::model::{CreditsSnapshot, PlanInfo, RateWindow, UsageSnapshot};
use crate::provider::{ProviderContext, ProviderMetadata, UsageProvider};

/// Default session cookie name used by perplexity.ai (next-auth).
const DEFAULT_COOKIE_NAME: &str = "__Secure-next-auth.session-token";

#[derive(Debug, serde::Deserialize)]
struct PerplexityCreditsResponse {
    #[serde(default)]
    balance_cents: f64,
    #[serde(default)]
    renewal_date_ts: f64,
    #[serde(default)]
    current_period_purchased_cents: f64,
    #[serde(default)]
    credit_grants: Vec<PerplexityCreditGrant>,
    #[serde(default)]
    total_usage_cents: f64,
}

#[derive(Debug, serde::Deserialize)]
struct PerplexityCreditGrant {
    #[serde(rename = "type")]
    grant_type: String,
    #[serde(default)]
    amount_cents: f64,
    #[serde(default)]
    expires_at_ts: Option<f64>,
}

/// Credit pools resolved from the raw response (all values in cents).
#[derive(Debug, Clone, PartialEq)]
struct PerplexityCredits {
    recurring_total: f64,
    recurring_used: f64,
    promo_total: f64,
    promo_used: f64,
    purchased_total: f64,
    purchased_used: f64,
    balance_cents: f64,
    total_usage_cents: f64,
    renewal_ts: f64,
    promo_expiry_ts: Option<f64>,
}

impl PerplexityCredits {
    /// Mirrors CodexBar's waterfall attribution: recurring → purchased → promo.
    fn from_response(resp: &PerplexityCreditsResponse, now_ts: f64) -> Self {
        let sum = |kind: &str| -> f64 {
            resp.credit_grants
                .iter()
                .filter(|g| g.grant_type == kind)
                .map(|g| g.amount_cents)
                .sum::<f64>()
                .max(0.0)
        };

        let recurring_sum = sum("recurring");
        let promo_sum = resp
            .credit_grants
            .iter()
            .filter(|g| g.grant_type == "promotional")
            .filter(|g| g.expires_at_ts.unwrap_or(f64::INFINITY) > now_ts)
            .map(|g| g.amount_cents)
            .sum::<f64>()
            .max(0.0);

        // Purchased credits can appear in the grants array, the top-level field,
        // or both. Take whichever is larger to avoid double counting.
        let purchased_from_grants = sum("purchased");
        let purchased_from_field = resp.current_period_purchased_cents.max(0.0);
        let purchased_sum = purchased_from_grants.max(purchased_from_field);

        let mut remaining = resp.total_usage_cents;
        let used_from_recurring = remaining.min(recurring_sum).max(0.0);
        remaining -= used_from_recurring;
        let used_from_purchased = remaining.min(purchased_sum).max(0.0);
        remaining -= used_from_purchased;
        let used_from_promo = remaining.min(promo_sum).max(0.0);

        let promo_expiry_ts = resp
            .credit_grants
            .iter()
            .filter(|g| g.grant_type == "promotional")
            .filter(|g| g.expires_at_ts.unwrap_or(f64::INFINITY) > now_ts)
            .filter_map(|g| g.expires_at_ts)
            .fold(None, |acc: Option<f64>, ts| {
                Some(acc.map_or(ts, |cur| cur.min(ts)))
            });

        Self {
            recurring_total: recurring_sum,
            recurring_used: used_from_recurring,
            promo_total: promo_sum,
            promo_used: used_from_promo,
            purchased_total: purchased_sum,
            purchased_used: used_from_purchased,
            balance_cents: resp.balance_cents,
            total_usage_cents: resp.total_usage_cents,
            renewal_ts: resp.renewal_date_ts,
            promo_expiry_ts,
        }
    }

    /// Infer plan name from the recurring allotment (Free=0, Pro<$50, Max≥$50).
    fn plan_name(&self) -> Option<&'static str> {
        if self.recurring_total <= 0.0 {
            None
        } else if self.recurring_total < 5000.0 {
            Some("Pro")
        } else {
            Some("Max")
        }
    }
}

/// Perplexity credits/usage provider (browser-cookie auth).
pub struct PerplexityProvider {
    metadata: ProviderMetadata,
    /// Base URL override for tests.
    base_url: Option<String>,
}

impl PerplexityProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: "perplexity",
                name: "Perplexity",
                description: "Perplexity AI credits monitor (browser cookie)",
                auth_methods: &["cookie", "token", "env"],
                website: Some("https://www.perplexity.ai"),
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
        self.base_url
            .as_deref()
            .unwrap_or("https://www.perplexity.ai")
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

    /// Builds the `Cookie` header from config or environment.
    ///
    /// A full `cookie` value is sent verbatim; a bare session `token` is wrapped
    /// in the default next-auth cookie name.
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
                    return Ok(format!("{}={}", DEFAULT_COOKIE_NAME, cleaned));
                }
            }
        }
        if let Ok(value) = std::env::var("PERPLEXITY_SESSION_TOKEN") {
            let cleaned = Self::clean(&value);
            if !cleaned.is_empty() {
                return Ok(format!("{}={}", DEFAULT_COOKIE_NAME, cleaned));
            }
        }
        Err(SpendPanelError::AuthFailed(
            "perplexity".into(),
            "no session cookie found in cookie/token config or PERPLEXITY_SESSION_TOKEN".into(),
        ))
    }

    fn build_client(ctx: &ProviderContext) -> Result<reqwest::Client, SpendPanelError> {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(ctx.timeout_secs))
            .build()
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))
    }

    async fn fetch_credits(
        base_url: &str,
        client: &reqwest::Client,
        cookie: &str,
    ) -> Result<PerplexityCreditsResponse, SpendPanelError> {
        let url = format!(
            "{}/rest/billing/credits?version=2.18&source=default",
            base_url.trim_end_matches('/')
        );
        let resp = client
            .get(url)
            .header("Accept", "application/json")
            .header("Cookie", cookie)
            .header("Origin", "https://www.perplexity.ai")
            .header("Referer", "https://www.perplexity.ai/account/usage")
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
                "perplexity".into(),
                format!(
                    "invalid or expired session cookie (HTTP {})",
                    status.as_u16()
                ),
            ));
        }
        if !status.is_success() {
            return Err(SpendPanelError::ProviderError(
                "perplexity".into(),
                format!("HTTP {}: {}", status, body),
            ));
        }

        serde_json::from_str(&body)
            .map_err(|e| SpendPanelError::ParseError("perplexity".into(), e.to_string()))
    }

    /// Converts a positive Unix-seconds timestamp to a UTC datetime (skips 0/negative).
    fn ts_to_date(ts: f64) -> Option<chrono::DateTime<Utc>> {
        if ts > 0.0 {
            Utc.timestamp_opt(ts as i64, 0).single()
        } else {
            None
        }
    }

    fn snapshot_from_credits(credits: PerplexityCredits) -> UsageSnapshot {
        let mut snapshot = UsageSnapshot::new("perplexity");

        // Primary: recurring (monthly) plan credits.
        if credits.recurring_total > 0.0 {
            let mut window = RateWindow::new(
                credits.recurring_used.round() as u64,
                credits.recurring_total.round() as u64,
                "Plan credits",
                0,
            );
            window.resets_at = Self::ts_to_date(credits.renewal_ts);
            snapshot.primary_rate_window = Some(window);
        }

        // Secondary: promotional bonus credits.
        if credits.promo_total > 0.0 {
            let mut window = RateWindow::new(
                credits.promo_used.round() as u64,
                credits.promo_total.round() as u64,
                "Bonus credits",
                0,
            );
            window.resets_at = credits
                .promo_expiry_ts
                .and_then(|ts| Utc.timestamp_opt(ts as i64, 0).single());
            snapshot.secondary_rate_window = Some(window);
        }

        // Tertiary: on-demand purchased credits.
        if credits.purchased_total > 0.0 {
            snapshot.tertiary_rate_window = Some(RateWindow::new(
                credits.purchased_used.round() as u64,
                credits.purchased_total.round() as u64,
                "Purchased credits",
                0,
            ));
        }

        let mut credits_snapshot = CreditsSnapshot::new(credits.balance_cents / 100.0, "USD");
        credits_snapshot.used = Some(credits.total_usage_cents / 100.0);
        credits_snapshot.bonus = Some(credits.promo_total / 100.0);
        credits_snapshot.purchased = Some(credits.purchased_total / 100.0);
        credits_snapshot.renews_at = Self::ts_to_date(credits.renewal_ts);
        snapshot.credits = Some(credits_snapshot);

        if let Some(plan) = credits.plan_name() {
            snapshot.plan = Some(PlanInfo {
                name: plan.to_string(),
                tier: None,
                features: Vec::new(),
                price: None,
                currency: None,
                billing_period: Some("monthly".into()),
            });
        }

        snapshot
    }
}

impl Default for PerplexityProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl UsageProvider for PerplexityProvider {
    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    fn detect_credentials(&self) -> bool {
        std::env::var("PERPLEXITY_SESSION_TOKEN")
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false)
    }

    async fn fetch_usage(&self, ctx: &ProviderContext) -> Result<UsageSnapshot, SpendPanelError> {
        let cookie = Self::resolve_cookie(ctx)?;
        let client = Self::build_client(ctx)?;
        let response = Self::fetch_credits(self.api_base(), &client, &cookie).await?;
        let now_ts = Utc::now().timestamp() as f64;
        let credits = PerplexityCredits::from_response(&response, now_ts);
        Ok(Self::snapshot_from_credits(credits))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const SAMPLE: &str = r#"{
      "balance_cents": 1500.0,
      "renewal_date_ts": 1788000000,
      "current_period_purchased_cents": 1000.0,
      "total_usage_cents": 700.0,
      "credit_grants": [
        {"type": "recurring", "amount_cents": 500.0},
        {"type": "promotional", "amount_cents": 300.0, "expires_at_ts": 9999999999},
        {"type": "purchased", "amount_cents": 1000.0}
      ]
    }"#;

    fn parse(body: &str, now_ts: f64) -> PerplexityCredits {
        let resp: PerplexityCreditsResponse = serde_json::from_str(body).unwrap();
        PerplexityCredits::from_response(&resp, now_ts)
    }

    #[test]
    fn test_metadata() {
        let p = PerplexityProvider::new();
        assert_eq!(p.metadata().id, "perplexity");
        assert!(p.metadata().auth_methods.contains(&"cookie"));
    }

    #[test]
    fn test_resolve_cookie_full_cookie_verbatim() {
        let mut ctx = ProviderContext::new();
        ctx.config.insert(
            "cookie".into(),
            "__Secure-next-auth.session-token=abc".into(),
        );
        assert_eq!(
            PerplexityProvider::resolve_cookie(&ctx).unwrap(),
            "__Secure-next-auth.session-token=abc"
        );
    }

    #[test]
    fn test_resolve_cookie_token_wrapped() {
        let mut ctx = ProviderContext::new();
        ctx.config.insert("token".into(), "sess-xyz".into());
        assert_eq!(
            PerplexityProvider::resolve_cookie(&ctx).unwrap(),
            "__Secure-next-auth.session-token=sess-xyz"
        );
    }

    #[test]
    fn test_resolve_cookie_missing_is_error() {
        let err = PerplexityProvider::resolve_cookie(&ProviderContext::new()).unwrap_err();
        assert!(matches!(err, SpendPanelError::AuthFailed(_, _)));
    }

    #[test]
    fn test_waterfall_attribution() {
        // total_usage 700: recurring(500) fully used, then purchased(200), promo 0.
        let credits = parse(SAMPLE, 1.0);
        assert_eq!(credits.recurring_total, 500.0);
        assert_eq!(credits.recurring_used, 500.0);
        assert_eq!(credits.purchased_total, 1000.0);
        assert_eq!(credits.purchased_used, 200.0);
        assert_eq!(credits.promo_total, 300.0);
        assert_eq!(credits.promo_used, 0.0);
    }

    #[test]
    fn test_expired_promo_excluded() {
        let body = r#"{
          "balance_cents": 0,
          "renewal_date_ts": 0,
          "current_period_purchased_cents": 0,
          "total_usage_cents": 0,
          "credit_grants": [
            {"type": "promotional", "amount_cents": 300.0, "expires_at_ts": 100}
          ]
        }"#;
        let credits = parse(body, 200.0);
        assert_eq!(credits.promo_total, 0.0);
    }

    #[test]
    fn test_plan_name_thresholds() {
        let pro = PerplexityCredits {
            recurring_total: 2000.0,
            ..parse(SAMPLE, 1.0)
        };
        assert_eq!(pro.plan_name(), Some("Pro"));
        let max = PerplexityCredits {
            recurring_total: 10000.0,
            ..parse(SAMPLE, 1.0)
        };
        assert_eq!(max.plan_name(), Some("Max"));
    }

    #[test]
    fn test_snapshot_maps_pools() {
        let snapshot = PerplexityProvider::snapshot_from_credits(parse(SAMPLE, 1.0));
        let primary = snapshot.primary_rate_window.unwrap();
        assert_eq!(primary.used, Some(500));
        assert_eq!(primary.limit, Some(500));
        let tertiary = snapshot.tertiary_rate_window.unwrap();
        assert_eq!(tertiary.used, Some(200));
        assert_eq!(tertiary.limit, Some(1000));
        let credits = snapshot.credits.unwrap();
        assert_eq!(credits.balance, 15.0);
        assert_eq!(credits.purchased, Some(10.0));
    }

    #[test]
    fn test_no_recurring_drops_primary_keeps_pools() {
        // Free plan: no recurring credits, but purchased/bonus remain.
        let body = r#"{
          "balance_cents": 500,
          "renewal_date_ts": 0,
          "current_period_purchased_cents": 800,
          "total_usage_cents": 100,
          "credit_grants": [
            {"type": "purchased", "amount_cents": 800},
            {"type": "promotional", "amount_cents": 200, "expires_at_ts": 9999999999}
          ]
        }"#;
        let snapshot = PerplexityProvider::snapshot_from_credits(parse(body, 1.0));
        assert!(
            snapshot.primary_rate_window.is_none(),
            "no recurring → no primary"
        );
        assert!(snapshot.secondary_rate_window.is_some());
        assert!(snapshot.tertiary_rate_window.is_some());
        // renewal_date_ts 0 must not produce a 1970 reset.
        assert!(snapshot.credits.unwrap().renews_at.is_none());
    }

    #[tokio::test]
    async fn test_fetch_usage_success() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rest/billing/credits"))
            .and(header("cookie", "__Secure-next-auth.session-token=abc"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(SAMPLE, "application/json"))
            .mount(&server)
            .await;

        let provider = PerplexityProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("token".into(), "abc".into());
        let snapshot = provider.fetch_usage(&ctx).await.unwrap();
        assert_eq!(snapshot.credits.unwrap().balance, 15.0);
    }

    #[tokio::test]
    async fn test_fetch_usage_401_is_auth_failed() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rest/billing/credits"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let provider = PerplexityProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("token".into(), "bad".into());
        let err = provider.fetch_usage(&ctx).await.unwrap_err();
        assert!(matches!(err, SpendPanelError::AuthFailed(_, _)));
    }
}
