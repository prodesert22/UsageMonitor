use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::error::SpendPanelError;
use crate::model::{NamedRateWindow, RateWindow, UsageSnapshot};
use crate::provider::{ProviderContext, ProviderMetadata, UsageProvider};

#[derive(Debug, serde::Deserialize)]
struct KimiUsageResponse {
    #[serde(default)]
    usages: Vec<KimiUsage>,
    /// Overall quota for the billing cycle ("uso total" in the Kimi UI).
    #[serde(default, rename = "totalQuota")]
    total_quota: Option<KimiUsageDetail>,
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
    /// Rolling window descriptor (e.g. 300 TIME_UNIT_MINUTE = 5h session).
    #[serde(default)]
    window: Option<KimiWindow>,
    detail: KimiUsageDetail,
}

#[derive(Debug, serde::Deserialize)]
struct KimiWindow {
    #[serde(default)]
    duration: Option<u64>,
    #[serde(default, rename = "timeUnit")]
    time_unit: Option<String>,
}

impl KimiWindow {
    /// Window length in minutes, converted from the declared time unit.
    fn minutes(&self) -> Option<u32> {
        let d = self.duration?;
        let factor: u64 = match self.time_unit.as_deref() {
            Some("TIME_UNIT_SECOND") => return u32::try_from(d.div_ceil(60)).ok(),
            Some("TIME_UNIT_HOUR") => 60,
            Some("TIME_UNIT_DAY") => 1440,
            Some("TIME_UNIT_WEEK") => 10080,
            _ => 1, // TIME_UNIT_MINUTE or unknown: assume minutes
        };
        u32::try_from(d.saturating_mul(factor)).ok()
    }
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
            let secs = if secs > 1_000_000_000_000 {
                secs / 1000
            } else {
                secs
            };
            return chrono::TimeZone::timestamp_opt(&Utc, secs, 0).single();
        }
        DateTime::parse_from_rfc3339(raw)
            .ok()
            .map(|d| d.with_timezone(&Utc))
    }
}

/// Short human label for a window length in minutes (e.g. "5h", "7d").
fn window_label(minutes: u32) -> String {
    if minutes >= 1440 && minutes % 1440 == 0 {
        format!("{}d", minutes / 1440)
    } else if minutes >= 60 && minutes % 60 == 0 {
        format!("{}h", minutes / 60)
    } else {
        format!("{}min", minutes)
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
        let mut ordered: Vec<RateWindow> = Vec::new();

        // Rolling rate limits (e.g. the 5h session) come first: they are the
        // most urgent window. `limits[0]` is the session in practice.
        if let Some(limits) = &coding.limits {
            for (i, rate) in limits.iter().enumerate() {
                let (u, l) = rate.detail.counts();
                let minutes = rate
                    .window
                    .as_ref()
                    .and_then(|w| w.minutes())
                    .unwrap_or(300);
                let label = if i == 0 {
                    "Sessão".to_string()
                } else {
                    format!("Limite {}", window_label(minutes))
                };
                let mut window = RateWindow::new(u, l, label, minutes);
                window.resets_at = rate.detail.resets_at();
                ordered.push(window);
            }
        }

        // The scope-level detail is the 7-day (weekly) quota.
        let (wused, wlimit) = coding.detail.counts();
        let mut weekly = RateWindow::new(wused, wlimit, "Semanal", 10080);
        weekly.resets_at = coding.detail.resets_at();
        ordered.push(weekly);

        // Overall cycle quota ("uso total" in the Kimi UI).
        if let Some(total) = &resp.total_quota {
            let (tused, tlimit) = total.counts();
            ordered.push(RateWindow::new(tused, tlimit, "Total", 0));
        }

        // Most urgent (shortest) window first; the cycle total (no window)
        // always sorts last.
        ordered.sort_by_key(|w| {
            if w.window_minutes == 0 {
                u32::MAX
            } else {
                w.window_minutes
            }
        });

        for (idx, window) in ordered.into_iter().enumerate() {
            match idx {
                0 => snapshot.primary_rate_window = Some(window),
                1 => snapshot.secondary_rate_window = Some(window),
                2 => snapshot.tertiary_rate_window = Some(window),
                n => snapshot.extra_rate_windows.push(NamedRateWindow {
                    id: format!("window_{}", n),
                    label: window.label.clone(),
                    window,
                }),
            }
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
        ["KIMI_AUTH_TOKEN", "KIMI_API_KEY"].iter().any(|e| {
            std::env::var(e)
                .map(|v| !v.trim().is_empty())
                .unwrap_or(false)
        })
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

    /// Mirrors the real GetUsages response shape (RFC3339 resetTime, window
    /// descriptor on the session limit, top-level totalQuota).
    const SAMPLE: &str = r#"{
      "usages": [
        {"scope": "FEATURE_CODING",
         "detail": {"limit": "100", "used": "43", "remaining": "57",
                    "resetTime": "2026-07-31T15:29:07.734013Z"},
         "limits": [
           {"window": {"duration": 300, "timeUnit": "TIME_UNIT_MINUTE"},
            "detail": {"limit": "100", "used": "15", "remaining": "85",
                       "resetTime": "2026-07-25T23:29:07.734013Z"}}
         ]}
      ],
      "totalQuota": {"limit": "100", "used": "30", "remaining": "70"}
    }"#;

    #[test]
    fn test_metadata() {
        assert_eq!(KimiProvider::new().metadata().id, "kimi");
    }

    #[test]
    fn test_parse_session_weekly_total() {
        let snap = KimiProvider::parse(SAMPLE).unwrap();
        let session = snap.primary_rate_window.unwrap();
        assert_eq!(session.label, "Sessão");
        assert_eq!(session.used, Some(15));
        assert_eq!(session.limit, Some(100));
        assert_eq!(session.window_minutes, 300);
        assert!(session.resets_at.is_some());

        let weekly = snap.secondary_rate_window.unwrap();
        assert_eq!(weekly.label, "Semanal");
        assert_eq!(weekly.used, Some(43));
        assert_eq!(weekly.window_minutes, 10080);
        assert!(weekly.resets_at.is_some());

        let total = snap.tertiary_rate_window.unwrap();
        assert_eq!(total.label, "Total");
        assert_eq!(total.used, Some(30));
        assert_eq!(total.limit, Some(100));
        assert!(total.resets_at.is_none());
    }

    #[test]
    fn test_reset_time_rfc3339_parses() {
        let snap = KimiProvider::parse(SAMPLE).unwrap();
        let weekly = snap.secondary_rate_window.unwrap();
        let reset = weekly.resets_at.unwrap();
        assert_eq!(reset.to_rfc3339(), "2026-07-31T15:29:07.734013+00:00");
    }

    #[test]
    fn test_reset_time_epoch_still_parses() {
        let body = r#"{"usages":[{"scope":"FEATURE_CODING",
          "detail":{"limit":"10","used":"3","resetTime":"1788000000"}}]}"#;
        let snap = KimiProvider::parse(body).unwrap();
        assert!(snap.primary_rate_window.unwrap().resets_at.is_some());
    }

    #[test]
    fn test_no_limits_weekly_becomes_primary() {
        let body = r#"{"usages":[{"scope":"FEATURE_CODING",
          "detail":{"limit":"1000","remaining":"600"}}]}"#;
        let snap = KimiProvider::parse(body).unwrap();
        let weekly = snap.primary_rate_window.unwrap();
        // used = limit - remaining = 400
        assert_eq!(weekly.used, Some(400));
        assert_eq!(weekly.label, "Semanal");
        assert!(snap.secondary_rate_window.is_none());
    }

    #[test]
    fn test_no_total_quota_drops_tertiary() {
        let body = r#"{"usages":[{"scope":"FEATURE_CODING",
          "detail":{"limit":"100","used":"43"},
          "limits":[{"window":{"duration":300,"timeUnit":"TIME_UNIT_MINUTE"},
                     "detail":{"limit":"100","used":"15"}}]}]}"#;
        let snap = KimiProvider::parse(body).unwrap();
        assert_eq!(snap.primary_rate_window.unwrap().label, "Sessão");
        assert_eq!(snap.secondary_rate_window.unwrap().label, "Semanal");
        assert!(snap.tertiary_rate_window.is_none());
    }

    #[test]
    fn test_window_unit_conversion() {
        let w = |d: u64, u: &str| KimiWindow {
            duration: Some(d),
            time_unit: Some(u.into()),
        };
        assert_eq!(w(300, "TIME_UNIT_MINUTE").minutes(), Some(300));
        assert_eq!(w(5, "TIME_UNIT_HOUR").minutes(), Some(300));
        assert_eq!(w(1, "TIME_UNIT_DAY").minutes(), Some(1440));
        assert_eq!(w(1, "TIME_UNIT_WEEK").minutes(), Some(10080));
        assert_eq!(w(90, "TIME_UNIT_SECOND").minutes(), Some(2));
    }

    #[test]
    fn test_falls_back_to_first_scope() {
        let body = r#"{"usages":[{"scope":"OTHER",
          "detail":{"limit":"10","used":"3"}}]}"#;
        let snap = KimiProvider::parse(body).unwrap();
        assert_eq!(snap.primary_rate_window.unwrap().used, Some(3));
    }

    #[test]
    fn test_extra_limits_beyond_session() {
        let body = r#"{"usages":[{"scope":"FEATURE_CODING",
          "detail":{"limit":"100","used":"10"},
          "limits":[
            {"window":{"duration":300,"timeUnit":"TIME_UNIT_MINUTE"},
             "detail":{"limit":"100","used":"5"}},
            {"window":{"duration":1,"timeUnit":"TIME_UNIT_DAY"},
             "detail":{"limit":"50","used":"20"}}
          ]}]}"#;
        let snap = KimiProvider::parse(body).unwrap();
        // Sorted by urgency: 5h session, 1d limit, then weekly.
        assert_eq!(snap.primary_rate_window.unwrap().label, "Sessão");
        let daily = snap.secondary_rate_window.unwrap();
        assert_eq!(daily.label, "Limite 1d");
        assert_eq!(daily.used, Some(20));
        assert_eq!(snap.tertiary_rate_window.unwrap().label, "Semanal");
        assert!(snap.extra_rate_windows.is_empty());
    }

    #[tokio::test]
    async fn test_fetch_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(
                "/apiv2/kimi.gateway.billing.v1.BillingService/GetUsages",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_raw(SAMPLE, "application/json"))
            .mount(&server)
            .await;
        let provider = KimiProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("token".into(), "jwt".into());
        let snap = provider.fetch_usage(&ctx).await.unwrap();
        assert_eq!(snap.primary_rate_window.unwrap().used, Some(15));
        assert_eq!(snap.secondary_rate_window.unwrap().used, Some(43));
        assert_eq!(snap.tertiary_rate_window.unwrap().used, Some(30));
    }

    #[tokio::test]
    async fn test_fetch_401() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(
                "/apiv2/kimi.gateway.billing.v1.BillingService/GetUsages",
            ))
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
