use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::error::SpendPanelError;
use crate::model::{CreditsSnapshot, PlanInfo, RateWindow, UsageSnapshot};
use crate::provider::{ProviderContext, ProviderMetadata, UsageProvider};

/// Abacus AI compute-points provider (apps.abacus.ai, browser cookie auth).
pub struct AbacusProvider {
    metadata: ProviderMetadata,
    base_url: Option<String>,
}

impl AbacusProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: "abacus",
                name: "Abacus AI",
                description: "Abacus AI compute-points monitor (browser cookie)",
                auth_methods: &["cookie", "env"],
                website: Some("https://abacus.ai"),
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
        self.base_url.as_deref().unwrap_or("https://apps.abacus.ai")
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

    fn resolve_cookie(ctx: &ProviderContext) -> Result<String, SpendPanelError> {
        for key in ["cookie", "token"] {
            if let Some(v) = ctx.config.get(key) {
                let c = Self::clean(v);
                if !c.is_empty() {
                    return Ok(c);
                }
            }
        }
        if let Ok(v) = std::env::var("ABACUS_COOKIE") {
            let c = Self::clean(&v);
            if !c.is_empty() {
                return Ok(c);
            }
        }
        Err(SpendPanelError::AuthFailed(
            "abacus".into(),
            "no session cookie in cookie config or ABACUS_COOKIE".into(),
        ))
    }

    fn build_client(ctx: &ProviderContext) -> Result<reqwest::Client, SpendPanelError> {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(ctx.timeout_secs))
            .build()
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))
    }

    async fn get_json(
        client: &reqwest::Client,
        url: String,
        cookie: &str,
    ) -> Result<serde_json::Value, SpendPanelError> {
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
                "abacus".into(),
                format!("session cookie rejected (HTTP {})", status.as_u16()),
            ));
        }
        if !status.is_success() {
            return Err(SpendPanelError::ProviderError(
                "abacus".into(),
                format!("HTTP {}: {}", status, body),
            ));
        }
        serde_json::from_str(&body)
            .map_err(|e| SpendPanelError::ParseError("abacus".into(), e.to_string()))
    }

    fn snapshot_from(
        compute: &serde_json::Value,
        billing: Option<&serde_json::Value>,
    ) -> Result<UsageSnapshot, SpendPanelError> {
        let total = num(compute, "totalComputePoints");
        let left = num(compute, "computePointsLeft");
        let (Some(total), Some(left)) = (total, left) else {
            return Err(SpendPanelError::ParseError(
                "abacus".into(),
                "missing totalComputePoints / computePointsLeft".into(),
            ));
        };
        let used = (total - left).max(0.0);

        let mut snapshot = UsageSnapshot::new("abacus");
        let mut window = RateWindow::new(used.round() as u64, total.round() as u64, "Compute points", 0);
        let resets = billing
            .and_then(|b| b.get("nextBillingDate"))
            .and_then(parse_date);
        window.resets_at = resets;
        snapshot.primary_rate_window = Some(window);

        let mut credits = CreditsSnapshot::new(left, "credits");
        credits.total = Some(total);
        credits.used = Some(used);
        credits.renews_at = resets;
        snapshot.credits = Some(credits);

        if let Some(tier) = billing
            .and_then(|b| b.get("currentTier"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            snapshot.plan = Some(PlanInfo {
                name: tier.to_string(),
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

fn num(v: &serde_json::Value, key: &str) -> Option<f64> {
    v.get(key)
        .and_then(|x| x.as_f64().or_else(|| x.as_str().and_then(|s| s.parse().ok())))
}

fn parse_date(v: &serde_json::Value) -> Option<DateTime<Utc>> {
    if let Some(s) = v.as_str() {
        if let Ok(secs) = s.parse::<i64>() {
            let secs = if secs > 1_000_000_000_000 { secs / 1000 } else { secs };
            return chrono::TimeZone::timestamp_opt(&Utc, secs, 0).single();
        }
        return DateTime::parse_from_rfc3339(s)
            .ok()
            .map(|d| d.with_timezone(&Utc));
    }
    if let Some(secs) = v.as_i64() {
        let secs = if secs > 1_000_000_000_000 { secs / 1000 } else { secs };
        return chrono::TimeZone::timestamp_opt(&Utc, secs, 0).single();
    }
    None
}

impl Default for AbacusProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl UsageProvider for AbacusProvider {
    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    fn detect_credentials(&self) -> bool {
        std::env::var("ABACUS_COOKIE")
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false)
    }

    async fn fetch_usage(&self, ctx: &ProviderContext) -> Result<UsageSnapshot, SpendPanelError> {
        let cookie = Self::resolve_cookie(ctx)?;
        let client = Self::build_client(ctx)?;
        let base = self.api_base().trim_end_matches('/');
        let compute =
            Self::get_json(&client, format!("{}/api/_getOrganizationComputePoints", base), &cookie)
                .await?;
        // Billing is optional — plan/reset are best-effort.
        let billing = Self::get_json(&client, format!("{}/api/_getBillingInfo", base), &cookie)
            .await
            .ok();
        Self::snapshot_from(&compute, billing.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn test_metadata() {
        assert_eq!(AbacusProvider::new().metadata().id, "abacus");
    }

    #[test]
    fn test_snapshot_from_compute_and_billing() {
        let compute = serde_json::json!({"totalComputePoints": 1000, "computePointsLeft": 400});
        let billing = serde_json::json!({"currentTier": "PRO", "nextBillingDate": "2026-07-01T00:00:00Z"});
        let snap = AbacusProvider::snapshot_from(&compute, Some(&billing)).unwrap();
        let w = snap.primary_rate_window.unwrap();
        assert_eq!(w.used, Some(600));
        assert_eq!(w.limit, Some(1000));
        let c = snap.credits.unwrap();
        assert_eq!(c.balance, 400.0);
        assert_eq!(snap.plan.unwrap().name, "PRO");
    }

    #[test]
    fn test_snapshot_without_billing() {
        // Billing is optional — no plan, no reset, but credits still resolve.
        let compute = serde_json::json!({"totalComputePoints": 800, "computePointsLeft": 800});
        let snap = AbacusProvider::snapshot_from(&compute, None).unwrap();
        assert_eq!(snap.primary_rate_window.unwrap().used, Some(0));
        assert_eq!(snap.credits.unwrap().balance, 800.0);
        assert!(snap.plan.is_none());
    }

    #[test]
    fn test_missing_fields_error() {
        let compute = serde_json::json!({"foo": 1});
        assert!(matches!(
            AbacusProvider::snapshot_from(&compute, None).unwrap_err(),
            SpendPanelError::ParseError(_, _)
        ));
    }

    #[tokio::test]
    async fn test_fetch_success() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/_getOrganizationComputePoints"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{"totalComputePoints": 500, "computePointsLeft": 200}"#,
                "application/json",
            ))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/_getBillingInfo"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{"currentTier": "Free"}"#,
                "application/json",
            ))
            .mount(&server)
            .await;
        let provider = AbacusProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("cookie".into(), "sid=abc".into());
        let snap = provider.fetch_usage(&ctx).await.unwrap();
        assert_eq!(snap.primary_rate_window.unwrap().used, Some(300));
        assert_eq!(snap.plan.unwrap().name, "Free");
    }

    #[tokio::test]
    async fn test_fetch_401() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/_getOrganizationComputePoints"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let provider = AbacusProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("cookie".into(), "bad".into());
        assert!(matches!(
            provider.fetch_usage(&ctx).await.unwrap_err(),
            SpendPanelError::AuthFailed(_, _)
        ));
    }
}
