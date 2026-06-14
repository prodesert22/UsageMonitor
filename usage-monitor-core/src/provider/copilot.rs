use async_trait::async_trait;

use crate::error::SpendPanelError;
use crate::model::{PlanInfo, RateWindow, UsageSnapshot};
use crate::provider::{ProviderContext, ProviderMetadata, UsageProvider};

#[derive(Debug, serde::Deserialize)]
struct CopilotUsageResponse {
    #[serde(default)]
    quota_snapshots: CopilotQuotaSnapshots,
    #[serde(default)]
    copilot_plan: Option<String>,
    #[serde(default)]
    token_based_billing: bool,
}

#[derive(Debug, Default, serde::Deserialize)]
struct CopilotQuotaSnapshots {
    #[serde(default)]
    premium_interactions: Option<CopilotQuotaSnapshot>,
    #[serde(default)]
    chat: Option<CopilotQuotaSnapshot>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
struct CopilotQuotaSnapshot {
    #[serde(default)]
    entitlement: f64,
    #[serde(default)]
    remaining: f64,
    #[serde(default)]
    percent_remaining: Option<f64>,
    #[serde(default)]
    unlimited: bool,
}

impl CopilotQuotaSnapshot {
    /// Percent remaining, derived from entitlement/remaining when absent.
    fn percent_remaining(&self) -> Option<f64> {
        if self.unlimited {
            return Some(100.0);
        }
        if let Some(p) = self.percent_remaining {
            return Some(p);
        }
        if self.entitlement > 0.0 {
            return Some((self.remaining / self.entitlement) * 100.0);
        }
        None
    }

    /// Zero-entitlement placeholder GitHub returns for token-based seats.
    fn is_placeholder(&self) -> bool {
        if self.unlimited {
            return false;
        }
        self.entitlement == 0.0 && self.remaining == 0.0
    }

    fn to_rate_window(&self, label: &str) -> Option<RateWindow> {
        if self.is_placeholder() {
            return None;
        }
        let percent_remaining = self.percent_remaining()?;
        let used_percent = (100.0 - percent_remaining).clamp(0.0, 100.0);
        // entitlement is the denominator (interactions); used = entitlement - remaining.
        if self.entitlement > 0.0 {
            let used = (self.entitlement - self.remaining).max(0.0);
            Some(RateWindow::new(
                used.round() as u64,
                self.entitlement.round() as u64,
                label.to_string(),
                0,
            ))
        } else {
            // Unlimited or percent-only: synthesize a 0..100 window from the percent.
            Some(RateWindow::new(
                used_percent.round() as u64,
                100,
                label.to_string(),
                0,
            ))
        }
    }
}

/// GitHub Copilot usage provider (GitHub OAuth/PAT token auth).
pub struct CopilotProvider {
    metadata: ProviderMetadata,
    /// Base URL override for tests (defaults to api.github.com).
    base_url: Option<String>,
}

impl CopilotProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: "copilot",
                name: "GitHub Copilot",
                description: "GitHub Copilot quota monitor",
                auth_methods: &["token", "api_key", "env"],
                website: Some("https://github.com/features/copilot"),
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
        self.base_url.as_deref().unwrap_or("https://api.github.com")
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

    fn resolve_token(ctx: &ProviderContext) -> Result<String, SpendPanelError> {
        for key in ["token", "api_key"] {
            if let Some(value) = ctx.config.get(key) {
                let cleaned = Self::clean(value);
                if !cleaned.is_empty() {
                    return Ok(cleaned);
                }
            }
        }
        for env in ["COPILOT_API_TOKEN", "GITHUB_TOKEN", "GH_TOKEN"] {
            if let Ok(value) = std::env::var(env) {
                let cleaned = Self::clean(&value);
                if !cleaned.is_empty() {
                    return Ok(cleaned);
                }
            }
        }
        Err(SpendPanelError::AuthFailed(
            "copilot".into(),
            "no token found in token/api_key config, COPILOT_API_TOKEN, GITHUB_TOKEN, or GH_TOKEN"
                .into(),
        ))
    }

    fn build_client(ctx: &ProviderContext) -> Result<reqwest::Client, SpendPanelError> {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(ctx.timeout_secs))
            .build()
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))
    }

    async fn fetch_usage_response(
        base_url: &str,
        client: &reqwest::Client,
        token: &str,
    ) -> Result<CopilotUsageResponse, SpendPanelError> {
        let url = format!(
            "{}/copilot_internal/user",
            base_url.trim_end_matches('/')
        );
        let resp = client
            .get(url)
            .header("Authorization", format!("token {}", token))
            .header("Accept", "application/json")
            .header("X-Github-Api-Version", "2025-04-01")
            .header("Editor-Version", "vscode/1.96.2")
            .header("User-Agent", "GitHubCopilotChat/0.26.7")
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
                "copilot".into(),
                format!("invalid token (HTTP {})", status.as_u16()),
            ));
        }
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(SpendPanelError::ProviderError(
                "copilot".into(),
                "no Copilot subscription for this token (HTTP 404)".into(),
            ));
        }
        if !status.is_success() {
            return Err(SpendPanelError::ProviderError(
                "copilot".into(),
                format!("HTTP {}: {}", status, body),
            ));
        }

        serde_json::from_str(&body)
            .map_err(|e| SpendPanelError::ParseError("copilot".into(), e.to_string()))
    }

    fn snapshot_from_response(
        resp: &CopilotUsageResponse,
    ) -> Result<UsageSnapshot, SpendPanelError> {
        let premium = resp
            .quota_snapshots
            .premium_interactions
            .as_ref()
            .and_then(|s| s.to_rate_window("Premium"));
        let chat = resp
            .quota_snapshots
            .chat
            .as_ref()
            .and_then(|s| s.to_rate_window("Chat"));

        let (primary, secondary) = match (premium, chat) {
            (Some(p), c) => (Some(p), c),
            (None, Some(c)) => (None, Some(c)),
            (None, None) => {
                if resp.token_based_billing {
                    (None, None)
                } else {
                    return Err(SpendPanelError::ProviderError(
                        "copilot".into(),
                        "no usable quota in response".into(),
                    ));
                }
            }
        };

        let mut snapshot = UsageSnapshot::new("copilot");
        snapshot.primary_rate_window = primary;
        snapshot.secondary_rate_window = secondary;
        if let Some(plan) = resp
            .copilot_plan
            .as_deref()
            .filter(|p| !p.is_empty() && *p != "unknown")
        {
            snapshot.plan = Some(PlanInfo {
                name: capitalize(plan),
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

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

impl Default for CopilotProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl UsageProvider for CopilotProvider {
    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    fn detect_credentials(&self) -> bool {
        ["COPILOT_API_TOKEN", "GITHUB_TOKEN", "GH_TOKEN"]
            .iter()
            .any(|env| std::env::var(env).map(|v| !v.trim().is_empty()).unwrap_or(false))
    }

    async fn fetch_usage(&self, ctx: &ProviderContext) -> Result<UsageSnapshot, SpendPanelError> {
        let token = Self::resolve_token(ctx)?;
        let client = Self::build_client(ctx)?;
        let response = Self::fetch_usage_response(self.api_base(), &client, &token).await?;
        Self::snapshot_from_response(&response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const SAMPLE: &str = r#"{
      "quota_snapshots": {
        "premium_interactions": {"entitlement": 300, "remaining": 90, "percent_remaining": 30, "unlimited": false},
        "chat": {"entitlement": 0, "remaining": 0, "unlimited": true}
      },
      "copilot_plan": "individual",
      "token_based_billing": false
    }"#;

    fn parse(body: &str) -> CopilotUsageResponse {
        serde_json::from_str(body).unwrap()
    }

    #[test]
    fn test_metadata() {
        let p = CopilotProvider::new();
        assert_eq!(p.metadata().id, "copilot");
        assert!(p.metadata().auth_methods.contains(&"token"));
    }

    #[test]
    fn test_resolve_token_missing_is_error() {
        let err = CopilotProvider::resolve_token(&ProviderContext::new()).unwrap_err();
        assert!(matches!(err, SpendPanelError::AuthFailed(_, _)));
    }

    #[test]
    fn test_premium_window_used_from_entitlement() {
        let resp = parse(SAMPLE);
        let window = resp
            .quota_snapshots
            .premium_interactions
            .as_ref()
            .unwrap()
            .to_rate_window("Premium")
            .unwrap();
        assert_eq!(window.used, Some(210));
        assert_eq!(window.limit, Some(300));
    }

    #[test]
    fn test_unlimited_chat_full_remaining() {
        let resp = parse(SAMPLE);
        let window = resp
            .quota_snapshots
            .chat
            .as_ref()
            .unwrap()
            .to_rate_window("Chat")
            .unwrap();
        // unlimited → 100% remaining → 0% used
        assert_eq!(window.used, Some(0));
    }

    #[test]
    fn test_placeholder_dropped() {
        let snap = CopilotQuotaSnapshot {
            entitlement: 0.0,
            remaining: 0.0,
            percent_remaining: Some(100.0),
            unlimited: false,
        };
        assert!(snap.is_placeholder());
        assert!(snap.to_rate_window("Premium").is_none());
    }

    #[test]
    fn test_snapshot_plan_capitalized() {
        let snapshot = CopilotProvider::snapshot_from_response(&parse(SAMPLE)).unwrap();
        assert_eq!(snapshot.plan.unwrap().name, "Individual");
        assert!(snapshot.primary_rate_window.is_some());
    }

    #[test]
    fn test_token_based_billing_no_quota_ok() {
        let body = r#"{
          "quota_snapshots": {
            "premium_interactions": {"entitlement": 0, "remaining": 0},
            "chat": {"entitlement": 0, "remaining": 0}
          },
          "copilot_plan": "business",
          "token_based_billing": true
        }"#;
        let snapshot = CopilotProvider::snapshot_from_response(&parse(body)).unwrap();
        assert!(snapshot.primary_rate_window.is_none());
        assert!(snapshot.secondary_rate_window.is_none());
        assert_eq!(snapshot.plan.unwrap().name, "Business");
    }

    #[test]
    fn test_chat_only_leaves_primary_empty() {
        // No premium quota → primary stays empty, chat lands in secondary.
        let body = r#"{
          "quota_snapshots": {
            "chat": {"entitlement": 50, "remaining": 20, "percent_remaining": 40}
          },
          "copilot_plan": "free"
        }"#;
        let snapshot = CopilotProvider::snapshot_from_response(&parse(body)).unwrap();
        assert!(snapshot.primary_rate_window.is_none());
        let chat = snapshot.secondary_rate_window.unwrap();
        assert_eq!(chat.used, Some(30));
        assert_eq!(chat.label, "Chat");
    }

    #[test]
    fn test_no_usable_quota_without_token_billing_errors() {
        let body = r#"{"quota_snapshots":{},"copilot_plan":"free","token_based_billing":false}"#;
        assert!(matches!(
            CopilotProvider::snapshot_from_response(&parse(body)).unwrap_err(),
            SpendPanelError::ProviderError(_, _)
        ));
    }

    #[tokio::test]
    async fn test_fetch_usage_success() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/copilot_internal/user"))
            .and(header("authorization", "token gho_test"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(SAMPLE, "application/json"))
            .mount(&server)
            .await;

        let provider = CopilotProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("token".into(), "gho_test".into());
        let snapshot = provider.fetch_usage(&ctx).await.unwrap();
        assert_eq!(snapshot.primary_rate_window.unwrap().limit, Some(300));
    }

    #[tokio::test]
    async fn test_fetch_usage_401_is_auth_failed() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/copilot_internal/user"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let provider = CopilotProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("token".into(), "bad".into());
        let err = provider.fetch_usage(&ctx).await.unwrap_err();
        assert!(matches!(err, SpendPanelError::AuthFailed(_, _)));
    }
}
