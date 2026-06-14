//! Antigravity (Google Code Assist) usage provider.
//!
//! Ports CodexBar's remote-usage flow for Linux: it reads Antigravity's Google
//! OAuth credentials (default `~/.codexbar/antigravity/oauth_creds.json`, or an
//! explicit `access_token`), refreshes when expired, then reads per-model daily
//! quotas from Code Assist's `fetchAvailableModels`, falling back to
//! `retrieveUserQuota` buckets when models carry no consumed quota.

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::error::SpendPanelError;
use crate::model::{NamedRateWindow, PlanInfo, RateWindow, UsageSnapshot};
use crate::provider::{ProviderContext, ProviderMetadata, UsageProvider};

const CLOUDCODE_BASE: &str = "https://cloudcode-pa.googleapis.com";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

#[derive(Debug, serde::Deserialize)]
struct OAuthCreds {
    #[serde(default, alias = "accessToken")]
    access_token: Option<String>,
    #[serde(default, alias = "refreshToken")]
    refresh_token: Option<String>,
    #[serde(default, alias = "expiresAt")]
    expiry_date: Option<f64>,
    #[serde(default, alias = "projectId", alias = "project_id")]
    project_id: Option<String>,
    #[serde(default, alias = "clientId")]
    client_id: Option<String>,
    #[serde(default, alias = "clientSecret")]
    client_secret: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct RefreshResponse {
    access_token: String,
}

#[derive(Debug, serde::Deserialize)]
struct FetchAvailableModelsResponse {
    #[serde(default)]
    models: Option<std::collections::HashMap<String, RemoteModel>>,
}

#[derive(Debug, serde::Deserialize)]
struct RemoteModel {
    #[serde(default, rename = "displayName")]
    display_name: Option<String>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default, rename = "quotaInfo")]
    quota_info: Option<RemoteQuotaInfo>,
}

#[derive(Debug, serde::Deserialize)]
struct RemoteQuotaInfo {
    #[serde(default, rename = "remainingFraction")]
    remaining_fraction: Option<f64>,
    #[serde(default, rename = "resetTime")]
    reset_time: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct RetrieveUserQuotaResponse {
    #[serde(default)]
    buckets: Option<Vec<RetrieveUserQuotaBucket>>,
}

#[derive(Debug, serde::Deserialize)]
struct RetrieveUserQuotaBucket {
    #[serde(default, rename = "modelId")]
    model_id: Option<String>,
    #[serde(default, rename = "remainingFraction")]
    remaining_fraction: Option<f64>,
    #[serde(default, rename = "resetTime")]
    reset_time: Option<String>,
}

/// One model's resolved daily quota.
#[derive(Debug, Clone, PartialEq)]
struct ModelQuota {
    model_id: String,
    label: String,
    remaining_fraction: Option<f64>,
    reset_time: Option<DateTime<Utc>>,
}

impl ModelQuota {
    fn percent_left(&self) -> f64 {
        self.remaining_fraction.unwrap_or(1.0) * 100.0
    }
}

/// Antigravity Code Assist usage provider.
pub struct AntigravityProvider {
    metadata: ProviderMetadata,
    cloudcode_base: Option<String>,
    token_url: Option<String>,
}

impl AntigravityProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: "antigravity",
                name: "Antigravity",
                description: "Antigravity Code Assist daily quota monitor (Google OAuth)",
                auth_methods: &["oauth", "access_token", "env"],
                website: Some("https://antigravity.google"),
            },
            cloudcode_base: None,
            token_url: None,
        }
    }

    pub fn with_base_url(url: &str) -> Self {
        let mut p = Self::new();
        p.cloudcode_base = Some(url.to_string());
        p.token_url = Some(format!("{}/token", url.trim_end_matches('/')));
        p
    }

    fn cloudcode_base(&self) -> &str {
        self.cloudcode_base.as_deref().unwrap_or(CLOUDCODE_BASE)
    }

    fn token_url(&self) -> &str {
        self.token_url.as_deref().unwrap_or(TOKEN_URL)
    }

    fn creds_path(ctx: &ProviderContext) -> std::path::PathBuf {
        if let Some(p) = ctx.config.get("credentials_path").filter(|v| !v.is_empty()) {
            return std::path::PathBuf::from(p);
        }
        let home = std::env::var("HOME").unwrap_or_default();
        std::path::Path::new(&home).join(".codexbar/antigravity/oauth_creds.json")
    }

    fn build_client(ctx: &ProviderContext) -> Result<reqwest::Client, SpendPanelError> {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(ctx.timeout_secs))
            .build()
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))
    }

    /// (access_token, project_id) resolved from config or the creds file.
    async fn resolve_auth(
        &self,
        ctx: &ProviderContext,
        client: &reqwest::Client,
    ) -> Result<(String, Option<String>), SpendPanelError> {
        for key in ["access_token", "token"] {
            if let Some(value) = ctx.config.get(key).map(|s| s.trim()).filter(|s| !s.is_empty()) {
                let project = ctx.config.get("project").filter(|v| !v.is_empty()).cloned();
                return Ok((value.to_string(), project));
            }
        }

        let path = Self::creds_path(ctx);
        let data = std::fs::read_to_string(&path).map_err(|_| {
            SpendPanelError::AuthFailed(
                "antigravity".into(),
                format!(
                    "no access_token in config and no credentials at {}",
                    path.display()
                ),
            )
        })?;
        let creds: OAuthCreds = serde_json::from_str(&data)
            .map_err(|e| SpendPanelError::ParseError("antigravity".into(), e.to_string()))?;

        let project = ctx
            .config
            .get("project")
            .filter(|v| !v.is_empty())
            .cloned()
            .or_else(|| creds.project_id.clone());

        let expired = creds
            .expiry_date
            .map(|ms| (ms / 1000.0) < Utc::now().timestamp() as f64)
            .unwrap_or(true);
        if let Some(token) = creds
            .access_token
            .clone()
            .filter(|t| !t.is_empty() && !expired)
        {
            return Ok((token, project));
        }

        let refresh = creds
            .refresh_token
            .clone()
            .filter(|t| !t.is_empty())
            .ok_or_else(|| {
                SpendPanelError::AuthFailed(
                    "antigravity".into(),
                    "access token expired and no refresh_token available; re-run antigravity login"
                        .into(),
                )
            })?;
        let token = self.refresh_access_token(ctx, client, &creds, &refresh).await?;
        Ok((token, project))
    }

    async fn refresh_access_token(
        &self,
        ctx: &ProviderContext,
        client: &reqwest::Client,
        creds: &OAuthCreds,
        refresh_token: &str,
    ) -> Result<String, SpendPanelError> {
        let client_id = ctx
            .config
            .get("client_id")
            .filter(|v| !v.is_empty())
            .cloned()
            .or_else(|| std::env::var("ANTIGRAVITY_OAUTH_CLIENT_ID").ok().filter(|v| !v.is_empty()))
            .or_else(|| creds.client_id.clone());
        let client_secret = ctx
            .config
            .get("client_secret")
            .filter(|v| !v.is_empty())
            .cloned()
            .or_else(|| std::env::var("ANTIGRAVITY_OAUTH_CLIENT_SECRET").ok().filter(|v| !v.is_empty()))
            .or_else(|| creds.client_secret.clone());

        let (Some(client_id), Some(client_secret)) = (client_id, client_secret) else {
            return Err(SpendPanelError::AuthFailed(
                "antigravity".into(),
                "OAuth client not configured; set ANTIGRAVITY_OAUTH_CLIENT_ID/SECRET or store them in the credentials".into(),
            ));
        };

        let params = [
            ("client_id", client_id.as_str()),
            ("client_secret", client_secret.as_str()),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ];
        let resp = client
            .post(self.token_url())
            .form(&params)
            .send()
            .await
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;
        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;
        if !status.is_success() {
            return Err(SpendPanelError::AuthFailed(
                "antigravity".into(),
                format!("token refresh failed (HTTP {})", status.as_u16()),
            ));
        }
        let parsed: RefreshResponse = serde_json::from_str(&body)
            .map_err(|e| SpendPanelError::ParseError("antigravity".into(), e.to_string()))?;
        Ok(parsed.access_token)
    }

    async fn post_json(
        &self,
        client: &reqwest::Client,
        endpoint: &str,
        access_token: &str,
        body: String,
    ) -> Result<(reqwest::StatusCode, String), SpendPanelError> {
        let url = format!("{}/v1internal:{}", self.cloudcode_base().trim_end_matches('/'), endpoint);
        let resp = client
            .post(url)
            .header("Authorization", format!("Bearer {}", access_token))
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;
        Ok((status, text))
    }

    fn quota_body(project_id: Option<&str>) -> String {
        match project_id {
            Some(id) => format!(r#"{{"project": "{}"}}"#, id),
            None => "{}".to_string(),
        }
    }

    /// Resolves per-model quotas: prefer fetchAvailableModels, fall back to
    /// retrieveUserQuota buckets when models report no consumed quota.
    async fn fetch_model_quotas(
        &self,
        client: &reqwest::Client,
        access_token: &str,
        project_id: Option<&str>,
    ) -> Result<Vec<ModelQuota>, SpendPanelError> {
        let body = Self::quota_body(project_id);
        let (status, text) = self
            .post_json(client, "fetchAvailableModels", access_token, body.clone())
            .await?;

        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(SpendPanelError::AuthFailed(
                "antigravity".into(),
                "access token rejected (HTTP 401)".into(),
            ));
        }

        let from_models = if status.is_success() {
            serde_json::from_str::<FetchAvailableModelsResponse>(&text)
                .ok()
                .map(|r| Self::parse_models(&r))
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        // When every model is full (or none were returned), the consumed quota
        // lives in retrieveUserQuota — query it as the authoritative source.
        let all_full = from_models
            .iter()
            .all(|q| q.remaining_fraction.map(|f| f >= 0.999).unwrap_or(true));
        if from_models.is_empty() || all_full {
            let (qstatus, qtext) = self
                .post_json(client, "retrieveUserQuota", access_token, body)
                .await?;
            if qstatus == reqwest::StatusCode::UNAUTHORIZED {
                return Err(SpendPanelError::AuthFailed(
                    "antigravity".into(),
                    "access token rejected (HTTP 401)".into(),
                ));
            }
            let parsed = qstatus
                .is_success()
                .then(|| serde_json::from_str::<RetrieveUserQuotaResponse>(&qtext).ok())
                .flatten();
            if let Some(parsed) = parsed {
                let buckets = Self::parse_buckets(&parsed);
                if !buckets.is_empty() {
                    return Ok(buckets);
                }
            }
            if from_models.is_empty() {
                return Err(SpendPanelError::ProviderError(
                    "antigravity".into(),
                    "no model quotas available (fetchAvailableModels and retrieveUserQuota both empty)".into(),
                ));
            }
        }
        Ok(from_models)
    }

    fn parse_models(resp: &FetchAvailableModelsResponse) -> Vec<ModelQuota> {
        let Some(models) = &resp.models else {
            return Vec::new();
        };
        let mut quotas: Vec<ModelQuota> = models
            .iter()
            .filter_map(|(id, model)| {
                let quota = model.quota_info.as_ref()?;
                let label = model
                    .display_name
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .or(model.label.as_deref().filter(|s| !s.trim().is_empty()))
                    .unwrap_or(id)
                    .to_string();
                Some(ModelQuota {
                    model_id: id.clone(),
                    label,
                    remaining_fraction: quota.remaining_fraction,
                    reset_time: parse_reset(quota.reset_time.as_deref()),
                })
            })
            .collect();
        quotas.sort_by(|a, b| a.model_id.cmp(&b.model_id));
        quotas
    }

    fn parse_buckets(resp: &RetrieveUserQuotaResponse) -> Vec<ModelQuota> {
        let Some(buckets) = &resp.buckets else {
            return Vec::new();
        };
        let mut map: std::collections::BTreeMap<String, (Option<f64>, Option<String>)> =
            std::collections::BTreeMap::new();
        for bucket in buckets {
            let Some(model_id) = bucket.model_id.as_deref().map(str::trim).filter(|s| !s.is_empty())
            else {
                continue;
            };
            let next = (bucket.remaining_fraction, bucket.reset_time.clone());
            map.entry(model_id.to_string())
                .and_modify(|existing| {
                    let cur = existing.0.unwrap_or(f64::MAX);
                    let nv = next.0.unwrap_or(f64::MAX);
                    if nv < cur {
                        *existing = next.clone();
                    }
                })
                .or_insert(next);
        }
        map.into_iter()
            .map(|(model_id, (fraction, reset))| ModelQuota {
                label: model_id.clone(),
                model_id,
                remaining_fraction: fraction,
                reset_time: parse_reset(reset.as_deref()),
            })
            .collect()
    }

    fn snapshot_from_quotas(quotas: &[ModelQuota]) -> UsageSnapshot {
        let mut sorted: Vec<&ModelQuota> = quotas.iter().collect();
        sorted.sort_by(|a, b| a.percent_left().total_cmp(&b.percent_left()));

        let window = |q: &ModelQuota| -> RateWindow {
            let used = (100.0 - q.percent_left()).clamp(0.0, 100.0).round() as u64;
            let mut w = RateWindow::new(used, 100, q.label.clone(), 1440);
            w.resets_at = q.reset_time;
            w
        };

        let mut snapshot = UsageSnapshot::new("antigravity");
        let mut iter = sorted.into_iter();
        if let Some(q) = iter.next() {
            snapshot.primary_rate_window = Some(window(q));
        }
        if let Some(q) = iter.next() {
            snapshot.secondary_rate_window = Some(window(q));
        }
        if let Some(q) = iter.next() {
            snapshot.tertiary_rate_window = Some(window(q));
        }
        for q in iter {
            snapshot.extra_rate_windows.push(NamedRateWindow {
                id: q.model_id.clone(),
                label: q.label.clone(),
                window: window(q),
            });
        }
        snapshot
    }
}

fn parse_reset(s: Option<&str>) -> Option<DateTime<Utc>> {
    let raw = s?;
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

impl Default for AntigravityProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl UsageProvider for AntigravityProvider {
    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    fn detect_credentials(&self) -> bool {
        let home = std::env::var("HOME").unwrap_or_default();
        std::path::Path::new(&home)
            .join(".codexbar/antigravity/oauth_creds.json")
            .exists()
    }

    async fn fetch_usage(&self, ctx: &ProviderContext) -> Result<UsageSnapshot, SpendPanelError> {
        let client = Self::build_client(ctx)?;
        let (access_token, project_id) = self.resolve_auth(ctx, &client).await?;
        let quotas = self
            .fetch_model_quotas(&client, &access_token, project_id.as_deref())
            .await?;
        let mut snapshot = Self::snapshot_from_quotas(&quotas);
        snapshot.plan = Some(PlanInfo {
            name: "Code Assist".into(),
            tier: None,
            features: Vec::new(),
            price: None,
            currency: None,
            billing_period: Some("daily".into()),
        });
        Ok(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const MODELS: &str = r#"{
      "models": {
        "claude-sonnet": {"displayName": "Claude Sonnet", "quotaInfo": {"remainingFraction": 0.25, "resetTime": "2026-06-14T00:00:00Z"}},
        "gemini-pro": {"displayName": "Gemini Pro", "quotaInfo": {"remainingFraction": 0.8}}
      }
    }"#;

    const BUCKETS: &str = r#"{
      "buckets": [
        {"modelId": "claude-sonnet", "remainingFraction": 0.5, "resetTime": "2026-06-14T00:00:00Z"},
        {"modelId": "claude-sonnet", "remainingFraction": 0.3},
        {"modelId": "gemini-pro", "remainingFraction": 0.6}
      ]
    }"#;

    #[test]
    fn test_metadata() {
        assert_eq!(AntigravityProvider::new().metadata().id, "antigravity");
    }

    #[test]
    fn test_parse_models() {
        let resp: FetchAvailableModelsResponse = serde_json::from_str(MODELS).unwrap();
        let quotas = AntigravityProvider::parse_models(&resp);
        assert_eq!(quotas.len(), 2);
        let claude = quotas.iter().find(|q| q.model_id == "claude-sonnet").unwrap();
        assert_eq!(claude.label, "Claude Sonnet");
        assert_eq!(claude.percent_left(), 25.0);
    }

    #[test]
    fn test_parse_buckets_keeps_lowest() {
        let resp: RetrieveUserQuotaResponse = serde_json::from_str(BUCKETS).unwrap();
        let quotas = AntigravityProvider::parse_buckets(&resp);
        let claude = quotas.iter().find(|q| q.model_id == "claude-sonnet").unwrap();
        assert_eq!(claude.remaining_fraction, Some(0.3));
    }

    #[test]
    fn test_snapshot_orders_by_lowest() {
        let resp: FetchAvailableModelsResponse = serde_json::from_str(MODELS).unwrap();
        let quotas = AntigravityProvider::parse_models(&resp);
        let snapshot = AntigravityProvider::snapshot_from_quotas(&quotas);
        // claude 25% left → 75% used is the lowest remaining → primary.
        assert_eq!(snapshot.primary_rate_window.unwrap().used, Some(75));
        assert_eq!(snapshot.secondary_rate_window.unwrap().used, Some(20));
    }

    #[tokio::test]
    async fn test_fetch_usage_with_models() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1internal:fetchAvailableModels"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(MODELS, "application/json"))
            .mount(&server)
            .await;

        let provider = AntigravityProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("access_token".into(), "ya29-test".into());
        let snapshot = provider.fetch_usage(&ctx).await.unwrap();
        assert_eq!(snapshot.primary_rate_window.unwrap().used, Some(75));
    }

    #[tokio::test]
    async fn test_fetch_usage_falls_back_to_buckets() {
        let server = MockServer::start().await;
        // All models full → triggers retrieveUserQuota fallback.
        Mock::given(method("POST"))
            .and(path("/v1internal:fetchAvailableModels"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{"models":{"gemini-pro":{"displayName":"Gemini Pro","quotaInfo":{"remainingFraction":1.0}}}}"#,
                "application/json",
            ))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1internal:retrieveUserQuota"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(BUCKETS, "application/json"))
            .mount(&server)
            .await;

        let provider = AntigravityProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("access_token".into(), "ya29-test".into());
        let snapshot = provider.fetch_usage(&ctx).await.unwrap();
        // buckets: claude 30% left → 70% used is lowest → primary.
        assert_eq!(snapshot.primary_rate_window.unwrap().used, Some(70));
    }

    #[tokio::test]
    async fn test_fetch_usage_401_is_auth_failed() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1internal:fetchAvailableModels"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let provider = AntigravityProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("access_token".into(), "bad".into());
        let err = provider.fetch_usage(&ctx).await.unwrap_err();
        assert!(matches!(err, SpendPanelError::AuthFailed(_, _)));
    }
}
