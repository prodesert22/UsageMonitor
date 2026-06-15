use async_trait::async_trait;

use crate::error::SpendPanelError;
use crate::model::{CreditsSnapshot, UsageSnapshot};
use crate::provider::{ProviderContext, ProviderMetadata, UsageProvider};

/// Kimi K2 credits provider (kimi-k2.ai, API-key auth).
pub struct KimiK2Provider {
    metadata: ProviderMetadata,
    base_url: Option<String>,
}

impl KimiK2Provider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: "kimik2",
                name: "Kimi K2",
                description: "Kimi K2 credits monitor (kimi-k2.ai)",
                auth_methods: &["api_key", "env"],
                website: Some("https://kimi-k2.ai"),
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
        self.base_url.as_deref().unwrap_or("https://kimi-k2.ai")
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

    fn resolve_key(ctx: &ProviderContext) -> Result<String, SpendPanelError> {
        for key in ["api_key", "token"] {
            if let Some(v) = ctx.config.get(key) {
                let c = Self::clean(v);
                if !c.is_empty() {
                    return Ok(c);
                }
            }
        }
        for env in ["KIMI_K2_API_KEY", "KIMIK2_API_KEY"] {
            if let Ok(v) = std::env::var(env) {
                let c = Self::clean(&v);
                if !c.is_empty() {
                    return Ok(c);
                }
            }
        }
        Err(SpendPanelError::AuthFailed(
            "kimik2".into(),
            "no API key in api_key/token config, KIMI_K2_API_KEY, or KIMIK2_API_KEY".into(),
        ))
    }

    fn build_client(ctx: &ProviderContext) -> Result<reqwest::Client, SpendPanelError> {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(ctx.timeout_secs))
            .build()
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))
    }

    /// Extracts a number from any of the candidate dotted paths, searching the
    /// root plus the common `data`/`result`/`usage`/`credits` wrappers.
    fn find_number(root: &serde_json::Value, paths: &[&[&str]]) -> Option<f64> {
        let mut contexts: Vec<&serde_json::Value> = vec![root];
        for key in ["data", "result", "usage", "credits"] {
            if let Some(v) = root.get(key) {
                contexts.push(v);
                // Descend one more level so `data.credits` / `result.usage` etc. resolve.
                for nested in ["usage", "credits"] {
                    if let Some(n) = v.get(nested) {
                        contexts.push(n);
                    }
                }
            }
        }
        for path in paths {
            for ctx in &contexts {
                let mut cursor = *ctx;
                let mut ok = true;
                for key in *path {
                    match cursor.get(key) {
                        Some(next) => cursor = next,
                        None => {
                            ok = false;
                            break;
                        }
                    }
                }
                let value = ok
                    .then(|| {
                        cursor
                            .as_f64()
                            .or_else(|| cursor.as_str().and_then(|s| s.parse().ok()))
                    })
                    .flatten();
                if let Some(n) = value {
                    return Some(n);
                }
            }
        }
        None
    }

    fn parse(body: &str) -> Result<UsageSnapshot, SpendPanelError> {
        let json: serde_json::Value = serde_json::from_str(body)
            .map_err(|e| SpendPanelError::ParseError("kimik2".into(), e.to_string()))?;

        let consumed = Self::find_number(
            &json,
            &[
                &["total_credits_consumed"],
                &["totalCreditsConsumed"],
                &["credits_consumed"],
                &["consumedCredits"],
                &["usedCredits"],
                &["total"],
            ],
        )
        .unwrap_or(0.0);
        let remaining = Self::find_number(
            &json,
            &[
                &["credits_remaining"],
                &["creditsRemaining"],
                &["remaining_credits"],
                &["available_credits"],
                &["credits_left"],
            ],
        )
        .unwrap_or(0.0)
        .max(0.0);

        let mut snapshot = UsageSnapshot::new("kimik2");
        let mut credits = CreditsSnapshot::new(remaining, "credits");
        credits.used = Some(consumed);
        if consumed > 0.0 || remaining > 0.0 {
            credits.total = Some(consumed + remaining);
        }
        snapshot.credits = Some(credits);
        Ok(snapshot)
    }
}

impl Default for KimiK2Provider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl UsageProvider for KimiK2Provider {
    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    fn detect_credentials(&self) -> bool {
        ["KIMI_K2_API_KEY", "KIMIK2_API_KEY"].iter().any(|e| {
            std::env::var(e)
                .map(|v| !v.trim().is_empty())
                .unwrap_or(false)
        })
    }

    async fn fetch_usage(&self, ctx: &ProviderContext) -> Result<UsageSnapshot, SpendPanelError> {
        let key = Self::resolve_key(ctx)?;
        let client = Self::build_client(ctx)?;
        let url = format!("{}/api/user/credits", self.api_base().trim_end_matches('/'));
        let resp = client
            .get(url)
            .header("Authorization", format!("Bearer {}", key))
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
                "kimik2".into(),
                format!("invalid API key (HTTP {})", status.as_u16()),
            ));
        }
        if !status.is_success() {
            return Err(SpendPanelError::ProviderError(
                "kimik2".into(),
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
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn test_metadata() {
        assert_eq!(KimiK2Provider::new().metadata().id, "kimik2");
    }

    #[test]
    fn test_resolve_key_missing() {
        assert!(matches!(
            KimiK2Provider::resolve_key(&ProviderContext::new()).unwrap_err(),
            SpendPanelError::AuthFailed(_, _)
        ));
    }

    #[test]
    fn test_parse_flexible_paths() {
        let snap = KimiK2Provider::parse(
            r#"{"data":{"credits":{"total_credits_consumed":40,"credits_remaining":"60"}}}"#,
        )
        .unwrap();
        let c = snap.credits.unwrap();
        assert_eq!(c.balance, 60.0);
        assert_eq!(c.used, Some(40.0));
        assert_eq!(c.total, Some(100.0));
    }

    #[tokio::test]
    async fn test_fetch_success() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/user/credits"))
            .and(header("authorization", "Bearer k2"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{"credits_remaining":75,"total_credits_consumed":25}"#,
                "application/json",
            ))
            .mount(&server)
            .await;
        let provider = KimiK2Provider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("api_key".into(), "k2".into());
        let snap = provider.fetch_usage(&ctx).await.unwrap();
        assert_eq!(snap.credits.unwrap().balance, 75.0);
    }

    #[tokio::test]
    async fn test_fetch_401() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/user/credits"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let provider = KimiK2Provider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("api_key".into(), "bad".into());
        assert!(matches!(
            provider.fetch_usage(&ctx).await.unwrap_err(),
            SpendPanelError::AuthFailed(_, _)
        ));
    }
}
