//! Provider for OpenCode Go via the official Zen usage endpoint.
//!
//! Setup: the provider calls `GET https://opencode.ai/zen/go/v1/usage` with
//! the OpenCode Go API key as `Authorization: Bearer <key>` and maps the
//! account-wide `rolling` (5h), `weekly`, and optional `monthly` windows —
//! the same used percents the OpenCode dashboard shows. The key can be set
//! explicitly (`opencode-go set token <key>`) or auto-detected from
//! `~/.local/share/opencode/auth.json` (the `opencode-go` entry, falling back
//! to the `opencode` entry).
//! See `docs/providers/opencode-go.md` for the full spec.

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::error::SpendPanelError;
use crate::model::{RateWindow, RateWindowStatus, UsageSnapshot};
use crate::provider::{ProviderContext, ProviderMetadata, UsageProvider};

const DEFAULT_BASE: &str = "https://opencode.ai";
const USAGE_PATH: &str = "/zen/go/v1/usage";
/// Environment variable holding the OpenCode Go API key (same name CodexBar uses).
const API_KEY_ENV: &str = "OPENCODE_API_KEY";
/// File holding the desktop login, whose key entries work as Bearer keys for
/// the usage endpoint: `$XDG_DATA_HOME/opencode/auth.json`, falling back to
/// `~/.local/share/opencode/auth.json`.
const AUTH_FILE_REL: &str = "opencode/auth.json";
const AUTH_FILE_FALLBACK_REL: &str = ".local/share/opencode/auth.json";
/// Login-file entries tried in order when no explicit key is configured: the
/// Go key first, then the main Console key (valid for the endpoint, bound to
/// whatever subscription that login holds).
const AUTH_FILE_ENTRIES: &[&str] = &["opencode-go", "opencode"];

/// One parsed usage window from the endpoint payload.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ParsedWindow {
    /// 0–100 (used percent, as the dashboard shows it).
    percent: f64,
    /// Server-reported reset time.
    resets_at: Option<DateTime<Utc>>,
    /// True when the server reports the window as `rate-limited`.
    limited: bool,
}

pub struct OpenCodeGoProvider {
    metadata: ProviderMetadata,
    /// Base URL override for tests.
    base_url: Option<String>,
}

impl OpenCodeGoProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: "opencode-go",
                name: "OpenCode Go",
                description: "OpenCode Go quota via the official Zen usage endpoint (API key)",
                auth_methods: &["api_key"],
                website: Some("https://opencode.ai"),
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
        self.base_url.as_deref().unwrap_or(DEFAULT_BASE)
    }

    fn build_client(ctx: &ProviderContext) -> Result<reqwest::Client, SpendPanelError> {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(ctx.timeout_secs))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))
    }

    /// API key from the `token`/`api_key` config field, the `OPENCODE_API_KEY`
    /// env var, or the desktop login file. A pasted `Bearer <key>` value is
    /// accepted and stripped. Legacy cookie values from the pre-endpoint
    /// setup are never sent: they are skipped, and when nothing else
    /// resolves, the error tells the user to configure an API key instead.
    fn resolve_api_key(ctx: &ProviderContext) -> Result<String, SpendPanelError> {
        let mut saw_legacy_cookie = false;
        for field in ["token", "api_key"] {
            if let Some(raw) = ctx.config.get(field) {
                if looks_like_cookie(raw) {
                    saw_legacy_cookie = true;
                    continue;
                }
                if let Some(key) = clean_api_key(raw) {
                    return Ok(key);
                }
            }
        }
        if let Some(raw) = std::env::var_os(API_KEY_ENV).and_then(|v| v.into_string().ok())
            && let Some(key) = clean_api_key(&raw)
        {
            return Ok(key);
        }
        if let Some(key) = Self::auth_file_key() {
            return Ok(key);
        }
        Err(SpendPanelError::AuthFailed(
            "opencode-go".into(),
            if saw_legacy_cookie {
                "the configured token is a legacy dashboard cookie, which the usage endpoint rejects; run `usage-monitor-cli opencode-go set token \"<API key>\"` or sign in with opencode so ~/.local/share/opencode/auth.json holds a key (see docs/providers/opencode-go.md)".into()
            } else {
                "no API key configured; run `usage-monitor-cli opencode-go set token \"<key>\"` or sign in with opencode so ~/.local/share/opencode/auth.json holds an opencode-go key (see docs/providers/opencode-go.md)".into()
            },
        ))
    }

    /// Reads the first usable key from the desktop login file, if present.
    fn auth_file_key() -> Option<String> {
        let path = auth_file_path()?;
        let raw = std::fs::read_to_string(path).ok()?;
        let json: serde_json::Value = serde_json::from_str(&raw).ok()?;
        AUTH_FILE_ENTRIES
            .iter()
            .filter_map(|entry| json.get(entry)?.get("key")?.as_str())
            .filter_map(clean_api_key)
            .next()
    }

    fn rate_window(label: String, window_minutes: u32, w: &ParsedWindow) -> RateWindow {
        let ratio = (w.percent / 100.0).clamp(0.0, 1.0);
        RateWindow {
            label,
            window_minutes,
            usage_ratio: ratio,
            limit: None,
            used: None,
            remaining: None,
            resets_at: w.resets_at,
            status: if w.limited {
                RateWindowStatus::Exhausted
            } else {
                RateWindowStatus::from_ratio(ratio)
            },
        }
    }

    fn snapshot_from_windows(
        rolling: &ParsedWindow,
        weekly: &ParsedWindow,
        monthly: Option<&ParsedWindow>,
    ) -> UsageSnapshot {
        let mut snapshot = UsageSnapshot::new("opencode-go");
        snapshot.collected_at = Utc::now();
        snapshot.primary_rate_window = Some(Self::rate_window("Rolling (5h)".into(), 300, rolling));
        snapshot.secondary_rate_window = Some(Self::rate_window("Weekly".into(), 10_080, weekly));
        if let Some(monthly) = monthly {
            snapshot.tertiary_rate_window =
                Some(Self::rate_window("Monthly".into(), 43_200, monthly));
        }
        snapshot
    }
}

impl Default for OpenCodeGoProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// Login-file location: `$XDG_DATA_HOME/opencode/auth.json`, falling back to
/// `~/.local/share/opencode/auth.json` when `XDG_DATA_HOME` is unset/empty.
fn auth_file_path() -> Option<std::path::PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME")
        && !xdg.is_empty()
    {
        return Some(std::path::PathBuf::from(xdg).join(AUTH_FILE_REL));
    }
    std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(AUTH_FILE_FALLBACK_REL))
}

/// Detects leftover values from the pre-endpoint cookie setup: a full
/// `Cookie:` header, an `auth=<value>` pair, a multi-cookie header, or a
/// bare Better-Auth session token. A lone `=` is NOT treated as a cookie so
/// base64-padded API keys keep working.
fn looks_like_cookie(raw: &str) -> bool {
    let value = raw.trim();
    let lower = value.to_lowercase();
    value.contains(';')
        || lower.starts_with("cookie:")
        || lower.starts_with("auth=")
        || value.starts_with("Fe26.")
}

/// True when an env/file value holds a usable key (trims whitespace, unlike a
/// bare emptiness check).
fn has_key_value(raw: &std::ffi::OsStr) -> bool {
    raw.to_str().is_some_and(|s| clean_api_key(s).is_some())
}

/// Trims a pasted key, accepting a leading `Bearer ` scheme the same way
/// other Bearer-token providers do. Empty values resolve to `None`.
fn clean_api_key(raw: &str) -> Option<String> {
    let value = raw.trim();
    let value = value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))
        .map(str::trim)
        .unwrap_or(value);
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// Extracts a server-provided error message (`{"error": {"message": ...}}` or
/// `{"error": "..."}`), when present.
fn server_error_message(body: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    let error = json.get("error")?;
    if let Some(message) = error.get("message").and_then(|m| m.as_str()) {
        return (!message.is_empty()).then(|| message.to_string());
    }
    error
        .as_str()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Parses one usage window (`{status, percent, resetsAt}`); returns `None`
/// when the value is missing or malformed so callers can decide whether the
/// window is required or optional.
fn parse_window(value: Option<&serde_json::Value>) -> Option<ParsedWindow> {
    let window = value?;
    let percent = window.get("percent")?.as_f64()?;
    if !percent.is_finite() || percent < 0.0 || percent > 100.0 {
        return None;
    }
    let status = window.get("status").and_then(|s| s.as_str()).unwrap_or("");
    if status != "ok" && status != "rate-limited" {
        return None;
    }
    let resets_at = window
        .get("resetsAt")
        .or_else(|| window.get("resets_at"))
        .or_else(|| window.get("renewsAt"))
        .and_then(|v| v.as_str())
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc));
    Some(ParsedWindow {
        percent,
        resets_at,
        limited: status == "rate-limited",
    })
}

#[async_trait]
impl UsageProvider for OpenCodeGoProvider {
    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    fn detect_credentials(&self) -> bool {
        if std::env::var_os(API_KEY_ENV).is_some_and(|v| has_key_value(&v)) {
            return true;
        }
        Self::auth_file_key().is_some()
    }

    async fn fetch_usage(&self, ctx: &ProviderContext) -> Result<UsageSnapshot, SpendPanelError> {
        let key = Self::resolve_api_key(ctx)?;
        let client = Self::build_client(ctx)?;
        let url = format!("{}{}", self.api_base(), USAGE_PATH);

        let resp = client
            .get(&url)
            .header("Authorization", format!("Bearer {}", key))
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;

        let status = resp.status();
        // `text()` consumes the response, so grab `retry-after` first.
        let retry_after = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.trim().parse::<u64>().ok());
        let body = resp
            .text()
            .await
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;

        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            let detail = server_error_message(&body)
                .map(|m| format!(": {}", m))
                .unwrap_or_default();
            return Err(SpendPanelError::AuthFailed(
                "opencode-go".into(),
                format!(
                    "API key rejected (HTTP {}){}; check the key or subscription at https://opencode.ai",
                    status.as_u16(),
                    detail
                ),
            ));
        }
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(SpendPanelError::RateLimited(
                "opencode-go".into(),
                retry_after,
            ));
        }
        if !status.is_success() {
            return Err(SpendPanelError::ProviderError(
                "opencode-go".into(),
                format!("usage endpoint HTTP {}", status.as_u16()),
            ));
        }

        let json: serde_json::Value = serde_json::from_str(&body)
            .map_err(|e| SpendPanelError::ParseError("opencode-go".into(), e.to_string()))?;
        let usage = json.get("usage");
        let rolling = parse_window(usage.and_then(|u| u.get("rolling")));
        let weekly = parse_window(usage.and_then(|u| u.get("weekly")));
        match (rolling, weekly) {
            (Some(rolling), Some(weekly)) => {
                let monthly = parse_window(usage.and_then(|u| u.get("monthly")));
                Ok(Self::snapshot_from_windows(
                    &rolling,
                    &weekly,
                    monthly.as_ref(),
                ))
            }
            _ => Err(SpendPanelError::ParseError(
                "opencode-go".into(),
                "response is missing rolling/weekly usage windows".into(),
            )),
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

    fn usage_payload(rolling: &str, weekly: &str, monthly: Option<&str>) -> String {
        let monthly_part = monthly
            .map(|m| format!(r#","monthly":{}"#, m))
            .unwrap_or_default();
        format!(
            r#"{{"usage":{{"rolling":{},"weekly":{}{}}}}}"#,
            rolling, weekly, monthly_part
        )
    }

    fn window(status: &str, percent: f64, resets_at: &str) -> String {
        format!(
            r#"{{"status":"{}","percent":{},"resetsAt":"{}"}}"#,
            status, percent, resets_at
        )
    }

    #[test]
    fn test_clean_api_key() {
        assert_eq!(clean_api_key("  abc123  "), Some("abc123".into()));
        assert_eq!(clean_api_key("Bearer abc123"), Some("abc123".into()));
        assert_eq!(clean_api_key("bearer abc123  "), Some("abc123".into()));
        assert_eq!(clean_api_key("   "), None);
        assert_eq!(clean_api_key(""), None);
    }

    #[test]
    fn test_resolve_api_key_token_field_preferred() {
        let mut ctx = ProviderContext::new();
        ctx.config.insert("token".into(), "key-token".into());
        ctx.config.insert("api_key".into(), "key-alias".into());
        assert_eq!(
            OpenCodeGoProvider::resolve_api_key(&ctx).unwrap(),
            "key-token"
        );

        let mut ctx = ProviderContext::new();
        ctx.config.insert("api_key".into(), "key-alias".into());
        assert_eq!(
            OpenCodeGoProvider::resolve_api_key(&ctx).unwrap(),
            "key-alias"
        );
    }

    #[test]
    fn test_looks_like_cookie() {
        for raw in [
            "auth=Fe26.abc123",
            "Fe26.abc123",
            "Cookie: auth=Fe26.abc; other=1",
            "cookie: auth=x",
            "a=b; c=d",
            "  auth=Fe26.abc  ",
        ] {
            assert!(looks_like_cookie(raw), "should detect {raw}");
        }
        for raw in [
            "sk-opengo-abc123",
            "oc_sk_live_abc123",
            "Bearer sk-opengo-abc123",
            "abc123==", // base64 padding alone is not a cookie
            "plain-key-without-separators",
        ] {
            assert!(!looks_like_cookie(raw), "should accept {raw}");
        }
    }

    #[test]
    fn test_resolve_api_key_skips_legacy_cookie_for_valid_alias() {
        // A stale cookie in `token` is skipped; the `api_key` alias still wins.
        let mut ctx = ProviderContext::new();
        ctx.config.insert("token".into(), "auth=Fe26.stale".into());
        ctx.config.insert("api_key".into(), "live-key".into());
        assert_eq!(
            OpenCodeGoProvider::resolve_api_key(&ctx).unwrap(),
            "live-key"
        );
    }

    #[test]
    fn test_has_key_value_trims_whitespace() {
        use std::ffi::OsStr;
        assert!(has_key_value(OsStr::new("k")));
        assert!(!has_key_value(OsStr::new("   ")));
        assert!(!has_key_value(OsStr::new("")));
    }

    #[test]
    fn test_resolve_api_key_strips_bearer_prefix() {
        let mut ctx = ProviderContext::new();
        ctx.config
            .insert("token".into(), "Bearer pasted-key".into());
        assert_eq!(
            OpenCodeGoProvider::resolve_api_key(&ctx).unwrap(),
            "pasted-key"
        );
    }

    #[test]
    fn test_parse_window_ok() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"status":"ok","percent":42.5,"resetsAt":"2026-09-23T00:00:00.000Z"}"#,
        )
        .unwrap();
        let w = parse_window(Some(&v)).unwrap();
        assert!((w.percent - 42.5).abs() < 1e-9);
        assert!(!w.limited);
        assert!(w.resets_at.is_some());
    }

    #[test]
    fn test_parse_window_rate_limited() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"status":"rate-limited","percent":100,"resetsAt":"2026-09-23T00:00:00Z"}"#,
        )
        .unwrap();
        let w = parse_window(Some(&v)).unwrap();
        assert!(w.limited);
    }

    #[test]
    fn test_parse_window_rejects_malformed() {
        assert!(parse_window(None).is_none());
        for raw in [
            r#"{"status":"ok","percent":140,"resetsAt":"2026-09-23T00:00:00Z"}"#,
            r#"{"status":"ok","percent":-1,"resetsAt":"2026-09-23T00:00:00Z"}"#,
            r#"{"status":"weird","percent":10,"resetsAt":"2026-09-23T00:00:00Z"}"#,
            r#"{"status":"ok","resetsAt":"2026-09-23T00:00:00Z"}"#,
            r#"{"status":"ok","percent":10}"#,
        ] {
            // Missing resetsAt is fine (reset time unknown); everything else
            // must be rejected.
            let v: serde_json::Value = serde_json::from_str(raw).unwrap();
            let parsed = parse_window(Some(&v));
            if raw.contains("\"percent\":140")
                || raw.contains("\"percent\":-1")
                || raw.contains("\"status\":\"weird\"")
                || !raw.contains("percent")
            {
                assert!(parsed.is_none(), "should reject {raw}");
            } else {
                assert!(parsed.is_some(), "should accept {raw}");
            }
        }
    }

    #[test]
    fn test_server_error_message() {
        assert_eq!(
            server_error_message(r#"{"error":{"message":"EntitlementError"}}"#).as_deref(),
            Some("EntitlementError")
        );
        assert_eq!(
            server_error_message(r#"{"error":"boom"}"#).as_deref(),
            Some("boom")
        );
        assert!(server_error_message(r#"{"ok":true}"#).is_none());
        assert!(server_error_message("not json").is_none());
    }

    #[test]
    fn test_provider_metadata() {
        let p = OpenCodeGoProvider::new();
        assert_eq!(p.metadata().id, "opencode-go");
        assert!(p.metadata().auth_methods.contains(&"api_key"));
    }

    #[tokio::test]
    async fn test_fetch_maps_windows() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/zen/go/v1/usage"))
            .and(header("Authorization", "Bearer test-key"))
            .respond_with(ResponseTemplate::new(200).set_body_string(usage_payload(
                &window("ok", 10.0, "2026-09-23T05:00:00.000Z"),
                &window("ok", 50.0, "2026-09-29T00:00:00.000Z"),
                Some(&window("ok", 5.0, "2026-10-23T00:00:00.000Z")),
            )))
            .mount(&server)
            .await;

        let provider = OpenCodeGoProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("token".into(), "test-key".into());

        let snap = provider.fetch_usage(&ctx).await.unwrap();
        assert_eq!(snap.provider_id, "opencode-go");

        let primary = snap.primary_rate_window.unwrap();
        assert_eq!(primary.label, "Rolling (5h)");
        assert_eq!(primary.window_minutes, 300);
        assert!((primary.usage_ratio - 0.10).abs() < 1e-9);
        assert_eq!(primary.status, RateWindowStatus::Normal);

        let secondary = snap.secondary_rate_window.unwrap();
        assert_eq!(secondary.label, "Weekly");
        assert!((secondary.usage_ratio - 0.50).abs() < 1e-9);

        let tertiary = snap.tertiary_rate_window.unwrap();
        assert_eq!(tertiary.label, "Monthly");
        assert!((tertiary.usage_ratio - 0.05).abs() < 1e-9);
        assert!(snap.extra_rate_windows.is_empty());
    }

    #[tokio::test]
    async fn test_fetch_without_monthly() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/zen/go/v1/usage"))
            .respond_with(ResponseTemplate::new(200).set_body_string(usage_payload(
                &window("ok", 20.0, "2026-09-23T05:00:00.000Z"),
                &window("ok", 95.0, "2026-09-29T00:00:00.000Z"),
                None,
            )))
            .mount(&server)
            .await;

        let provider = OpenCodeGoProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("api_key".into(), "k".into());

        let snap = provider.fetch_usage(&ctx).await.unwrap();
        assert!(snap.tertiary_rate_window.is_none());
        assert_eq!(
            snap.secondary_rate_window.unwrap().status,
            RateWindowStatus::Critical
        );
    }

    #[tokio::test]
    async fn test_fetch_rate_limited_window_is_exhausted() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/zen/go/v1/usage"))
            .respond_with(ResponseTemplate::new(200).set_body_string(usage_payload(
                &window("rate-limited", 67.0, "2026-09-23T05:00:00.000Z"),
                &window("ok", 10.0, "2026-09-29T00:00:00.000Z"),
                None,
            )))
            .mount(&server)
            .await;

        let provider = OpenCodeGoProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("token".into(), "k".into());

        let snap = provider.fetch_usage(&ctx).await.unwrap();
        assert_eq!(
            snap.primary_rate_window.unwrap().status,
            RateWindowStatus::Exhausted
        );
    }

    #[tokio::test]
    async fn test_fetch_401_is_auth_failed() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/zen/go/v1/usage"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let provider = OpenCodeGoProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("token".into(), "bad".into());

        let err = provider.fetch_usage(&ctx).await.unwrap_err();
        assert!(matches!(err, SpendPanelError::AuthFailed(_, _)));
        assert!(err.to_string().contains("401"));
    }

    #[tokio::test]
    async fn test_fetch_403_includes_server_message() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/zen/go/v1/usage"))
            .respond_with(
                ResponseTemplate::new(403)
                    .set_body_string(r#"{"error":{"message":"EntitlementError"}}"#),
            )
            .mount(&server)
            .await;

        let provider = OpenCodeGoProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("token".into(), "no-sub".into());

        let err = provider.fetch_usage(&ctx).await.unwrap_err();
        assert!(matches!(err, SpendPanelError::AuthFailed(_, _)));
        assert!(err.to_string().contains("EntitlementError"));
    }

    #[tokio::test]
    async fn test_fetch_missing_windows_is_parse_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/zen/go/v1/usage"))
            .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"usage":{}}"#))
            .mount(&server)
            .await;

        let provider = OpenCodeGoProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("token".into(), "k".into());

        let err = provider.fetch_usage(&ctx).await.unwrap_err();
        assert!(matches!(err, SpendPanelError::ParseError(_, _)));
    }

    #[tokio::test]
    async fn test_fetch_server_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/zen/go/v1/usage"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let provider = OpenCodeGoProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("token".into(), "k".into());

        let err = provider.fetch_usage(&ctx).await.unwrap_err();
        assert!(matches!(err, SpendPanelError::ProviderError(_, _)));
    }

    #[tokio::test]
    async fn test_fetch_429_is_rate_limited_with_retry_after() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/zen/go/v1/usage"))
            .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "120"))
            .mount(&server)
            .await;

        let provider = OpenCodeGoProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("token".into(), "k".into());

        let err = provider.fetch_usage(&ctx).await.unwrap_err();
        assert!(
            matches!(err, SpendPanelError::RateLimited(_, Some(120))),
            "got: {err}"
        );
    }

    #[tokio::test]
    async fn test_fetch_429_without_retry_after() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/zen/go/v1/usage"))
            .respond_with(ResponseTemplate::new(429))
            .mount(&server)
            .await;

        let provider = OpenCodeGoProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("token".into(), "k".into());

        let err = provider.fetch_usage(&ctx).await.unwrap_err();
        assert!(
            matches!(err, SpendPanelError::RateLimited(_, None)),
            "got: {err}"
        );
    }
}
