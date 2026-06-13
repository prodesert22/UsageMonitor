//! Provider for OpenAI Codex (ChatGPT plan) via Codex CLI OAuth credentials.
//!
//! Reads the OAuth tokens written by the Codex CLI to `~/.codex/auth.json`
//! (or `$CODEX_HOME/auth.json`) and queries the ChatGPT backend usage endpoint
//! (`/backend-api/wham/usage`) for the subscription rate limit windows
//! (5h session and weekly), plan type, and credits.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::error::SpendPanelError;
use crate::model::{CreditsSnapshot, NamedRateWindow, PlanInfo, RateWindow, RateWindowStatus, UsageSnapshot};
use crate::provider::{ProviderContext, ProviderMetadata, UsageProvider};

/// Public OAuth client ID of the Codex CLI (not a secret).
const OAUTH_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";

const DEFAULT_API_BASE: &str = "https://chatgpt.com";
const DEFAULT_TOKEN_BASE: &str = "https://auth.openai.com";

// ---------------------------------------------------------------------------
// OAuth credentials (Codex CLI format: ~/.codex/auth.json)
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
struct AuthFile {
    tokens: Option<TokensSection>,
    last_refresh: Option<String>,
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
struct TokensSection {
    id_token: Option<String>,
    access_token: Option<String>,
    refresh_token: Option<String>,
    account_id: Option<String>,
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}

/// Codex CLI OAuth credentials, loaded from disk.
#[derive(Debug, Clone, PartialEq)]
pub struct CodexOAuthCredentials {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub account_id: Option<String>,
}

impl CodexOAuthCredentials {
    /// Default path of the Codex CLI credentials ($CODEX_HOME or ~/.codex).
    pub fn default_path() -> Option<PathBuf> {
        if let Some(home) = std::env::var_os("CODEX_HOME") {
            return Some(PathBuf::from(home).join("auth.json"));
        }
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".codex/auth.json"))
    }

    /// Loads and validates credentials from a file.
    pub fn load_from_path(path: &Path) -> Result<Self, SpendPanelError> {
        let raw = std::fs::read_to_string(path).map_err(|e| {
            SpendPanelError::AuthFailed(
                "codex".into(),
                format!("cannot read credentials at {}: {}", path.display(), e),
            )
        })?;
        Self::parse(&raw)
    }

    /// Parses the Codex CLI auth.json.
    pub fn parse(raw: &str) -> Result<Self, SpendPanelError> {
        let file: AuthFile = serde_json::from_str(raw)
            .map_err(|e| SpendPanelError::ParseError("codex".into(), format!("auth.json: {}", e)))?;
        let tokens = file.tokens.ok_or_else(|| {
            SpendPanelError::AuthFailed(
                "codex".into(),
                "no tokens section in auth.json; run `codex login`".into(),
            )
        })?;
        let access_token = tokens.access_token.unwrap_or_default().trim().to_string();
        if access_token.is_empty() {
            return Err(SpendPanelError::AuthFailed(
                "codex".into(),
                "empty access token in auth.json".into(),
            ));
        }
        Ok(Self {
            access_token,
            refresh_token: tokens.refresh_token,
            account_id: tokens.account_id,
        })
    }
}

// ---------------------------------------------------------------------------
// Response types for the /backend-api/wham/usage endpoint
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize, Debug, Default)]
struct WhamUsageResponse {
    plan_type: Option<String>,
    rate_limit: Option<WhamRateLimit>,
    additional_rate_limits: Option<Vec<WhamAdditionalRateLimit>>,
    credits: Option<WhamCredits>,
}

#[derive(serde::Deserialize, Debug, Default)]
struct WhamRateLimit {
    primary_window: Option<WhamWindow>,
    secondary_window: Option<WhamWindow>,
}

#[derive(serde::Deserialize, Debug, Default)]
struct WhamWindow {
    /// Usage percentage (0–100).
    used_percent: Option<f64>,
    /// Window size in seconds (e.g. 18000 = 5h, 604800 = 7d).
    limit_window_seconds: Option<u64>,
    /// Epoch seconds of the next reset.
    reset_at: Option<i64>,
}

#[derive(serde::Deserialize, Debug, Default)]
struct WhamAdditionalRateLimit {
    name: Option<String>,
    label: Option<String>,
    #[serde(alias = "rate_limit", alias = "limit")]
    window: Option<WhamWindow>,
}

#[derive(serde::Deserialize, Debug, Default)]
struct WhamCredits {
    has_credits: Option<bool>,
    /// Balance comes as a string (e.g. "0").
    balance: Option<String>,
}

#[derive(serde::Deserialize, Debug)]
struct TokenRefreshResponse {
    access_token: String,
    refresh_token: Option<String>,
    id_token: Option<String>,
}

// ---------------------------------------------------------------------------
// Provider
// ---------------------------------------------------------------------------

pub struct CodexProvider {
    metadata: ProviderMetadata,
    /// API base URL override for tests.
    api_base: Option<String>,
    /// Refresh endpoint base URL override for tests.
    token_base: Option<String>,
}

impl CodexProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: "codex",
                name: "Codex (ChatGPT)",
                description: "ChatGPT plan Codex usage monitor via Codex CLI OAuth",
                auth_methods: &["oauth", "cli"],
                website: Some("https://chatgpt.com/codex"),
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

    fn credentials_path(ctx: &ProviderContext) -> Result<PathBuf, SpendPanelError> {
        if let Some(p) = ctx.config.get("credentials_path") {
            return Ok(PathBuf::from(p));
        }
        CodexOAuthCredentials::default_path().ok_or_else(|| {
            SpendPanelError::ConfigError("cannot resolve HOME for codex credentials".into())
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
        creds: &CodexOAuthCredentials,
        persist_path: Option<&Path>,
    ) -> Result<CodexOAuthCredentials, SpendPanelError> {
        let refresh_token = creds.refresh_token.as_deref().ok_or_else(|| {
            SpendPanelError::AuthFailed(
                "codex".into(),
                "access token rejected and no refresh token available; run `codex login`".into(),
            )
        })?;

        let url = format!("{}/oauth/token", token_base);
        let resp = client
            .post(&url)
            .json(&serde_json::json!({
                "client_id": OAUTH_CLIENT_ID,
                "grant_type": "refresh_token",
                "refresh_token": refresh_token,
                "scope": "openid profile email",
            }))
            .send()
            .await
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;

        let status = resp.status();
        let body = resp.text().await.map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;
        if !status.is_success() {
            return Err(SpendPanelError::AuthFailed(
                "codex".into(),
                format!("token refresh failed (HTTP {}): {}", status, body),
            ));
        }

        let token: TokenRefreshResponse = serde_json::from_str(&body)
            .map_err(|e| SpendPanelError::ParseError("codex".into(), format!("token refresh: {}", e)))?;

        let refreshed = CodexOAuthCredentials {
            access_token: token.access_token,
            refresh_token: token.refresh_token.or_else(|| creds.refresh_token.clone()),
            account_id: creds.account_id.clone(),
        };

        if let Some(path) = persist_path {
            if let Err(e) = Self::persist_credentials(path, &refreshed, token.id_token.as_deref()) {
                tracing::warn!("failed to persist refreshed codex credentials: {}", e);
            }
        }

        Ok(refreshed)
    }

    /// Rewrites auth.json with the refreshed tokens, keeping extra fields.
    fn persist_credentials(
        path: &Path,
        creds: &CodexOAuthCredentials,
        id_token: Option<&str>,
    ) -> Result<(), SpendPanelError> {
        let raw = std::fs::read_to_string(path)
            .map_err(|e| SpendPanelError::ConfigError(format!("read auth.json: {}", e)))?;
        let mut file: AuthFile = serde_json::from_str(&raw)
            .map_err(|e| SpendPanelError::ParseError("codex".into(), format!("auth.json: {}", e)))?;

        let mut tokens = file.tokens.take().unwrap_or(TokensSection {
            id_token: None,
            access_token: None,
            refresh_token: None,
            account_id: None,
            extra: serde_json::Map::new(),
        });
        tokens.access_token = Some(creds.access_token.clone());
        tokens.refresh_token = creds.refresh_token.clone();
        if let Some(idt) = id_token {
            tokens.id_token = Some(idt.to_string());
        }
        file.tokens = Some(tokens);
        file.last_refresh = Some(Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true));

        let serialized = serde_json::to_string(&file)
            .map_err(|e| SpendPanelError::ParseError("codex".into(), format!("auth.json: {}", e)))?;
        std::fs::write(path, serialized)
            .map_err(|e| SpendPanelError::ConfigError(format!("write auth.json: {}", e)))
    }

    /// GET /backend-api/wham/usage with the ChatGPT Bearer token.
    async fn fetch_wham_usage(
        api_base: &str,
        client: &reqwest::Client,
        creds: &CodexOAuthCredentials,
    ) -> Result<WhamUsageResponse, SpendPanelError> {
        let url = format!("{}/backend-api/wham/usage", api_base);
        let mut req = client
            .get(&url)
            .header("authorization", format!("Bearer {}", creds.access_token))
            .header("accept", "application/json");
        if let Some(account_id) = &creds.account_id {
            req = req.header("chatgpt-account-id", account_id);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;

        let status = resp.status();
        if status == 401 || status == 403 {
            return Err(SpendPanelError::AuthFailed(
                "codex".into(),
                "OAuth token rejected; run `codex login` to re-authenticate".into(),
            ));
        }
        if status == 429 {
            let retry_after = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.trim().parse::<u64>().ok());
            return Err(SpendPanelError::RateLimited("codex".into(), retry_after));
        }

        let body = resp.text().await.map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;
        if !status.is_success() {
            return Err(SpendPanelError::ProviderError(
                "codex".into(),
                format!("HTTP {}: {}", status, body),
            ));
        }

        serde_json::from_str(&body)
            .map_err(|e| SpendPanelError::ParseError("codex".into(), format!("wham usage: {}", e)))
    }

    fn rate_window(label: &str, w: &WhamWindow) -> RateWindow {
        let ratio = (w.used_percent.unwrap_or(0.0) / 100.0).clamp(0.0, 1.0);
        RateWindow {
            label: label.into(),
            window_minutes: (w.limit_window_seconds.unwrap_or(0) / 60) as u32,
            usage_ratio: ratio,
            limit: None,
            used: None,
            remaining: None,
            resets_at: w.reset_at.and_then(|s| DateTime::<Utc>::from_timestamp(s, 0)),
            status: RateWindowStatus::from_ratio(ratio),
        }
    }

    /// Label for a window based on its size (5h session vs weekly).
    fn window_label(w: &WhamWindow, fallback: &str) -> String {
        match w.limit_window_seconds {
            Some(s) if s <= 6 * 3600 => format!("Session ({}h)", s / 3600),
            Some(s) if s >= 6 * 86_400 => "Weekly".to_string(),
            _ => fallback.to_string(),
        }
    }

    fn plan_from_type(plan_type: Option<&str>) -> PlanInfo {
        let name = match plan_type {
            Some("plus") => "ChatGPT Plus".to_string(),
            Some("pro") => "ChatGPT Pro".to_string(),
            Some("team") => "ChatGPT Team".to_string(),
            Some("business") => "ChatGPT Business".to_string(),
            Some("enterprise") => "ChatGPT Enterprise".to_string(),
            Some("free") => "ChatGPT Free".to_string(),
            Some(other) => format!("ChatGPT ({})", other),
            None => "ChatGPT".to_string(),
        };
        PlanInfo {
            name,
            tier: plan_type.map(|s| s.to_string()),
            features: vec![],
            price: None,
            currency: None,
            billing_period: Some("monthly".into()),
        }
    }

    fn snapshot_from_usage(usage: &WhamUsageResponse) -> UsageSnapshot {
        let mut snapshot = UsageSnapshot::new("codex");
        snapshot.collected_at = Utc::now();

        if let Some(rl) = &usage.rate_limit {
            if let Some(w) = &rl.primary_window {
                snapshot.primary_rate_window = Some(Self::rate_window(&Self::window_label(w, "Session"), w));
            }
            if let Some(w) = &rl.secondary_window {
                snapshot.secondary_rate_window = Some(Self::rate_window(&Self::window_label(w, "Weekly"), w));
            }
        }

        if let Some(extras) = &usage.additional_rate_limits {
            for extra in extras {
                let Some(w) = &extra.window else { continue };
                let label = extra
                    .label
                    .clone()
                    .or_else(|| extra.name.clone())
                    .unwrap_or_else(|| "Additional".to_string());
                snapshot.extra_rate_windows.push(NamedRateWindow {
                    id: extra.name.clone().unwrap_or_else(|| label.clone()),
                    label: label.clone(),
                    window: Self::rate_window(&label, w),
                });
            }
        }

        if let Some(credits) = &usage.credits {
            if credits.has_credits.unwrap_or(false) {
                let balance = credits
                    .balance
                    .as_deref()
                    .and_then(|b| b.parse::<f64>().ok())
                    .unwrap_or(0.0);
                snapshot.credits = Some(CreditsSnapshot::new(balance, "credits"));
            }
        }

        snapshot.plan = Some(Self::plan_from_type(usage.plan_type.as_deref()));
        snapshot
    }
}

impl Default for CodexProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl UsageProvider for CodexProvider {
    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    fn detect_credentials(&self) -> bool {
        CodexOAuthCredentials::default_path().is_some_and(|p| p.exists())
    }

    async fn fetch_usage(&self, ctx: &ProviderContext) -> Result<UsageSnapshot, SpendPanelError> {
        let client = Self::build_client(ctx)?;

        // A direct token via config takes precedence (useful for tests/integrations).
        let (creds, persist_path) = if let Some(token) = ctx.config.get("access_token") {
            (
                CodexOAuthCredentials {
                    access_token: token.clone(),
                    refresh_token: None,
                    account_id: ctx.config.get("account_id").cloned(),
                },
                None,
            )
        } else {
            let path = Self::credentials_path(ctx)?;
            (CodexOAuthCredentials::load_from_path(&path)?, Some(path))
        };

        // auth.json has no expiry field, so try the request and refresh on rejection.
        match Self::fetch_wham_usage(self.api_base(), &client, &creds).await {
            Ok(usage) => Ok(Self::snapshot_from_usage(&usage)),
            Err(SpendPanelError::AuthFailed(_, _)) if creds.refresh_token.is_some() => {
                let refreshed =
                    Self::refresh_token(self.token_base(), &client, &creds, persist_path.as_deref()).await?;
                let usage = Self::fetch_wham_usage(self.api_base(), &client, &refreshed).await?;
                Ok(Self::snapshot_from_usage(&usage))
            }
            Err(e) => Err(e),
        }
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

    fn write_temp_auth(name: &str, contents: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("usage-monitor-codex-{}-{}.json", name, std::process::id()));
        std::fs::write(&path, contents).unwrap();
        path
    }

    fn auth_json(access_token: &str) -> String {
        serde_json::json!({
            "auth_mode": "chatgpt",
            "OPENAI_API_KEY": null,
            "tokens": {
                "id_token": "idt-test",
                "access_token": access_token,
                "refresh_token": "rt-test",
                "account_id": "acc-123"
            },
            "last_refresh": "2026-06-12T10:00:00.000Z"
        })
        .to_string()
    }

    fn wham_body() -> serde_json::Value {
        serde_json::json!({
            "plan_type": "plus",
            "rate_limit": {
                "allowed": false,
                "limit_reached": true,
                "primary_window": {
                    "used_percent": 1,
                    "limit_window_seconds": 18000,
                    "reset_after_seconds": 18000,
                    "reset_at": 1781326965
                },
                "secondary_window": {
                    "used_percent": 100,
                    "limit_window_seconds": 604800,
                    "reset_after_seconds": 49494,
                    "reset_at": 1781358459
                }
            },
            "additional_rate_limits": null,
            "credits": {
                "has_credits": false,
                "unlimited": false,
                "balance": "0"
            }
        })
    }

    #[test]
    fn test_parse_auth_json() {
        let creds = CodexOAuthCredentials::parse(&auth_json("at-test")).unwrap();
        assert_eq!(creds.access_token, "at-test");
        assert_eq!(creds.refresh_token.as_deref(), Some("rt-test"));
        assert_eq!(creds.account_id.as_deref(), Some("acc-123"));
    }

    #[test]
    fn test_parse_auth_json_missing_tokens() {
        let result = CodexOAuthCredentials::parse(r#"{"OPENAI_API_KEY": "sk-x"}"#);
        assert!(matches!(result, Err(SpendPanelError::AuthFailed(_, _))));
    }

    #[test]
    fn test_parse_auth_json_empty_token() {
        let result = CodexOAuthCredentials::parse(r#"{"tokens": {"access_token": " "}}"#);
        assert!(matches!(result, Err(SpendPanelError::AuthFailed(_, _))));
    }

    #[test]
    fn test_provider_metadata() {
        let p = CodexProvider::new();
        let m = p.metadata();
        assert_eq!(m.id, "codex");
        assert!(m.auth_methods.contains(&"oauth"));
    }

    #[test]
    fn test_window_label_from_size() {
        let session = WhamWindow { used_percent: None, limit_window_seconds: Some(18000), reset_at: None };
        let weekly = WhamWindow { used_percent: None, limit_window_seconds: Some(604800), reset_at: None };
        let odd = WhamWindow { used_percent: None, limit_window_seconds: None, reset_at: None };
        assert_eq!(CodexProvider::window_label(&session, "x"), "Session (5h)");
        assert_eq!(CodexProvider::window_label(&weekly, "x"), "Weekly");
        assert_eq!(CodexProvider::window_label(&odd, "fallback"), "fallback");
    }

    #[test]
    fn test_plan_from_type() {
        assert_eq!(CodexProvider::plan_from_type(Some("plus")).name, "ChatGPT Plus");
        assert_eq!(CodexProvider::plan_from_type(Some("pro")).name, "ChatGPT Pro");
        assert_eq!(CodexProvider::plan_from_type(None).name, "ChatGPT");
    }

    #[tokio::test]
    async fn test_fetch_wham_usage_success() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/backend-api/wham/usage"))
            .and(header("authorization", "Bearer at-test"))
            .and(header("chatgpt-account-id", "acc-123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(wham_body()))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let creds = CodexOAuthCredentials {
            access_token: "at-test".into(),
            refresh_token: None,
            account_id: Some("acc-123".into()),
        };
        let usage = CodexProvider::fetch_wham_usage(&server.uri(), &client, &creds)
            .await
            .unwrap();
        assert_eq!(usage.plan_type.as_deref(), Some("plus"));
        let rl = usage.rate_limit.unwrap();
        assert_eq!(rl.primary_window.unwrap().used_percent, Some(1.0));
        assert_eq!(rl.secondary_window.unwrap().used_percent, Some(100.0));
    }

    #[tokio::test]
    async fn test_fetch_wham_usage_401() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/backend-api/wham/usage"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let creds = CodexOAuthCredentials {
            access_token: "bad".into(),
            refresh_token: None,
            account_id: None,
        };
        let result = CodexProvider::fetch_wham_usage(&server.uri(), &client, &creds).await;
        assert!(matches!(result, Err(SpendPanelError::AuthFailed(_, _))));
    }

    #[tokio::test]
    async fn test_full_fetch_with_auth_file() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/backend-api/wham/usage"))
            .and(header("authorization", "Bearer at-valid"))
            .respond_with(ResponseTemplate::new(200).set_body_json(wham_body()))
            .mount(&server)
            .await;

        let auth_path = write_temp_auth("full-fetch", &auth_json("at-valid"));

        let provider = CodexProvider::with_base_urls(&server.uri(), &server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("credentials_path".into(), auth_path.display().to_string());

        let snap = provider.fetch_usage(&ctx).await.unwrap();
        std::fs::remove_file(&auth_path).ok();

        assert_eq!(snap.provider_id, "codex");

        let primary = snap.primary_rate_window.unwrap();
        assert_eq!(primary.label, "Session (5h)");
        assert_eq!(primary.window_minutes, 300);
        assert!((primary.usage_ratio - 0.01).abs() < 1e-9);
        assert!(primary.resets_at.is_some());

        let secondary = snap.secondary_rate_window.unwrap();
        assert_eq!(secondary.label, "Weekly");
        assert_eq!(secondary.window_minutes, 10_080);
        assert_eq!(secondary.usage_ratio, 1.0);
        assert_eq!(secondary.status, RateWindowStatus::Exhausted);

        // has_credits=false → no credits snapshot.
        assert!(snap.credits.is_none());

        assert_eq!(snap.plan.unwrap().name, "ChatGPT Plus");
    }

    #[tokio::test]
    async fn test_rejected_token_triggers_refresh_and_persists() {
        let server = MockServer::start().await;

        // First request with stale token → 401; refreshed token → 200.
        Mock::given(method("GET"))
            .and(path("/backend-api/wham/usage"))
            .and(header("authorization", "Bearer at-stale"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "at-refreshed",
                "refresh_token": "rt-rotated",
                "id_token": "idt-new"
            })))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/backend-api/wham/usage"))
            .and(header("authorization", "Bearer at-refreshed"))
            .respond_with(ResponseTemplate::new(200).set_body_json(wham_body()))
            .mount(&server)
            .await;

        let auth_path = write_temp_auth("refresh", &auth_json("at-stale"));

        let provider = CodexProvider::with_base_urls(&server.uri(), &server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("credentials_path".into(), auth_path.display().to_string());

        let snap = provider.fetch_usage(&ctx).await.unwrap();
        assert!(snap.primary_rate_window.is_some());

        // Refreshed credentials must have been persisted to the file.
        let raw = std::fs::read_to_string(&auth_path).unwrap();
        std::fs::remove_file(&auth_path).ok();
        let persisted: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(persisted["tokens"]["access_token"], "at-refreshed");
        assert_eq!(persisted["tokens"]["refresh_token"], "rt-rotated");
        assert_eq!(persisted["tokens"]["id_token"], "idt-new");
        // account_id and unknown fields preserved.
        assert_eq!(persisted["tokens"]["account_id"], "acc-123");
        assert_eq!(persisted["auth_mode"], "chatgpt");
    }

    #[tokio::test]
    async fn test_rejected_token_without_refresh_token_fails() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/backend-api/wham/usage"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let provider = CodexProvider::with_base_urls(&server.uri(), &server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("access_token".into(), "at-bad".into());

        let result = provider.fetch_usage(&ctx).await;
        assert!(matches!(result, Err(SpendPanelError::AuthFailed(_, _))));
    }
}
