//! Provider for Claude subscriptions (Pro/Max) via Claude Code OAuth credentials.
//!
//! Unlike the `anthropic` provider (Admin API with api_key), this provider reads
//! the OAuth credentials written by the Claude Code CLI to `~/.claude/.credentials.json`
//! and queries the `/api/oauth/usage` endpoint to obtain the subscription rate limit
//! windows (5h session, weekly, weekly per model).

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::error::SpendPanelError;
use crate::model::{CreditsSnapshot, NamedRateWindow, PlanInfo, RateWindow, RateWindowStatus, UsageSnapshot};
use crate::provider::{ProviderContext, ProviderMetadata, UsageProvider};

/// Public OAuth client ID of the Claude Code CLI (not a secret).
const OAUTH_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
/// Beta header required by the OAuth usage endpoint.
const OAUTH_BETA_HEADER: &str = "oauth-2025-04-20";
/// User-Agent mimicking Claude Code (the endpoint expects a known client).
const USER_AGENT: &str = "claude-code/2.1.0";

const DEFAULT_API_BASE: &str = "https://api.anthropic.com";
const DEFAULT_TOKEN_BASE: &str = "https://platform.claude.com";

// ---------------------------------------------------------------------------
// OAuth credentials (Claude Code format: ~/.claude/.credentials.json)
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
struct CredentialsFile {
    #[serde(rename = "claudeAiOauth")]
    claude_ai_oauth: Option<OAuthSection>,
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
struct OAuthSection {
    #[serde(rename = "accessToken")]
    access_token: Option<String>,
    #[serde(rename = "refreshToken")]
    refresh_token: Option<String>,
    /// Epoch in milliseconds.
    #[serde(rename = "expiresAt")]
    expires_at: Option<f64>,
    #[serde(rename = "subscriptionType")]
    subscription_type: Option<String>,
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}

/// Claude Code OAuth credentials, loaded from disk.
#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeOAuthCredentials {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub subscription_type: Option<String>,
}

impl ClaudeOAuthCredentials {
    /// Default path of the Claude Code credentials on Linux.
    pub fn default_path() -> Option<PathBuf> {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".claude/.credentials.json"))
    }

    /// Loads and validates credentials from a file.
    pub fn load_from_path(path: &Path) -> Result<Self, SpendPanelError> {
        let raw = std::fs::read_to_string(path).map_err(|e| {
            SpendPanelError::AuthFailed(
                "claude".into(),
                format!("cannot read credentials at {}: {}", path.display(), e),
            )
        })?;
        Self::parse(&raw)
    }

    /// Parses the Claude Code credentials JSON.
    pub fn parse(raw: &str) -> Result<Self, SpendPanelError> {
        let file: CredentialsFile = serde_json::from_str(raw)
            .map_err(|e| SpendPanelError::ParseError("claude".into(), format!("credentials: {}", e)))?;
        let oauth = file.claude_ai_oauth.ok_or_else(|| {
            SpendPanelError::AuthFailed("claude".into(), "no claudeAiOauth section in credentials".into())
        })?;
        let access_token = oauth.access_token.unwrap_or_default().trim().to_string();
        if access_token.is_empty() {
            return Err(SpendPanelError::AuthFailed(
                "claude".into(),
                "empty access token in credentials".into(),
            ));
        }
        Ok(Self {
            access_token,
            refresh_token: oauth.refresh_token,
            expires_at: oauth.expires_at.and_then(millis_to_datetime),
            subscription_type: oauth.subscription_type,
        })
    }

    /// Is the token expired? Missing expiry info is treated as still valid.
    pub fn is_expired(&self) -> bool {
        match self.expires_at {
            Some(at) => Utc::now() >= at,
            None => false,
        }
    }
}

fn millis_to_datetime(millis: f64) -> Option<DateTime<Utc>> {
    DateTime::<Utc>::from_timestamp_millis(millis as i64)
}

// ---------------------------------------------------------------------------
// Response types for the /api/oauth/usage endpoint
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize, Debug, Default)]
struct OAuthUsageWindow {
    /// Usage percentage (0–100).
    utilization: Option<f64>,
    /// ISO-8601 timestamp of the next reset.
    resets_at: Option<String>,
}

#[derive(serde::Deserialize, Debug, Default)]
struct OAuthExtraUsage {
    is_enabled: Option<bool>,
    monthly_limit: Option<f64>,
    used_credits: Option<f64>,
    currency: Option<String>,
}

#[derive(serde::Deserialize, Debug, Default)]
struct OAuthUsageResponse {
    five_hour: Option<OAuthUsageWindow>,
    seven_day: Option<OAuthUsageWindow>,
    seven_day_opus: Option<OAuthUsageWindow>,
    seven_day_sonnet: Option<OAuthUsageWindow>,
    extra_usage: Option<OAuthExtraUsage>,
}

#[derive(serde::Deserialize, Debug)]
struct TokenRefreshResponse {
    access_token: String,
    refresh_token: Option<String>,
    /// Seconds until expiry.
    expires_in: Option<f64>,
}

// ---------------------------------------------------------------------------
// Provider
// ---------------------------------------------------------------------------

pub struct ClaudeProvider {
    metadata: ProviderMetadata,
    /// API base URL override for tests.
    api_base: Option<String>,
    /// Refresh endpoint base URL override for tests.
    token_base: Option<String>,
}

impl ClaudeProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: "claude",
                name: "Claude (subscription)",
                description: "Claude Pro/Max subscription usage monitor via Claude Code OAuth",
                auth_methods: &["oauth", "cli"],
                website: Some("https://claude.ai"),
            },
            api_base: None,
            token_base: None,
        }
    }

    /// Creates a provider with custom base URLs (for tests).
    pub fn with_base_urls(api_base: &str, token_base: &str) -> Self {
        let mut p = Self::new();
        p.api_base = Some(api_base.to_string());
        p.token_base = Some(token_base.to_string());
        p
    }

    fn api_base(&self) -> &str {
        self.api_base.as_deref().unwrap_or(DEFAULT_API_BASE)
    }

    fn token_base(&self) -> &str {
        self.token_base.as_deref().unwrap_or(DEFAULT_TOKEN_BASE)
    }

    /// Detection helper: credentials exist at the given path.
    fn detect_credentials_at(path: Option<&Path>) -> bool {
        path.is_some_and(|p| p.exists())
    }

    fn credentials_path(ctx: &ProviderContext) -> Result<PathBuf, SpendPanelError> {
        if let Some(p) = ctx.config.get("credentials_path") {
            return Ok(PathBuf::from(p));
        }
        ClaudeOAuthCredentials::default_path().ok_or_else(|| {
            SpendPanelError::ConfigError("cannot resolve HOME for claude credentials".into())
        })
    }

    fn build_client(ctx: &ProviderContext) -> Result<reqwest::Client, SpendPanelError> {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(ctx.timeout_secs))
            .build()
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))
    }

    /// Refreshes the access token via the refresh token and persists it back to
    /// the file, preserving fields we do not know about.
    async fn refresh_token(
        token_base: &str,
        client: &reqwest::Client,
        creds: &ClaudeOAuthCredentials,
        persist_path: Option<&Path>,
    ) -> Result<ClaudeOAuthCredentials, SpendPanelError> {
        let refresh_token = creds.refresh_token.as_deref().ok_or_else(|| {
            SpendPanelError::AuthFailed(
                "claude".into(),
                "access token expired and no refresh token available; run `claude` to re-authenticate".into(),
            )
        })?;

        let url = format!("{}/v1/oauth/token", token_base);
        let resp = client
            .post(&url)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
                ("client_id", OAUTH_CLIENT_ID),
            ])
            .send()
            .await
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;

        let status = resp.status();
        let body = resp.text().await.map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;
        if !status.is_success() {
            return Err(SpendPanelError::AuthFailed(
                "claude".into(),
                format!("token refresh failed (HTTP {}): {}", status, body),
            ));
        }

        let token: TokenRefreshResponse = serde_json::from_str(&body)
            .map_err(|e| SpendPanelError::ParseError("claude".into(), format!("token refresh: {}", e)))?;

        let expires_at = token
            .expires_in
            .map(|secs| Utc::now() + chrono::Duration::milliseconds((secs * 1000.0) as i64));

        let refreshed = ClaudeOAuthCredentials {
            access_token: token.access_token,
            refresh_token: token.refresh_token.or_else(|| creds.refresh_token.clone()),
            expires_at,
            subscription_type: creds.subscription_type.clone(),
        };

        if let Some(path) = persist_path {
            if let Err(e) = Self::persist_credentials(path, &refreshed) {
                tracing::warn!("failed to persist refreshed claude credentials: {}", e);
            }
        }

        Ok(refreshed)
    }

    /// Rewrites the credentials file with the refreshed token, keeping extra fields.
    fn persist_credentials(path: &Path, creds: &ClaudeOAuthCredentials) -> Result<(), SpendPanelError> {
        let raw = std::fs::read_to_string(path)
            .map_err(|e| SpendPanelError::ConfigError(format!("read credentials: {}", e)))?;
        let mut file: CredentialsFile = serde_json::from_str(&raw)
            .map_err(|e| SpendPanelError::ParseError("claude".into(), format!("credentials: {}", e)))?;

        let mut section = file.claude_ai_oauth.take().unwrap_or(OAuthSection {
            access_token: None,
            refresh_token: None,
            expires_at: None,
            subscription_type: None,
            extra: serde_json::Map::new(),
        });
        section.access_token = Some(creds.access_token.clone());
        section.refresh_token = creds.refresh_token.clone();
        section.expires_at = creds.expires_at.map(|at| at.timestamp_millis() as f64);
        section.subscription_type = creds.subscription_type.clone();
        file.claude_ai_oauth = Some(section);

        let serialized = serde_json::to_string(&file)
            .map_err(|e| SpendPanelError::ParseError("claude".into(), format!("credentials: {}", e)))?;
        std::fs::write(path, serialized)
            .map_err(|e| SpendPanelError::ConfigError(format!("write credentials: {}", e)))
    }

    /// GET /api/oauth/usage with the subscription Bearer token.
    async fn fetch_oauth_usage(
        api_base: &str,
        client: &reqwest::Client,
        access_token: &str,
    ) -> Result<OAuthUsageResponse, SpendPanelError> {
        let url = format!("{}/api/oauth/usage", api_base);
        let resp = client
            .get(&url)
            .header("authorization", format!("Bearer {}", access_token))
            .header("anthropic-beta", OAUTH_BETA_HEADER)
            .header("accept", "application/json")
            .header("content-type", "application/json")
            .header("user-agent", USER_AGENT)
            .send()
            .await
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;

        let status = resp.status();
        if status == 401 {
            return Err(SpendPanelError::AuthFailed(
                "claude".into(),
                "OAuth token rejected; run `claude` to re-authenticate".into(),
            ));
        }
        if status == 429 {
            let retry_after = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.trim().parse::<u64>().ok());
            return Err(SpendPanelError::RateLimited("claude".into(), retry_after));
        }

        let body = resp.text().await.map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;
        if !status.is_success() {
            return Err(SpendPanelError::ProviderError(
                "claude".into(),
                format!("HTTP {}: {}", status, body),
            ));
        }

        serde_json::from_str(&body)
            .map_err(|e| SpendPanelError::ParseError("claude".into(), format!("oauth usage: {}", e)))
    }

    fn rate_window(label: &str, window_minutes: u32, w: &OAuthUsageWindow) -> RateWindow {
        let ratio = (w.utilization.unwrap_or(0.0) / 100.0).clamp(0.0, 1.0);
        RateWindow {
            label: label.into(),
            window_minutes,
            usage_ratio: ratio,
            limit: None,
            used: None,
            remaining: None,
            resets_at: w
                .resets_at
                .as_deref()
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|d| d.with_timezone(&Utc)),
            status: RateWindowStatus::from_ratio(ratio),
        }
    }

    fn plan_from_subscription(subscription_type: Option<&str>) -> PlanInfo {
        let name = match subscription_type {
            Some("pro") => "Claude Pro".to_string(),
            Some("max") => "Claude Max".to_string(),
            Some("team") => "Claude Team".to_string(),
            Some("enterprise") => "Claude Enterprise".to_string(),
            Some(other) => format!("Claude ({})", other),
            None => "Claude".to_string(),
        };
        PlanInfo {
            name,
            tier: subscription_type.map(|s| s.to_string()),
            features: vec![],
            price: None,
            currency: None,
            billing_period: Some("monthly".into()),
        }
    }

    fn snapshot_from_usage(usage: &OAuthUsageResponse, creds: &ClaudeOAuthCredentials) -> UsageSnapshot {
        let mut snapshot = UsageSnapshot::new("claude");
        snapshot.collected_at = Utc::now();

        if let Some(w) = &usage.five_hour {
            snapshot.primary_rate_window = Some(Self::rate_window("Session (5h)", 300, w));
        }
        if let Some(w) = &usage.seven_day {
            snapshot.secondary_rate_window = Some(Self::rate_window("Weekly (all models)", 10_080, w));
        }
        if let Some(w) = &usage.seven_day_opus {
            snapshot.extra_rate_windows.push(NamedRateWindow {
                id: "seven_day_opus".into(),
                label: "Weekly (Opus)".into(),
                window: Self::rate_window("Weekly (Opus)", 10_080, w),
            });
        }
        if let Some(w) = &usage.seven_day_sonnet {
            snapshot.extra_rate_windows.push(NamedRateWindow {
                id: "seven_day_sonnet".into(),
                label: "Weekly (Sonnet)".into(),
                window: Self::rate_window("Weekly (Sonnet)", 10_080, w),
            });
        }

        if let Some(extra) = &usage.extra_usage {
            if extra.is_enabled.unwrap_or(false) {
                let used = extra.used_credits.unwrap_or(0.0);
                let total = extra.monthly_limit;
                snapshot.credits = Some(CreditsSnapshot {
                    balance: total.map(|t| (t - used).max(0.0)).unwrap_or(0.0),
                    currency: extra.currency.clone().unwrap_or_else(|| "USD".into()),
                    total,
                    used: Some(used),
                    renews_at: None,
                    bonus: None,
                    purchased: None,
                });
            }
        }

        snapshot.plan = Some(Self::plan_from_subscription(creds.subscription_type.as_deref()));
        snapshot
    }
}

impl Default for ClaudeProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl UsageProvider for ClaudeProvider {
    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    fn detect_credentials(&self) -> bool {
        ClaudeProvider::detect_credentials_at(ClaudeOAuthCredentials::default_path().as_deref())
    }

    async fn fetch_usage(&self, ctx: &ProviderContext) -> Result<UsageSnapshot, SpendPanelError> {
        let client = Self::build_client(ctx)?;

        // A direct token via config takes precedence (useful for tests/integrations).
        let (mut creds, persist_path) = if let Some(token) = ctx.config.get("access_token") {
            (
                ClaudeOAuthCredentials {
                    access_token: token.clone(),
                    refresh_token: None,
                    expires_at: None,
                    subscription_type: ctx.config.get("subscription_type").cloned(),
                },
                None,
            )
        } else {
            let path = Self::credentials_path(ctx)?;
            (ClaudeOAuthCredentials::load_from_path(&path)?, Some(path))
        };

        if creds.is_expired() {
            creds = Self::refresh_token(self.token_base(), &client, &creds, persist_path.as_deref()).await?;
        }

        let usage = Self::fetch_oauth_usage(self.api_base(), &client, &creds.access_token).await?;
        Ok(Self::snapshot_from_usage(&usage, &creds))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn write_temp_credentials(name: &str, contents: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("usage-monitor-test-{}-{}.json", name, std::process::id()));
        std::fs::write(&path, contents).unwrap();
        path
    }

    fn credentials_json(access_token: &str, expires_at_millis: i64) -> String {
        serde_json::json!({
            "claudeAiOauth": {
                "accessToken": access_token,
                "refreshToken": "rt-test",
                "expiresAt": expires_at_millis,
                "scopes": ["user:inference", "user:profile"],
                "subscriptionType": "max"
            }
        })
        .to_string()
    }

    fn future_millis() -> i64 {
        (Utc::now() + chrono::Duration::hours(1)).timestamp_millis()
    }

    fn past_millis() -> i64 {
        (Utc::now() - chrono::Duration::hours(1)).timestamp_millis()
    }

    #[test]
    fn test_parse_credentials() {
        let creds = ClaudeOAuthCredentials::parse(&credentials_json("at-test", future_millis())).unwrap();
        assert_eq!(creds.access_token, "at-test");
        assert_eq!(creds.refresh_token.as_deref(), Some("rt-test"));
        assert_eq!(creds.subscription_type.as_deref(), Some("max"));
        assert!(!creds.is_expired());
    }

    #[test]
    fn test_parse_credentials_expired() {
        let creds = ClaudeOAuthCredentials::parse(&credentials_json("at-test", past_millis())).unwrap();
        assert!(creds.is_expired());
    }

    #[test]
    fn test_parse_credentials_missing_section() {
        let result = ClaudeOAuthCredentials::parse(r#"{"foo": 1}"#);
        assert!(matches!(result, Err(SpendPanelError::AuthFailed(_, _))));
    }

    #[test]
    fn test_parse_credentials_empty_token() {
        let result = ClaudeOAuthCredentials::parse(r#"{"claudeAiOauth": {"accessToken": "  "}}"#);
        assert!(matches!(result, Err(SpendPanelError::AuthFailed(_, _))));
    }

    #[test]
    fn test_provider_metadata() {
        let p = ClaudeProvider::new();
        let m = p.metadata();
        assert_eq!(m.id, "claude");
        assert!(m.auth_methods.contains(&"oauth"));
    }

    #[tokio::test]
    async fn test_fetch_oauth_usage_success() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/oauth/usage"))
            .and(header("authorization", "Bearer at-test"))
            .and(header("anthropic-beta", OAUTH_BETA_HEADER))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "five_hour": {"utilization": 42.0, "resets_at": "2026-06-12T20:00:00Z"},
                "seven_day": {"utilization": 81.5, "resets_at": "2026-06-15T00:00:00Z"},
                "seven_day_opus": {"utilization": 96.0, "resets_at": "2026-06-15T00:00:00Z"}
            })))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let usage = ClaudeProvider::fetch_oauth_usage(&server.uri(), &client, "at-test")
            .await
            .unwrap();
        assert_eq!(usage.five_hour.as_ref().unwrap().utilization, Some(42.0));
        assert_eq!(usage.seven_day.as_ref().unwrap().utilization, Some(81.5));
        assert!(usage.seven_day_sonnet.is_none());
    }

    #[tokio::test]
    async fn test_fetch_oauth_usage_401() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/oauth/usage"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let result = ClaudeProvider::fetch_oauth_usage(&server.uri(), &client, "bad").await;
        assert!(matches!(result, Err(SpendPanelError::AuthFailed(_, _))));
    }

    #[tokio::test]
    async fn test_fetch_oauth_usage_429() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/oauth/usage"))
            .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "120"))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let result = ClaudeProvider::fetch_oauth_usage(&server.uri(), &client, "at").await;
        assert!(matches!(result, Err(SpendPanelError::RateLimited(_, Some(120)))));
    }

    #[test]
    fn test_rate_window_mapping() {
        let w = OAuthUsageWindow {
            utilization: Some(81.5),
            resets_at: Some("2026-06-15T00:00:00Z".into()),
        };
        let rw = ClaudeProvider::rate_window("Weekly", 10_080, &w);
        assert!((rw.usage_ratio - 0.815).abs() < 1e-9);
        assert_eq!(rw.status, RateWindowStatus::Warning);
        assert!(rw.resets_at.is_some());
    }

    #[test]
    fn test_rate_window_clamps_utilization() {
        let w = OAuthUsageWindow {
            utilization: Some(140.0),
            resets_at: None,
        };
        let rw = ClaudeProvider::rate_window("Session", 300, &w);
        assert_eq!(rw.usage_ratio, 1.0);
        assert_eq!(rw.status, RateWindowStatus::Exhausted);
    }

    #[test]
    fn test_plan_from_subscription() {
        assert_eq!(ClaudeProvider::plan_from_subscription(Some("max")).name, "Claude Max");
        assert_eq!(ClaudeProvider::plan_from_subscription(Some("pro")).name, "Claude Pro");
        assert_eq!(ClaudeProvider::plan_from_subscription(None).name, "Claude");
    }

    #[tokio::test]
    async fn test_full_fetch_with_credentials_file() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/oauth/usage"))
            .and(header("authorization", "Bearer at-valid"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "five_hour": {"utilization": 30.0, "resets_at": "2026-06-12T20:00:00Z"},
                "seven_day": {"utilization": 55.0, "resets_at": "2026-06-15T00:00:00Z"},
                "seven_day_sonnet": {"utilization": 10.0, "resets_at": "2026-06-15T00:00:00Z"},
                "extra_usage": {"is_enabled": true, "monthly_limit": 50.0, "used_credits": 12.5, "currency": "USD"}
            })))
            .mount(&server)
            .await;

        let creds_path = write_temp_credentials("full-fetch", &credentials_json("at-valid", future_millis()));

        let provider = ClaudeProvider::with_base_urls(&server.uri(), &server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("credentials_path".into(), creds_path.display().to_string());

        let snap = provider.fetch_usage(&ctx).await.unwrap();
        std::fs::remove_file(&creds_path).ok();

        assert_eq!(snap.provider_id, "claude");

        let primary = snap.primary_rate_window.unwrap();
        assert!((primary.usage_ratio - 0.30).abs() < 1e-9);
        assert_eq!(primary.window_minutes, 300);

        let secondary = snap.secondary_rate_window.unwrap();
        assert!((secondary.usage_ratio - 0.55).abs() < 1e-9);

        assert_eq!(snap.extra_rate_windows.len(), 1);
        assert_eq!(snap.extra_rate_windows[0].id, "seven_day_sonnet");

        let credits = snap.credits.unwrap();
        assert_eq!(credits.total, Some(50.0));
        assert_eq!(credits.used, Some(12.5));
        assert!((credits.balance - 37.5).abs() < 1e-9);

        assert_eq!(snap.plan.unwrap().name, "Claude Max");
    }

    #[tokio::test]
    async fn test_expired_token_triggers_refresh_and_persists() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/oauth/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "at-refreshed",
                "refresh_token": "rt-rotated",
                "expires_in": 28800
            })))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/api/oauth/usage"))
            .and(header("authorization", "Bearer at-refreshed"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "five_hour": {"utilization": 5.0, "resets_at": "2026-06-12T20:00:00Z"}
            })))
            .mount(&server)
            .await;

        let creds_path = write_temp_credentials("refresh", &credentials_json("at-stale", past_millis()));

        let provider = ClaudeProvider::with_base_urls(&server.uri(), &server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("credentials_path".into(), creds_path.display().to_string());

        let snap = provider.fetch_usage(&ctx).await.unwrap();
        assert!(snap.primary_rate_window.is_some());

        // Refreshed credentials must have been persisted to the file.
        let persisted = ClaudeOAuthCredentials::load_from_path(&creds_path).unwrap();
        std::fs::remove_file(&creds_path).ok();
        assert_eq!(persisted.access_token, "at-refreshed");
        assert_eq!(persisted.refresh_token.as_deref(), Some("rt-rotated"));
        assert!(!persisted.is_expired());
        // subscriptionType preserved.
        assert_eq!(persisted.subscription_type.as_deref(), Some("max"));
    }

    #[tokio::test]
    async fn test_expired_token_without_refresh_token_fails() {
        let creds_path = write_temp_credentials(
            "no-refresh",
            &serde_json::json!({
                "claudeAiOauth": {
                    "accessToken": "at-stale",
                    "expiresAt": past_millis()
                }
            })
            .to_string(),
        );

        let provider = ClaudeProvider::new();
        let mut ctx = ProviderContext::new();
        ctx.config.insert("credentials_path".into(), creds_path.display().to_string());

        let result = provider.fetch_usage(&ctx).await;
        std::fs::remove_file(&creds_path).ok();
        assert!(matches!(result, Err(SpendPanelError::AuthFailed(_, _))));
    }

    #[test]
    fn test_detect_credentials_at() {
        let existing = write_temp_credentials("detect", &credentials_json("at", future_millis()));
        assert!(ClaudeProvider::detect_credentials_at(Some(&existing)));
        std::fs::remove_file(&existing).ok();
        assert!(!ClaudeProvider::detect_credentials_at(Some(&existing)));
        assert!(!ClaudeProvider::detect_credentials_at(None));
    }

    #[tokio::test]
    async fn test_access_token_from_config_skips_file() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/oauth/usage"))
            .and(header("authorization", "Bearer at-direct"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "five_hour": {"utilization": 1.0, "resets_at": null}
            })))
            .mount(&server)
            .await;

        let provider = ClaudeProvider::with_base_urls(&server.uri(), &server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("access_token".into(), "at-direct".into());

        let snap = provider.fetch_usage(&ctx).await.unwrap();
        assert!(snap.primary_rate_window.is_some());
    }
}
