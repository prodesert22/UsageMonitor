//! Gemini (Google AI / Code Assist) usage provider.
//!
//! Ports CodexBar's OAuth flow for the Linux CLI: it reads the gemini-cli OAuth
//! credentials from `~/.gemini/oauth_creds.json` (or an explicit `access_token`
//! in config), refreshes the access token when expired using the public
//! gemini-cli OAuth client, then calls Code Assist's `loadCodeAssist`,
//! `onboardUser` (when no managed project exists yet) and `retrieveUserQuota`
//! endpoints to read per-model daily quotas.

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::error::SpendPanelError;
use crate::model::{PlanInfo, RateWindow, UsageSnapshot};
use crate::provider::{ProviderContext, ProviderMetadata, UsageProvider};

/// Public gemini-cli OAuth client (shipped in the open-source `@google/gemini-cli`).
pub(crate) const GEMINI_CLI_CLIENT_ID: &str =
    "681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com";
pub(crate) const GEMINI_CLI_CLIENT_SECRET: &str = "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";

const CLOUDCODE_BASE: &str = "https://cloudcode-pa.googleapis.com";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

/// gemini-cli version advertised in the Code Assist User-Agent (latest
/// published at time of writing).
const GEMINI_CLI_VERSION: &str = "0.57.0";
const FREE_TIER_ID: &str = "free-tier";
const LEGACY_TIER_ID: &str = "legacy-tier";
/// Bounded polling of the onboardUser operation (mirrors the reference clone).
const ONBOARD_POLL_ATTEMPTS: usize = 10;
const ONBOARD_POLL_DELAY: std::time::Duration = std::time::Duration::from_secs(1);

#[derive(Debug, serde::Deserialize)]
struct QuotaResponse {
    #[serde(default)]
    buckets: Option<Vec<QuotaBucket>>,
}

#[derive(Debug, serde::Deserialize)]
struct QuotaBucket {
    #[serde(default, rename = "remainingFraction")]
    remaining_fraction: Option<f64>,
    #[serde(default, rename = "resetTime")]
    reset_time: Option<String>,
    #[serde(default, rename = "modelId")]
    model_id: Option<String>,
}

/// `loadCodeAssist` payload: the managed project (string or `{id}`) plus the
/// user's current/eligible Code Assist tiers.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoadCodeAssist {
    #[serde(default)]
    cloudaicompanion_project: Option<serde_json::Value>,
    #[serde(default)]
    current_tier: Option<CurrentTier>,
    #[serde(default)]
    allowed_tiers: Vec<UserTier>,
    #[serde(default)]
    ineligible_tiers: Vec<IneligibleTier>,
}

impl LoadCodeAssist {
    /// The managed project id, whether returned as a string or `{id}`.
    fn managed_project_id(&self) -> Option<String> {
        match &self.cloudaicompanion_project {
            Some(serde_json::Value::String(s)) if !s.trim().is_empty() => {
                Some(s.trim().to_string())
            }
            Some(serde_json::Value::Object(obj)) => obj
                .get("id")
                .or_else(|| obj.get("projectId"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.trim().to_string()),
            _ => None,
        }
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CurrentTier {
    #[serde(default)]
    id: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserTier {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    is_default: Option<bool>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct IneligibleTier {
    #[serde(default)]
    reason_message: Option<String>,
}

/// `onboardUser` payload: an operation with a `done` flag and the resulting
/// managed project.
#[derive(Debug, serde::Deserialize)]
struct OnboardUser {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    done: bool,
    #[serde(default)]
    response: Option<OnboardResponse>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct OnboardResponse {
    #[serde(default)]
    cloudaicompanion_project: Option<CloudProject>,
}

#[derive(Debug, serde::Deserialize)]
struct CloudProject {
    #[serde(default)]
    id: Option<String>,
}

/// One model's resolved daily quota.
#[derive(Debug, Clone, PartialEq)]
struct ModelQuota {
    model_id: String,
    percent_left: f64,
    reset_time: Option<DateTime<Utc>>,
}

#[derive(Debug, serde::Deserialize)]
struct OAuthCreds {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    /// Unix-millis expiry, as gemini-cli stores it.
    #[serde(default)]
    expiry_date: Option<f64>,
}

#[derive(Debug, serde::Deserialize)]
struct RefreshResponse {
    access_token: String,
}

fn is_flash_lite(id: &str) -> bool {
    id.contains("flash-lite")
}
fn is_flash(id: &str) -> bool {
    id.contains("flash") && !is_flash_lite(id)
}
fn is_pro(id: &str) -> bool {
    id.contains("pro")
}

/// Gemini Code Assist usage provider.
pub struct GeminiProvider {
    metadata: ProviderMetadata,
    /// Base for cloudcode-pa endpoints (overridable in tests).
    cloudcode_base: Option<String>,
    /// Base for the OAuth token endpoint (overridable in tests).
    token_url: Option<String>,
}

impl GeminiProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: "gemini",
                name: "Google Gemini",
                description: "Gemini Code Assist daily quota monitor (gemini-cli OAuth)",
                auth_methods: &["oauth", "access_token", "env"],
                website: Some("https://aistudio.google.com"),
            },
            cloudcode_base: None,
            token_url: None,
        }
    }

    /// Points cloudcode + token endpoints at a test server.
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
        if let Some(p) = ctx
            .config
            .get("credentials_path")
            .map(|s| s.trim())
            .filter(|v| !v.is_empty())
        {
            return crate::provider::expand_credentials_path(p);
        }
        let home = std::env::var("HOME").unwrap_or_default();
        std::path::Path::new(&home).join(".gemini/oauth_creds.json")
    }

    fn build_client(ctx: &ProviderContext) -> Result<reqwest::Client, SpendPanelError> {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(ctx.timeout_secs))
            .user_agent(Self::build_user_agent())
            .build()
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))
    }

    /// gemini-cli style User-Agent so the Code Assist backend fingerprints us
    /// as CLI traffic.
    fn build_user_agent() -> String {
        format!(
            "GeminiCLI/{GEMINI_CLI_VERSION}/gemini-code-assist (linux; {}; terminal)",
            std::env::consts::ARCH
        )
    }

    /// Short random lowercase-alphanumeric activity id (mirrors the clone's
    /// base36 request id).
    fn activity_request_id() -> String {
        Self::random_alphanumeric(8)
    }

    fn random_alphanumeric(len: usize) -> String {
        const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
        let mut bytes = [0u8; 32];
        getrandom::getrandom(&mut bytes).expect("OS randomness unavailable");
        bytes
            .iter()
            .take(len)
            .map(|b| ALPHABET[*b as usize % ALPHABET.len()] as char)
            .collect()
    }

    /// Common headers for every Code Assist API call.
    fn apply_code_assist_headers(
        req: reqwest::RequestBuilder,
        access_token: &str,
    ) -> reqwest::RequestBuilder {
        req.header("Authorization", format!("Bearer {access_token}"))
            .header("Content-Type", "application/json")
            .header("User-Agent", Self::build_user_agent())
            .header("x-activity-request-id", Self::activity_request_id())
    }

    /// Code Assist metadata payload required for product provisioning.
    fn code_assist_metadata() -> serde_json::Value {
        serde_json::json!({
            "ideType": "IDE_UNSPECIFIED",
            "platform": "PLATFORM_UNSPECIFIED",
            "pluginType": "GEMINI",
        })
    }

    /// Default onboarding tier when the backend lists no allowed tiers.
    fn pick_onboard_tier(allowed_tiers: &[UserTier]) -> String {
        allowed_tiers
            .iter()
            .find(|tier| tier.is_default.unwrap_or(false))
            .and_then(|tier| tier.id.clone())
            .or_else(|| allowed_tiers.first().and_then(|tier| tier.id.clone()))
            .unwrap_or_else(|| LEGACY_TIER_ID.to_string())
    }

    /// Concise message from ineligible-tier reasons, if any.
    fn ineligible_message(tiers: &[IneligibleTier]) -> Option<String> {
        let reasons: Vec<&str> = tiers
            .iter()
            .filter_map(|tier| tier.reason_message.as_deref())
            .filter(|message| !message.trim().is_empty())
            .collect();
        if reasons.is_empty() {
            None
        } else {
            Some(reasons.join(", "))
        }
    }

    fn project_required_error() -> SpendPanelError {
        SpendPanelError::ProviderError(
            "gemini".into(),
            "Gemini Code Assist requires a Google Cloud project. Configure one with \
             `usage-monitor-cli gemini set project <project-id>` and retry."
                .into(),
        )
    }

    /// Resolves a usable access token: explicit config token, or the creds file
    /// (refreshing when expired).
    async fn resolve_access_token(
        &self,
        ctx: &ProviderContext,
        client: &reqwest::Client,
    ) -> Result<String, SpendPanelError> {
        for key in ["access_token", "token"] {
            if let Some(value) = ctx
                .config
                .get(key)
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
            {
                return Ok(value.to_string());
            }
        }

        let path = Self::creds_path(ctx);
        let data = std::fs::read_to_string(&path).map_err(|_| {
            SpendPanelError::AuthFailed(
                "gemini".into(),
                format!(
                    "no access_token in config and no credentials at {}",
                    path.display()
                ),
            )
        })?;
        let creds: OAuthCreds = serde_json::from_str(&data)
            .map_err(|e| SpendPanelError::ParseError("gemini".into(), e.to_string()))?;

        let expired = creds
            .expiry_date
            .map(|ms| (ms / 1000.0) < Utc::now().timestamp() as f64)
            .unwrap_or(true);
        let token = creds.access_token.clone().filter(|t| !t.is_empty());

        if let Some(token) = token.filter(|_| !expired) {
            return Ok(token);
        }

        let refresh = creds
            .refresh_token
            .filter(|t| !t.is_empty())
            .ok_or_else(|| {
                SpendPanelError::AuthFailed(
                    "gemini".into(),
                    "access token expired and no refresh_token available; re-run gemini login"
                        .into(),
                )
            })?;
        self.refresh_access_token(client, &refresh).await
    }

    async fn refresh_access_token(
        &self,
        client: &reqwest::Client,
        refresh_token: &str,
    ) -> Result<String, SpendPanelError> {
        let params = [
            ("client_id", GEMINI_CLI_CLIENT_ID),
            ("client_secret", GEMINI_CLI_CLIENT_SECRET),
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
                "gemini".into(),
                format!("token refresh failed (HTTP {})", status.as_u16()),
            ));
        }
        let parsed: RefreshResponse = serde_json::from_str(&body)
            .map_err(|e| SpendPanelError::ParseError("gemini".into(), e.to_string()))?;
        Ok(parsed.access_token)
    }

    /// Loads the Code Assist account state (best-effort; `None` on any
    /// failure, mirroring the clone's `loadManagedProject`).
    async fn load_code_assist(
        &self,
        client: &reqwest::Client,
        access_token: &str,
        configured_project: Option<&str>,
    ) -> Option<LoadCodeAssist> {
        let url = format!(
            "{}/v1internal:loadCodeAssist",
            self.cloudcode_base().trim_end_matches('/')
        );
        let mut body = serde_json::Map::new();
        body.insert("metadata".into(), Self::code_assist_metadata());
        if let Some(project) = configured_project.filter(|p| !p.is_empty()) {
            body.insert(
                "cloudaicompanionProject".into(),
                serde_json::Value::String(project.to_string()),
            );
        }
        let resp = Self::apply_code_assist_headers(client.post(url), access_token)
            .body(serde_json::Value::Object(body).to_string())
            .send()
            .await
            .ok()?;
        if !resp.status().is_success() {
            return None;
        }
        resp.json().await.ok()
    }

    /// Onboards the user onto a Code Assist tier, polling the returned
    /// operation until it completes. Returns the managed project id, or the
    /// configured project as a fallback.
    async fn onboard_user(
        &self,
        client: &reqwest::Client,
        access_token: &str,
        tier_id: &str,
        configured_project: Option<&str>,
    ) -> Result<Option<String>, SpendPanelError> {
        let base = format!("{}/v1internal", self.cloudcode_base().trim_end_matches('/'));
        let mut body = serde_json::Map::new();
        body.insert("tierId".into(), serde_json::Value::String(tier_id.into()));
        body.insert("metadata".into(), Self::code_assist_metadata());
        if let Some(project) = configured_project.filter(|p| !p.is_empty()) {
            body.insert(
                "cloudaicompanionProject".into(),
                serde_json::Value::String(project.to_string()),
            );
        }
        let resp = Self::apply_code_assist_headers(
            client.post(format!("{base}:onboardUser")),
            access_token,
        )
        .body(serde_json::Value::Object(body).to_string())
        .send()
        .await
        .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;
        if !resp.status().is_success() {
            return Ok(None);
        }
        let mut payload: OnboardUser = resp
            .json()
            .await
            .map_err(|e| SpendPanelError::ParseError("gemini".into(), e.to_string()))?;

        // Poll the operation until done (bounded so we never hang).
        if !payload.done {
            for _ in 0..ONBOARD_POLL_ATTEMPTS {
                let Some(op_name) = payload.name.clone() else {
                    break;
                };
                tokio::time::sleep(ONBOARD_POLL_DELAY).await;
                let op_resp = Self::apply_code_assist_headers(
                    client.get(format!("{base}/{op_name}")),
                    access_token,
                )
                .send()
                .await
                .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;
                if !op_resp.status().is_success() {
                    return Ok(None);
                }
                let next: OnboardUser = op_resp
                    .json()
                    .await
                    .map_err(|e| SpendPanelError::ParseError("gemini".into(), e.to_string()))?;
                payload = next;
                if payload.done {
                    break;
                }
            }
        }

        if payload.done {
            if let Some(project_id) = payload
                .response
                .as_ref()
                .and_then(|resp| resp.cloudaicompanion_project.as_ref())
                .and_then(|project| project.id.as_deref())
                .filter(|id| !id.is_empty())
            {
                return Ok(Some(project_id.to_string()));
            }
            if let Some(project) = configured_project.filter(|p| !p.is_empty()) {
                return Ok(Some(project.to_string()));
            }
        }
        Ok(None)
    }

    /// Resolves the Code Assist project id, onboarding the account when no
    /// managed project exists yet (this is what provisions quota access).
    async fn resolve_project_id(
        &self,
        client: &reqwest::Client,
        ctx: &ProviderContext,
        access_token: &str,
    ) -> Result<String, SpendPanelError> {
        let configured_project = ctx
            .config
            .get("project")
            .filter(|v| !v.is_empty())
            .map(String::as_str);

        let Some(load) = self
            .load_code_assist(client, access_token, configured_project)
            .await
        else {
            return Err(Self::project_required_error());
        };

        if let Some(project_id) = load.managed_project_id() {
            return Ok(project_id);
        }

        // No managed project resolved: follow the tier logic.
        let has_current_tier = load
            .current_tier
            .as_ref()
            .and_then(|tier| tier.id.as_deref())
            .is_some_and(|id| !id.is_empty());

        if has_current_tier {
            if let Some(project) = configured_project {
                return Ok(project.to_string());
            }
            if let Some(reason) = Self::ineligible_message(&load.ineligible_tiers) {
                return Err(SpendPanelError::ProviderError("gemini".into(), reason));
            }
            return Err(Self::project_required_error());
        }

        let tier_id = Self::pick_onboard_tier(&load.allowed_tiers);
        if tier_id != FREE_TIER_ID && configured_project.is_none() {
            return Err(Self::project_required_error());
        }
        match self
            .onboard_user(client, access_token, &tier_id, configured_project)
            .await?
        {
            Some(project_id) => Ok(project_id),
            None => Err(Self::project_required_error()),
        }
    }

    async fn retrieve_quota(
        &self,
        client: &reqwest::Client,
        access_token: &str,
        project_id: &str,
    ) -> Result<QuotaResponse, SpendPanelError> {
        let url = format!(
            "{}/v1internal:retrieveUserQuota",
            self.cloudcode_base().trim_end_matches('/')
        );
        let body = serde_json::json!({ "project": project_id });
        let resp = Self::apply_code_assist_headers(client.post(url), access_token)
            .body(body.to_string())
            .send()
            .await
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(SpendPanelError::AuthFailed(
                "gemini".into(),
                "access token rejected (HTTP 401)".into(),
            ));
        }
        if !status.is_success() {
            return Err(SpendPanelError::ProviderError(
                "gemini".into(),
                format!("HTTP {}: {}", status, text),
            ));
        }
        serde_json::from_str(&text)
            .map_err(|e| SpendPanelError::ParseError("gemini".into(), e.to_string()))
    }

    /// Groups buckets by model (keeping the lowest remaining fraction per model).
    fn parse_quota(resp: &QuotaResponse) -> Result<Vec<ModelQuota>, SpendPanelError> {
        let buckets = resp
            .buckets
            .as_deref()
            .filter(|b| !b.is_empty())
            .ok_or_else(|| {
                SpendPanelError::ParseError("gemini".into(), "no quota buckets in response".into())
            })?;

        let mut map: std::collections::BTreeMap<String, (f64, Option<String>)> =
            std::collections::BTreeMap::new();
        for bucket in buckets {
            let (Some(model_id), Some(fraction)) = (&bucket.model_id, bucket.remaining_fraction)
            else {
                continue;
            };
            map.entry(model_id.clone())
                .and_modify(|existing| {
                    if fraction < existing.0 {
                        *existing = (fraction, bucket.reset_time.clone());
                    }
                })
                .or_insert((fraction, bucket.reset_time.clone()));
        }

        Ok(map
            .into_iter()
            .map(|(model_id, (fraction, reset))| ModelQuota {
                model_id,
                percent_left: fraction * 100.0,
                reset_time: reset
                    .as_deref()
                    .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                    .map(|d| d.with_timezone(&Utc)),
            })
            .collect())
    }

    fn snapshot_from_quotas(quotas: &[ModelQuota]) -> UsageSnapshot {
        let lowest = |pred: fn(&str) -> bool| -> Option<&ModelQuota> {
            quotas
                .iter()
                .filter(|q| pred(&q.model_id.to_lowercase()))
                .min_by(|a, b| a.percent_left.total_cmp(&b.percent_left))
        };
        let window = |q: &ModelQuota, label: &str| -> RateWindow {
            let used = (100.0 - q.percent_left).clamp(0.0, 100.0).round() as u64;
            let mut w = RateWindow::new(used, 100, label.to_string(), 1440);
            w.resets_at = q.reset_time;
            w
        };

        let mut snapshot = UsageSnapshot::new("gemini");
        if let Some(pro) = lowest(is_pro) {
            snapshot.primary_rate_window = Some(window(pro, "Gemini Pro"));
        }
        if let Some(flash) = lowest(is_flash) {
            snapshot.secondary_rate_window = Some(window(flash, "Gemini Flash"));
        }
        if let Some(lite) = lowest(is_flash_lite) {
            snapshot.tertiary_rate_window = Some(window(lite, "Gemini Flash Lite"));
        }
        // Fall back to a plain window when no model matched the known families.
        let needs_fallback = snapshot.primary_rate_window.is_none();
        if let Some(any) = quotas
            .iter()
            .min_by(|a, b| a.percent_left.total_cmp(&b.percent_left))
            .filter(|_| needs_fallback)
        {
            let label = any.model_id.clone();
            snapshot.primary_rate_window = Some(window(any, &label));
        }
        snapshot
    }
}

impl Default for GeminiProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl UsageProvider for GeminiProvider {
    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    fn detect_credentials(&self) -> bool {
        let home = std::env::var("HOME").unwrap_or_default();
        std::path::Path::new(&home)
            .join(".gemini/oauth_creds.json")
            .exists()
    }

    async fn fetch_usage(&self, ctx: &ProviderContext) -> Result<UsageSnapshot, SpendPanelError> {
        let client = Self::build_client(ctx)?;
        let access_token = self.resolve_access_token(ctx, &client).await?;

        let project_id = self.resolve_project_id(&client, ctx, &access_token).await?;

        let quota = self
            .retrieve_quota(&client, &access_token, &project_id)
            .await?;
        let quotas = Self::parse_quota(&quota)?;
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
    use wiremock::matchers::{body_partial_json, header_regex, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const QUOTA: &str = r#"{
      "buckets": [
        {"modelId": "gemini-2.5-pro", "remainingFraction": 0.4, "resetTime": "2026-06-14T00:00:00Z"},
        {"modelId": "gemini-2.5-pro", "remainingFraction": 0.2, "resetTime": "2026-06-14T00:00:00Z"},
        {"modelId": "gemini-2.5-flash", "remainingFraction": 0.9, "resetTime": "2026-06-14T00:00:00Z"},
        {"modelId": "gemini-2.5-flash-lite", "remainingFraction": 1.0, "resetTime": "2026-06-14T00:00:00Z"}
      ]
    }"#;

    fn quota(body: &str) -> QuotaResponse {
        serde_json::from_str(body).unwrap()
    }

    #[test]
    fn test_metadata() {
        let p = GeminiProvider::new();
        assert_eq!(p.metadata().id, "gemini");
    }

    #[test]
    fn test_model_classifiers() {
        assert!(is_flash_lite("gemini-2.5-flash-lite"));
        assert!(is_flash("gemini-2.5-flash"));
        assert!(!is_flash("gemini-2.5-flash-lite"));
        assert!(is_pro("gemini-2.5-pro"));
    }

    #[test]
    fn test_parse_quota_keeps_lowest_per_model() {
        let quotas = GeminiProvider::parse_quota(&quota(QUOTA)).unwrap();
        let pro = quotas.iter().find(|q| q.model_id.contains("pro")).unwrap();
        assert_eq!(pro.percent_left, 20.0); // lowest of 0.4/0.2
    }

    #[test]
    fn test_parse_quota_empty_is_error() {
        let err = GeminiProvider::parse_quota(&quota(r#"{"buckets":[]}"#)).unwrap_err();
        assert!(matches!(err, SpendPanelError::ParseError(_, _)));
    }

    #[test]
    fn test_snapshot_maps_families() {
        let quotas = GeminiProvider::parse_quota(&quota(QUOTA)).unwrap();
        let snapshot = GeminiProvider::snapshot_from_quotas(&quotas);
        // pro 20% left → 80% used
        assert_eq!(snapshot.primary_rate_window.unwrap().used, Some(80));
        // flash 90% left → 10% used
        assert_eq!(snapshot.secondary_rate_window.unwrap().used, Some(10));
        // flash-lite 100% left → 0% used
        assert_eq!(snapshot.tertiary_rate_window.unwrap().used, Some(0));
    }

    #[test]
    fn test_snapshot_unknown_family_falls_back_to_primary() {
        // A model that matches no known family still populates the primary lane.
        let quotas = GeminiProvider::parse_quota(&quota(
            r#"{"buckets":[{"modelId":"some-experimental-model","remainingFraction":0.3}]}"#,
        ))
        .unwrap();
        let snapshot = GeminiProvider::snapshot_from_quotas(&quotas);
        assert_eq!(snapshot.primary_rate_window.unwrap().used, Some(70));
    }

    #[test]
    fn test_build_user_agent() {
        let ua = GeminiProvider::build_user_agent();
        assert!(ua.starts_with("GeminiCLI/0.57.0/gemini-code-assist (linux; "));
        assert!(ua.ends_with("; terminal)"));
        assert!(ua.contains(std::env::consts::ARCH));
    }

    #[test]
    fn test_random_alphanumeric() {
        let a = GeminiProvider::random_alphanumeric(8);
        let b = GeminiProvider::random_alphanumeric(8);
        assert_eq!(a.len(), 8);
        assert!(
            a.bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        );
        assert_ne!(a, b);
    }

    #[test]
    fn test_pick_onboard_tier_defaults_to_legacy() {
        assert_eq!(GeminiProvider::pick_onboard_tier(&[]), LEGACY_TIER_ID);
        assert_eq!(
            GeminiProvider::pick_onboard_tier(&[UserTier {
                id: Some("free-tier".into()),
                is_default: Some(false),
            }]),
            "free-tier"
        );
        assert_eq!(
            GeminiProvider::pick_onboard_tier(&[UserTier {
                id: Some("standard-tier".into()),
                is_default: Some(true),
            }]),
            "standard-tier"
        );
    }

    #[tokio::test]
    async fn test_fetch_usage_with_config_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1internal:loadCodeAssist"))
            .and(header_regex("User-Agent", "^GeminiCLI/"))
            .and(body_partial_json(serde_json::json!({
                "metadata": {
                    "ideType": "IDE_UNSPECIFIED",
                    "platform": "PLATFORM_UNSPECIFIED",
                    "pluginType": "GEMINI"
                }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{"cloudaicompanionProject":"proj-1"}"#,
                "application/json",
            ))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1internal:retrieveUserQuota"))
            .and(body_partial_json(
                serde_json::json!({ "project": "proj-1" }),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_raw(QUOTA, "application/json"))
            .mount(&server)
            .await;

        let provider = GeminiProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("access_token".into(), "ya29-test".into());
        let snapshot = provider.fetch_usage(&ctx).await.unwrap();
        assert_eq!(snapshot.primary_rate_window.unwrap().used, Some(80));
    }

    #[tokio::test]
    async fn test_load_code_assist_sends_user_agent_and_metadata() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1internal:loadCodeAssist"))
            .and(header_regex(
                "User-Agent",
                "^GeminiCLI/0.57.0/gemini-code-assist ",
            ))
            .and(header_regex("x-activity-request-id", "^[a-z0-9]{8}$"))
            .and(body_partial_json(serde_json::json!({
                "metadata": {
                    "ideType": "IDE_UNSPECIFIED",
                    "platform": "PLATFORM_UNSPECIFIED",
                    "pluginType": "GEMINI"
                }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{"cloudaicompanionProject":"proj-1"}"#,
                "application/json",
            ))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1internal:retrieveUserQuota"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(QUOTA, "application/json"))
            .mount(&server)
            .await;

        let provider = GeminiProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("access_token".into(), "ya29-test".into());
        let snapshot = provider.fetch_usage(&ctx).await.unwrap();
        assert_eq!(snapshot.primary_rate_window.unwrap().used, Some(80));
    }

    #[tokio::test]
    async fn test_fetch_usage_onboards_when_no_project() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1internal:loadCodeAssist"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{"allowedTiers":[{"id":"free-tier"}]}"#,
                "application/json",
            ))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1internal:onboardUser"))
            .and(body_partial_json(serde_json::json!({
                "tierId": "free-tier",
                "metadata": { "pluginType": "GEMINI" }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{"done":true,"response":{"cloudaicompanionProject":{"id":"onboarded-1"}}}"#,
                "application/json",
            ))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1internal:retrieveUserQuota"))
            .and(body_partial_json(
                serde_json::json!({ "project": "onboarded-1" }),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_raw(QUOTA, "application/json"))
            .mount(&server)
            .await;

        let provider = GeminiProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("access_token".into(), "ya29-test".into());
        let snapshot = provider.fetch_usage(&ctx).await.unwrap();
        assert_eq!(snapshot.primary_rate_window.unwrap().used, Some(80));
    }

    #[tokio::test]
    async fn test_fetch_usage_no_project_no_tier_is_clear_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1internal:loadCodeAssist"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(r#"{}"#, "application/json"))
            .mount(&server)
            .await;

        let provider = GeminiProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("access_token".into(), "ya29-test".into());
        let err = provider.fetch_usage(&ctx).await.unwrap_err();
        assert!(matches!(
            err,
            SpendPanelError::ProviderError(_, m)
                if m.contains("gemini set project") && m.contains("Google Cloud project")
        ));
    }

    #[tokio::test]
    async fn test_retrieve_quota_401_is_auth_failed() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1internal:loadCodeAssist"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{"cloudaicompanionProject":"proj-1"}"#,
                "application/json",
            ))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1internal:retrieveUserQuota"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let provider = GeminiProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("access_token".into(), "bad".into());
        let err = provider.fetch_usage(&ctx).await.unwrap_err();
        assert!(matches!(err, SpendPanelError::AuthFailed(_, _)));
    }
}
