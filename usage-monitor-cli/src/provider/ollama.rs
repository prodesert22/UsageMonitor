//! Ollama cloud usage provider (ollama.com/settings, browser cookie auth).
//!
//! Ports CodexBar's scrape of the settings page: it reads the "Session usage"
//! (or "Hourly usage") and "Weekly usage" percentages plus the plan name.

use async_trait::async_trait;

use crate::error::SpendPanelError;
use crate::model::{PlanInfo, RateWindow, UsageSnapshot};
use crate::provider::{ProviderContext, ProviderMetadata, UsageProvider};

/// Finds the first `NN%` (allowing decimals) appearing after `label` in `html`.
fn percent_after(html: &str, label: &str) -> Option<f64> {
    let start = html.find(label)? + label.len();
    // Bound the search window so a later block's percent isn't misattributed.
    let end = (start + 800).min(html.len());
    let window = &html[start..end];
    let bytes = window.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            // Walk back over an optional number immediately before '%'.
            let mut j = i;
            while j > 0 {
                let c = bytes[j - 1];
                if c.is_ascii_digit() || c == b'.' {
                    j -= 1;
                } else {
                    break;
                }
            }
            let parsed = (j < i).then(|| window[j..i].parse::<f64>().ok()).flatten();
            if let Some(value) = parsed {
                return Some(value.clamp(0.0, 100.0));
            }
        }
        i += 1;
    }
    None
}

/// Extracts the inner text of the first `<span>…</span>` after `anchor`.
fn span_after(html: &str, anchor: &str) -> Option<String> {
    let start = html.find(anchor)? + anchor.len();
    let rest = &html[start..];
    let open = rest.find("<span")?;
    let after_open = &rest[open..];
    let gt = after_open.find('>')? + 1;
    let inner = &after_open[gt..];
    let close = inner.find('<')?;
    let text = inner[..close].trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

/// Ollama cloud usage provider.
pub struct OllamaProvider {
    metadata: ProviderMetadata,
    base_url: Option<String>,
}

impl OllamaProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: "ollama",
                name: "Ollama",
                description: "Ollama cloud session/weekly usage monitor (browser cookie)",
                auth_methods: &["cookie", "env"],
                website: Some("https://ollama.com"),
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
        self.base_url.as_deref().unwrap_or("https://ollama.com")
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
        if let Ok(v) = std::env::var("OLLAMA_COOKIE") {
            let c = Self::clean(&v);
            if !c.is_empty() {
                return Ok(c);
            }
        }
        Err(SpendPanelError::AuthFailed(
            "ollama".into(),
            "no session cookie in cookie config or OLLAMA_COOKIE".into(),
        ))
    }

    fn build_client(ctx: &ProviderContext) -> Result<reqwest::Client, SpendPanelError> {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(ctx.timeout_secs))
            .build()
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))
    }

    fn parse(html: &str) -> Result<UsageSnapshot, SpendPanelError> {
        let session =
            percent_after(html, "Session usage").or_else(|| percent_after(html, "Hourly usage"));
        let weekly = percent_after(html, "Weekly usage");

        if session.is_none() && weekly.is_none() {
            if html.contains("Sign in") || html.contains("sign in") {
                return Err(SpendPanelError::AuthFailed(
                    "ollama".into(),
                    "not logged in to ollama.com (session cookie missing/expired)".into(),
                ));
            }
            return Err(SpendPanelError::ParseError(
                "ollama".into(),
                "no usage data found on settings page".into(),
            ));
        }

        let mut snapshot = UsageSnapshot::new("ollama");
        if let Some(s) = session {
            snapshot.primary_rate_window =
                Some(RateWindow::new(s.round() as u64, 100, "Session", 5 * 60));
        }
        if let Some(w) = weekly {
            snapshot.secondary_rate_window = Some(RateWindow::new(
                w.round() as u64,
                100,
                "Weekly",
                7 * 24 * 60,
            ));
        }
        if let Some(plan) = span_after(html, "Cloud Usage") {
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

impl Default for OllamaProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl UsageProvider for OllamaProvider {
    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    fn detect_credentials(&self) -> bool {
        std::env::var("OLLAMA_COOKIE")
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false)
    }

    async fn fetch_usage(&self, ctx: &ProviderContext) -> Result<UsageSnapshot, SpendPanelError> {
        let cookie = Self::resolve_cookie(ctx)?;
        let client = Self::build_client(ctx)?;
        let url = format!("{}/settings", self.api_base().trim_end_matches('/'));
        let resp = client
            .get(url)
            .header("Cookie", cookie)
            .header("Accept", "text/html")
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
                "ollama".into(),
                format!("session cookie rejected (HTTP {})", status.as_u16()),
            ));
        }
        if !status.is_success() {
            return Err(SpendPanelError::ProviderError(
                "ollama".into(),
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

    const HTML: &str = r#"<html><body>
      <div>Cloud Usage <span class="x">Pro</span></div>
      <div>Session usage <div class="bar">42%</div> resets soon</div>
      <div>Weekly usage <div class="bar">7.5%</div></div>
    </body></html>"#;

    #[test]
    fn test_metadata() {
        assert_eq!(OllamaProvider::new().metadata().id, "ollama");
    }

    #[test]
    fn test_percent_after() {
        assert_eq!(percent_after(HTML, "Session usage"), Some(42.0));
        assert_eq!(percent_after(HTML, "Weekly usage"), Some(7.5));
    }

    #[test]
    fn test_parse() {
        let snap = OllamaProvider::parse(HTML).unwrap();
        assert_eq!(snap.primary_rate_window.unwrap().used, Some(42));
        assert_eq!(snap.secondary_rate_window.unwrap().used, Some(8));
        assert_eq!(snap.plan.unwrap().name, "Pro");
    }

    #[test]
    fn test_parse_signed_out() {
        let err = OllamaProvider::parse("<html>Please Sign in</html>").unwrap_err();
        assert!(matches!(err, SpendPanelError::AuthFailed(_, _)));
    }

    #[tokio::test]
    async fn test_fetch_success() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/settings"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(HTML, "text/html"))
            .mount(&server)
            .await;
        let provider = OllamaProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("cookie".into(), "sid=abc".into());
        let snap = provider.fetch_usage(&ctx).await.unwrap();
        assert_eq!(snap.primary_rate_window.unwrap().used, Some(42));
    }

    #[tokio::test]
    async fn test_fetch_401() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/settings"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let provider = OllamaProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("cookie".into(), "bad".into());
        assert!(matches!(
            provider.fetch_usage(&ctx).await.unwrap_err(),
            SpendPanelError::AuthFailed(_, _)
        ));
    }
}
