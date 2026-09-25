//! Antigravity (Google Code Assist) usage provider.
//!
//! Ports CodexBar's Antigravity flows for Linux:
//!
//! - **CLI source (preferred):** runs `agy -p /usage --output-format json`
//!   and parses the weekly quota groups. Works with a plain `agy` login and
//!   needs no credentials file, mirroring CodexBar's local-before-remote
//!   order (`antigravity.cli-https` print-report path).
//! - **Remote source (fallback):** reads Antigravity's Google OAuth
//!   credentials (default `~/.codexbar/antigravity/oauth_creds.json`, or an
//!   explicit `access_token`), refreshes when expired, then reads per-model
//!   daily quotas from Code Assist's `fetchAvailableModels`, falling back to
//!   `retrieveUserQuota` buckets when models carry no consumed quota.
//!
//! The plan label is resolved best-effort from Code Assist's `loadCodeAssist`
//! (`paidTier.name`, e.g. `Plus`, wins, then `planInfo.planType`, then the
//! current tier), whenever an OAuth credentials file is available.

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::error::SpendPanelError;
use crate::model::{NamedRateWindow, PlanInfo, RateWindow, UsageSnapshot};
use crate::provider::{ProviderContext, ProviderMetadata, UsageProvider};

use super::gemini::{GEMINI_CLI_CLIENT_ID, GEMINI_CLI_CLIENT_SECRET};

const CLOUDCODE_BASE: &str = "https://cloudcode-pa.googleapis.com";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

/// Prefix for named windows built from `agy /usage` quota buckets, mirroring
/// CodexBar's `antigravity-quota-summary-` window id prefix.
const CLI_WINDOW_ID_PREFIX: &str = "antigravity-quota-summary-";

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

/// `agy -p /usage --output-format json` report, mirroring CodexBar's
/// `QuotaSummaryCLIReport` (`status` + `command{name,data}`).
#[derive(Debug, serde::Deserialize)]
struct CliUsageReport {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    command: Option<CliReportCommand>,
}

#[derive(Debug, serde::Deserialize)]
struct CliReportCommand {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    data: Option<CliQuotaPayload>,
}

#[derive(Debug, serde::Deserialize)]
struct CliQuotaPayload {
    #[serde(default)]
    groups: Option<Vec<CliQuotaGroup>>,
}

#[derive(Debug, serde::Deserialize)]
struct CliQuotaGroup {
    #[serde(default, alias = "displayName")]
    name: Option<String>,
    #[serde(default, alias = "display_name")]
    display_name: Option<String>,
    #[serde(default)]
    buckets: Option<Vec<CliQuotaBucket>>,
}

#[derive(Debug, serde::Deserialize)]
struct CliQuotaBucket {
    #[serde(default, alias = "bucketId", alias = "bucket_id")]
    id: Option<String>,
    #[serde(default, alias = "displayName", alias = "display_name")]
    name: Option<String>,
    /// Explicit cadence hint (`"weekly"`, `"5-hour"`, …); also used for
    /// kind detection alongside the id/name, normalized the same way.
    #[serde(default)]
    window: Option<String>,
    #[serde(default, alias = "remainingFraction", alias = "remaining_fraction")]
    remaining_fraction: Option<f64>,
    /// Nested `remaining` oneof form (`{remainingFraction}` or
    /// `{case: "remainingFraction", value}`).
    #[serde(default)]
    remaining: Option<CliRemaining>,
    #[serde(default, alias = "resetTime", alias = "reset_time")]
    reset_time: Option<String>,
    #[serde(default)]
    disabled: Option<bool>,
}

#[derive(Debug, serde::Deserialize)]
struct CliRemaining {
    #[serde(default, alias = "remainingFraction", alias = "remaining_fraction")]
    remaining_fraction: Option<f64>,
    #[serde(default, rename = "case")]
    oneof_case: Option<String>,
    #[serde(default)]
    value: Option<f64>,
}

/// `loadCodeAssist` account state used only for the plan label, mirroring the
/// fields CodexBar's remote fetcher resolves (`planInfo`, `currentTier`,
/// `paidTier`).
#[derive(Debug, serde::Deserialize)]
struct CodeAssistPlan {
    #[serde(default, rename = "planInfo")]
    plan_info: Option<CodeAssistPlanInfo>,
    #[serde(default, rename = "currentTier")]
    current_tier: Option<CodeAssistTier>,
    #[serde(default, rename = "paidTier")]
    paid_tier: Option<CodeAssistTier>,
}

#[derive(Debug, serde::Deserialize)]
struct CodeAssistPlanInfo {
    #[serde(default, rename = "planType")]
    plan_type: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct CodeAssistTier {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
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
                description: "Antigravity quota monitor (`agy` CLI or Google OAuth)",
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
        if let Some(p) = ctx
            .config
            .get("credentials_path")
            .map(|s| s.trim())
            .filter(|v| !v.is_empty())
        {
            return crate::provider::expand_credentials_path(p);
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
            if let Some(value) = ctx
                .config
                .get(key)
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
            {
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
        let token = self
            .refresh_access_token(ctx, client, &creds, &refresh)
            .await?;
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
            .or_else(|| {
                std::env::var("ANTIGRAVITY_OAUTH_CLIENT_ID")
                    .ok()
                    .filter(|v| !v.is_empty())
            })
            .or_else(|| creds.client_id.clone());
        let client_secret = ctx
            .config
            .get("client_secret")
            .filter(|v| !v.is_empty())
            .cloned()
            .or_else(|| {
                std::env::var("ANTIGRAVITY_OAUTH_CLIENT_SECRET")
                    .ok()
                    .filter(|v| !v.is_empty())
            })
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
        let url = format!(
            "{}/v1internal:{}",
            self.cloudcode_base().trim_end_matches('/'),
            endpoint
        );
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
            let Some(model_id) = bucket
                .model_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
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

    /// The `agy` binary on `PATH` (plus `~/.local/bin/agy`), if any.
    fn agy_on_path() -> Option<std::path::PathBuf> {
        if let Some(path) = std::env::var_os("PATH").and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|dir| dir.join("agy"))
                .find(|p| p.is_file())
        }) {
            return Some(path);
        }
        let fallback = std::env::var("HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_default()
            .join(".local/bin/agy");
        fallback.is_file().then_some(fallback)
    }

    /// Locates the `agy` binary: an explicit `cli_path` config value wins,
    /// otherwise the first `agy` on `PATH` (plus `~/.local/bin/agy`, where
    /// user installs land even when `PATH` is minimal).
    fn cli_binary(ctx: &ProviderContext) -> Option<std::path::PathBuf> {
        if let Some(path) = ctx
            .config
            .get("cli_path")
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            return Some(std::path::PathBuf::from(path));
        }
        Self::agy_on_path()
    }

    /// Runs `agy -p /usage --output-format json` and parses the quota-group
    /// report (CodexBar's `antigravity.cli-https` print-report path). Needs no
    /// credentials file: `agy` uses its own login.
    async fn fetch_via_cli(&self, ctx: &ProviderContext) -> Result<UsageSnapshot, SpendPanelError> {
        let bin = Self::cli_binary(ctx).ok_or_else(|| {
            SpendPanelError::AuthFailed(
                "antigravity".into(),
                "agy CLI not found on PATH and no credentials at ~/.codexbar/antigravity/oauth_creds.json; \
                 sign in with `agy` (or point `cli_path` at it), or supply an OAuth credentials file"
                    .into(),
            )
        })?;
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(ctx.timeout_secs.max(1)),
            tokio::process::Command::new(&bin)
                .args([
                    "-p",
                    "/usage",
                    "--output-format",
                    "json",
                    "--print-timeout",
                    "90s",
                ])
                .stdin(std::process::Stdio::null())
                .output(),
        )
        .await
        .map_err(|_| {
            SpendPanelError::ProviderError("antigravity".into(), "agy /usage timed out".into())
        })?
        .map_err(|e| SpendPanelError::NetworkError(format!("running agy /usage: {e}")))?;
        if !output.status.success() {
            let detail = String::from_utf8_lossy(&output.stderr);
            let detail = truncate(&detail, 300);
            return Err(SpendPanelError::ProviderError(
                "antigravity".into(),
                format!("agy /usage failed: {detail}"),
            ));
        }
        let report: CliUsageReport = serde_json::from_slice(&output.stdout).map_err(|e| {
            SpendPanelError::ParseError("antigravity".into(), format!("agy /usage report: {e}"))
        })?;
        Self::snapshot_from_cli_report(&report)
    }

    /// Maps a validated CLI usage report onto a snapshot, mirroring CodexBar's
    /// `parseCLIUsageReport` + quota-summary window mapping (Gemini primary,
    /// Claude/GPT secondary, weekly cadence).
    fn snapshot_from_cli_report(report: &CliUsageReport) -> Result<UsageSnapshot, SpendPanelError> {
        let status_ok = report
            .status
            .as_deref()
            .is_some_and(|s| s.eq_ignore_ascii_case("SUCCESS"));
        let command = report.command.as_ref().filter(|_| status_ok);
        let name_ok = command
            .and_then(|c| c.name.as_deref())
            .is_some_and(|n| n.eq_ignore_ascii_case("usage"));
        let payload = command.and_then(|c| c.data.as_ref()).filter(|_| name_ok);
        let groups = payload
            .and_then(|p| p.groups.as_deref())
            .unwrap_or_default();
        let windows = cli_windows(groups);
        if !windows.iter().any(|w| w.usage_known) {
            return Err(SpendPanelError::ParseError(
                "antigravity".into(),
                "agy /usage report has no known quota".into(),
            ));
        }
        // Most-consumed known window per family, mirroring CodexBar's
        // representatives (max used, ties broken by title).
        let pick = |matches: &dyn Fn(&str) -> bool| {
            windows
                .iter()
                .filter(|w| w.usage_known && matches(&w.named.label.to_lowercase()))
                .max_by(|a, b| {
                    a.named.window.used.cmp(&b.named.window.used).then_with(|| {
                        b.named
                            .label
                            .to_lowercase()
                            .cmp(&a.named.label.to_lowercase())
                    })
                })
                .map(|w| w.named.window.clone())
        };
        let mut snapshot = UsageSnapshot::new("antigravity");
        snapshot.primary_rate_window = pick(&|t: &str| t.contains("gemini"));
        snapshot.secondary_rate_window = pick(&|t: &str| t.contains("claude") || t.contains("gpt"));
        // Tertiary stays empty on this source (two quota families at most).
        let all_weekly = windows
            .iter()
            .filter(|w| w.usage_known)
            .all(|w| w.named.window.window_minutes == 10080);
        for w in windows {
            snapshot.extra_rate_windows.push(w.named);
        }
        snapshot.plan = Some(PlanInfo {
            name: "Antigravity".into(),
            tier: None,
            features: Vec::new(),
            price: None,
            currency: None,
            billing_period: all_weekly.then(|| "weekly".into()),
        });
        Ok(snapshot)
    }

    /// Best-effort plan enrichment: queries `loadCodeAssist` with any
    /// available OAuth credentials (Antigravity file first, then the
    /// gemini-cli file) and replaces the snapshot plan when the backend
    /// reports one (`paidTier.name`, e.g. `Plus`, wins). Never fails the
    /// fetch: without credentials or plan info the snapshot passes through.
    async fn with_enriched_plan(
        &self,
        ctx: &ProviderContext,
        mut snapshot: UsageSnapshot,
    ) -> UsageSnapshot {
        let Ok(client) = Self::build_client(ctx) else {
            return snapshot;
        };
        let Some((token, _project)) = self.enrichment_token(ctx, &client).await.ok().flatten()
        else {
            return snapshot;
        };
        let body = serde_json::json!({
            "metadata": {"ideType": "ANTIGRAVITY", "platform": "PLATFORM_UNSPECIFIED", "pluginType": "GEMINI"},
        })
        .to_string();
        let Ok((status, text)) = self
            .post_json(&client, "loadCodeAssist", &token, body)
            .await
        else {
            return snapshot;
        };
        if !status.is_success() {
            return snapshot;
        }
        let Ok(plan) = serde_json::from_str::<CodeAssistPlan>(&text) else {
            return snapshot;
        };
        if let Some(name) = Self::resolve_plan_name(&plan) {
            let tier = plan
                .current_tier
                .as_ref()
                .and_then(|t| t.id.clone())
                .filter(|id| !id.trim().is_empty());
            let billing = snapshot
                .plan
                .as_ref()
                .and_then(|p| p.billing_period.clone());
            snapshot.plan = Some(PlanInfo {
                name,
                tier,
                features: Vec::new(),
                price: None,
                currency: None,
                billing_period: billing,
            });
        }
        snapshot
    }

    /// Resolves the display plan, mirroring CodexBar's resolvers: an explicit
    /// `paidTier.name` (e.g. `Plus`) is authoritative, then
    /// `planInfo.planType`, then the current-tier mapping
    /// (`standard-tier` → `Paid`, `free-tier` → `Free`, `legacy-tier` →
    /// `Legacy`), else the current tier name.
    fn resolve_plan_name(plan: &CodeAssistPlan) -> Option<String> {
        if let Some(name) = plan
            .paid_tier
            .as_ref()
            .and_then(|t| t.name.clone())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
        {
            return Some(name);
        }
        if let Some(plan_type) = plan
            .plan_info
            .as_ref()
            .and_then(|p| p.plan_type.clone())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
        {
            return Some(plan_type);
        }
        match plan
            .current_tier
            .as_ref()
            .and_then(|t| t.id.as_deref())
            .map(str::trim)
        {
            Some("standard-tier") => Some("Paid".into()),
            Some("free-tier") => Some("Free".into()),
            Some("legacy-tier") => Some("Legacy".into()),
            _ => plan
                .current_tier
                .as_ref()
                .and_then(|t| t.name.clone())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
        }
    }

    /// Resolves an access token purely for plan enrichment: explicit config
    /// tokens win, then the Antigravity credentials file (refreshing when
    /// expired), then the gemini-cli credentials file (used as-is when still
    /// valid; its refresh belongs to `fetch gemini`). Returns `None` when no
    /// credentials exist — enrichment is skipped, not failed.
    async fn enrichment_token(
        &self,
        ctx: &ProviderContext,
        client: &reqwest::Client,
    ) -> Result<Option<(String, Option<String>)>, SpendPanelError> {
        for key in ["access_token", "token"] {
            if let Some(value) = ctx
                .config
                .get(key)
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
            {
                let project = ctx.config.get("project").filter(|v| !v.is_empty()).cloned();
                return Ok(Some((value.to_string(), project)));
            }
        }
        if let Some(found) = self
            .file_token(ctx, client, &Self::creds_path(ctx), false)
            .await?
        {
            return Ok(Some(found));
        }
        let gemini_path = std::env::var("HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_default()
            .join(".gemini/oauth_creds.json");
        if let Some(found) = self.file_token(ctx, client, &gemini_path, true).await? {
            return Ok(Some(found));
        }
        Ok(None)
    }

    /// Reads one credentials file for enrichment. Antigravity files refresh
    /// through the provider OAuth client; gemini-cli files are only used when
    /// their token is still valid (`gemini_only`).
    async fn file_token(
        &self,
        ctx: &ProviderContext,
        client: &reqwest::Client,
        path: &std::path::Path,
        gemini_only: bool,
    ) -> Result<Option<(String, Option<String>)>, SpendPanelError> {
        let Ok(data) = std::fs::read_to_string(path) else {
            return Ok(None);
        };
        let Ok(creds) = serde_json::from_str::<OAuthCreds>(&data) else {
            return Ok(None);
        };
        let project = ctx
            .config
            .get("project")
            .filter(|v| !v.is_empty())
            .cloned()
            .or_else(|| creds.project_id.clone());
        let expired = creds
            .expiry_date
            .map(|ms| ms / 1000.0 < chrono::Utc::now().timestamp() as f64)
            .unwrap_or(true);
        if let Some(token) = creds
            .access_token
            .clone()
            .filter(|t| !t.is_empty() && !expired)
        {
            return Ok(Some((token, project)));
        }
        if gemini_only {
            return Ok(None);
        }
        let refresh = creds.refresh_token.clone().filter(|t| !t.is_empty());
        let Some(refresh) = refresh else {
            return Ok(None);
        };
        // The gemini-cli public client backs gemini-format files; Antigravity
        // files carry (or are configured with) their own client.
        let looks_like_gemini = path.ends_with(".gemini/oauth_creds.json");
        let token = if looks_like_gemini {
            self.refresh_with_client(
                client,
                GEMINI_CLI_CLIENT_ID,
                GEMINI_CLI_CLIENT_SECRET,
                &refresh,
            )
            .await
            .ok()
        } else {
            self.refresh_access_token(ctx, client, &creds, &refresh)
                .await
                .ok()
        };
        Ok(token.map(|t| (t, project)))
    }

    /// Token refresh against an explicit OAuth client id/secret pair.
    async fn refresh_with_client(
        &self,
        client: &reqwest::Client,
        client_id: &str,
        client_secret: &str,
        refresh_token: &str,
    ) -> Result<String, SpendPanelError> {
        let params = [
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ];
        let resp = client
            .post(self.token_url())
            .form(&params)
            .send()
            .await
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(SpendPanelError::AuthFailed(
                "antigravity".into(),
                format!("token refresh failed (HTTP {})", resp.status().as_u16()),
            ));
        }
        let body = resp
            .text()
            .await
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))?;
        let parsed: RefreshResponse = serde_json::from_str(&body)
            .map_err(|e| SpendPanelError::ParseError("antigravity".into(), e.to_string()))?;
        Ok(parsed.access_token)
    }

    /// True when the remote Cloud Code path has credentials to try: an
    /// explicit config token or a readable credentials file.
    fn has_remote_creds(&self, ctx: &ProviderContext) -> bool {
        for key in ["access_token", "token"] {
            if ctx
                .config
                .get(key)
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .is_some()
            {
                return true;
            }
        }
        Self::creds_path(ctx).is_file()
    }

    /// Remote Cloud Code path (OAuth credentials required): per-model daily
    /// quotas from `fetchAvailableModels`, falling back to `retrieveUserQuota`.
    async fn fetch_via_remote(
        &self,
        ctx: &ProviderContext,
    ) -> Result<UsageSnapshot, SpendPanelError> {
        let client = Self::build_client(ctx)?;
        let (access_token, project_id) = self.resolve_auth(ctx, &client).await?;
        let quotas = self
            .fetch_model_quotas(&client, &access_token, project_id.as_deref())
            .await?;
        let snapshot = Self::snapshot_from_quotas(&quotas);
        let snapshot = self.with_enriched_plan(ctx, snapshot).await;
        if snapshot.plan.is_some() {
            return Ok(snapshot);
        }
        let mut snapshot = snapshot;
        // The remote path reports daily model windows.
        snapshot.plan = Some(PlanInfo {
            name: "Antigravity".into(),
            tier: None,
            features: Vec::new(),
            price: None,
            currency: None,
            billing_period: Some("daily".into()),
        });
        Ok(snapshot)
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

/// A CLI quota window with its known-usage flag (CodexBar's
/// `NamedRateWindow.usageKnown`: not disabled and carrying a fraction).
struct CliWindow {
    named: NamedRateWindow,
    usage_known: bool,
}

/// Maps CLI quota groups onto named windows, mirroring CodexBar's
/// `quotaSummaryWindows`: Gemini groups first, then Claude/GPT, then the
/// rest; session buckets before weekly ones, stable within equal ranks.
fn cli_windows(groups: &[CliQuotaGroup]) -> Vec<CliWindow> {
    let mut ranked: Vec<(usize, &CliQuotaGroup)> = groups.iter().enumerate().collect();
    ranked.sort_by(|a, b| {
        cli_group_rank(a.1)
            .cmp(&cli_group_rank(b.1))
            .then(a.0.cmp(&b.0))
    });
    let mut out = Vec::new();
    for (_, group) in ranked {
        let group_title = cli_group_title(group);
        let buckets = group.buckets.as_deref().unwrap_or_default();
        // Buckets sort session-first, then weekly, stable within equal ranks
        // (CodexBar's `quotaBucketSortRank`). Kind detection additionally
        // honors the explicit `window` hint the CLI report carries.
        let mut branked: Vec<(usize, &CliQuotaBucket, CliBucketKind)> = buckets
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let kind = cli_bucket_kind(
                    b,
                    b.id.as_deref().unwrap_or(""),
                    b.name.as_deref().unwrap_or(""),
                );
                (i, b, kind)
            })
            .collect();
        branked.sort_by(|a, b| {
            cli_kind_rank(a.2)
                .cmp(&cli_kind_rank(b.2))
                .then(a.0.cmp(&b.0))
        });
        for (_, bucket, kind) in branked {
            let Some(bucket_id) = non_empty(
                bucket
                    .id
                    .as_deref()
                    .or(bucket.name.as_deref())
                    .or(group.name.as_deref())
                    .or(group.display_name.as_deref()),
            ) else {
                continue;
            };
            // CodexBar requires a bucket id; fall back through the bucket
            // name to the group name so id-less buckets still surface.
            let display = non_empty(
                bucket
                    .name
                    .as_deref()
                    .or(group.display_name.as_deref())
                    .or(group.name.as_deref()),
            )
            .unwrap_or_else(|| bucket_id.clone());
            let fraction = bucket.remaining_fraction.or_else(|| {
                bucket.remaining.as_ref().and_then(|r| {
                    r.remaining_fraction.or_else(|| {
                        r.oneof_case
                            .as_deref()
                            .filter(|c| c.eq_ignore_ascii_case("remainingFraction"))
                            .and(r.value)
                    })
                })
            });
            let usage_known = bucket.disabled != Some(true) && fraction.is_some();
            let remaining = fraction
                .map(|f| (f * 100.0).clamp(0.0, 100.0))
                .unwrap_or(100.0);
            let used = (100.0 - remaining).round().clamp(0.0, 100.0) as u64;
            let minutes = match kind {
                CliBucketKind::Session => 300,
                CliBucketKind::Weekly => 10080,
                // Unknown cadence: 0 mirrors other providers with no window.
                CliBucketKind::Other => 0,
            };
            let bucket_title = match kind {
                CliBucketKind::Session => "5-hour".to_string(),
                CliBucketKind::Weekly => "weekly".to_string(),
                CliBucketKind::Other => display,
            };
            let mut window =
                RateWindow::new(used, 100, format!("{group_title} {bucket_title}"), minutes);
            window.resets_at = parse_reset(bucket.reset_time.as_deref());
            out.push(CliWindow {
                named: NamedRateWindow {
                    id: format!("{CLI_WINDOW_ID_PREFIX}{bucket_id}"),
                    label: window.label.clone(),
                    window,
                },
                usage_known,
            });
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CliBucketKind {
    Session,
    Weekly,
    Other,
}

fn non_empty(s: Option<&str>) -> Option<String> {
    s.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn cli_group_title(group: &CliQuotaGroup) -> String {
    let title = group
        .name
        .as_deref()
        .or(group.display_name.as_deref())
        .unwrap_or("")
        .trim();
    let lower = title.to_lowercase();
    if lower.contains("gemini") {
        return "Gemini".to_string();
    }
    if lower.contains("claude") || lower.contains("gpt") {
        return "Claude/GPT".to_string();
    }
    if title.is_empty() {
        return "Quota".to_string();
    }
    title.to_string()
}

fn cli_group_rank(group: &CliQuotaGroup) -> u8 {
    let title = group
        .name
        .as_deref()
        .or(group.display_name.as_deref())
        .unwrap_or("")
        .to_lowercase();
    if title.contains("gemini") {
        0
    } else if title.contains("claude") || title.contains("gpt") {
        1
    } else {
        2
    }
}

fn cli_bucket_kind(bucket: &CliQuotaBucket, bucket_id: &str, display: &str) -> CliBucketKind {
    const SESSION_ALIASES: [&str; 5] = ["session", "5h", "5-hour", "five hour", "five-hour"];
    let mut candidates: Vec<String> = Vec::new();
    for raw in [bucket_id, display, bucket.window.as_deref().unwrap_or("")] {
        let normalized = raw.trim().to_lowercase().replace('_', "-");
        if normalized.is_empty() {
            continue;
        }
        if let Some(stripped) = normalized.strip_suffix(" limit") {
            candidates.push(stripped.to_string());
        }
        for alias in SESSION_ALIASES.iter().chain(std::iter::once(&"weekly")) {
            if normalized.ends_with(&format!("-{alias}")) {
                candidates.push((*alias).to_string());
            }
        }
        candidates.push(normalized);
    }
    if candidates
        .iter()
        .any(|c| SESSION_ALIASES.contains(&c.as_str()))
    {
        return CliBucketKind::Session;
    }
    if candidates.iter().any(|c| c == "weekly") {
        return CliBucketKind::Weekly;
    }
    CliBucketKind::Other
}

fn cli_kind_rank(kind: CliBucketKind) -> u8 {
    match kind {
        CliBucketKind::Session => 0,
        CliBucketKind::Weekly => 1,
        CliBucketKind::Other => 2,
    }
}

/// Truncates process stderr for error messages (bounded, single-line).
fn truncate(s: &str, max_chars: usize) -> String {
    let flat: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max_chars {
        return flat;
    }
    flat.chars().take(max_chars).collect()
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
        let home = std::path::Path::new(&home);
        if home.join(".codexbar/antigravity/oauth_creds.json").exists() {
            return true;
        }
        // A bare `agy` binary is not enough: it must have run as this user
        // (its data dir exists), so sandboxed environments with an inherited
        // PATH never auto-enable.
        Self::agy_on_path().is_some() && home.join(".gemini/antigravity-cli").is_dir()
    }

    async fn fetch_usage(&self, ctx: &ProviderContext) -> Result<UsageSnapshot, SpendPanelError> {
        // Prefer the local `agy` CLI report: it works with a plain `agy`
        // login and needs no credentials file, mirroring CodexBar's
        // local-before-remote source order.
        match self.fetch_via_cli(ctx).await {
            Ok(snapshot) => Ok(self.with_enriched_plan(ctx, snapshot).await),
            Err(cli_err) => {
                // Without remote credentials there is nothing else to try;
                // surface the CLI error (it names both remedies).
                if !self.has_remote_creds(ctx) {
                    return Err(cli_err);
                }
                self.fetch_via_remote(ctx).await
            }
        }
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
        let claude = quotas
            .iter()
            .find(|q| q.model_id == "claude-sonnet")
            .unwrap();
        assert_eq!(claude.label, "Claude Sonnet");
        assert_eq!(claude.percent_left(), 25.0);
    }

    #[test]
    fn test_parse_buckets_keeps_lowest() {
        let resp: RetrieveUserQuotaResponse = serde_json::from_str(BUCKETS).unwrap();
        let quotas = AntigravityProvider::parse_buckets(&resp);
        let claude = quotas
            .iter()
            .find(|q| q.model_id == "claude-sonnet")
            .unwrap();
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
        // Force the remote path: the CLI source is environment-dependent.
        ctx.config
            .insert("cli_path".into(), "/nonexistent/agy".into());
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
        // Force the remote path: the CLI source is environment-dependent.
        ctx.config
            .insert("cli_path".into(), "/nonexistent/agy".into());
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
        // Force the remote path: the CLI source is environment-dependent.
        ctx.config
            .insert("cli_path".into(), "/nonexistent/agy".into());
        let err = provider.fetch_usage(&ctx).await.unwrap_err();
        assert!(matches!(err, SpendPanelError::AuthFailed(_, _)));
    }

    /// Real `agy -p /usage` report shape (weekly groups, snake_case keys).
    const CLI_REPORT: &str = r#"{
      "conversation_id": "",
      "status": "SUCCESS",
      "command": {"name": "usage", "data": {
        "description": "Within each group, models share a weekly limit.",
        "groups": [
          {"name": "Gemini Models", "description": "Models within this group: Gemini Flash, Gemini Pro",
           "buckets": [{"id": "gemini-weekly", "name": "Weekly Limit Remaining", "window": "weekly",
                        "remaining_fraction": 1, "reset_time": "2026-10-02T19:05:01Z"}]},
          {"name": "Claude and GPT models", "description": "Models within this group: Claude Opus, Claude Sonnet, GPT-OSS",
           "buckets": [{"id": "3p-weekly", "name": "Weekly Limit Remaining", "window": "weekly",
                        "remaining_fraction": 0.4, "reset_time": "2026-10-02T19:05:01Z"}]}
        ]
      }}
    }"#;

    #[test]
    fn test_cli_report_weekly_groups() {
        let report: CliUsageReport = serde_json::from_str(CLI_REPORT).unwrap();
        let snapshot = AntigravityProvider::snapshot_from_cli_report(&report).unwrap();
        let primary = snapshot.primary_rate_window.unwrap();
        assert_eq!(primary.label, "Gemini weekly");
        assert_eq!(primary.used, Some(0));
        assert_eq!(primary.window_minutes, 10080);
        assert!(primary.resets_at.is_some());
        let secondary = snapshot.secondary_rate_window.unwrap();
        assert_eq!(secondary.label, "Claude/GPT weekly");
        assert_eq!(secondary.used, Some(60));
        assert!(snapshot.tertiary_rate_window.is_none());
        let plan = snapshot.plan.unwrap();
        assert_eq!(plan.name, "Antigravity");
        assert_eq!(plan.billing_period.as_deref(), Some("weekly"));
        let ids: Vec<&str> = snapshot
            .extra_rate_windows
            .iter()
            .map(|w| w.id.as_str())
            .collect();
        assert_eq!(
            ids,
            [
                "antigravity-quota-summary-gemini-weekly",
                "antigravity-quota-summary-3p-weekly"
            ]
        );
    }

    #[test]
    fn test_cli_report_camel_case_and_nested_remaining() {
        let report: CliUsageReport = serde_json::from_str(
            r#"{"status": "SUCCESS", "command": {"name": "usage", "data": {"groups": [
              {"displayName": "Gemini Models", "buckets": [
                {"bucketId": "gemini-session", "displayName": "Session Remaining",
                 "remaining": {"case": "remainingFraction", "value": 0.5},
                 "resetTime": "2026-09-25T20:00:00Z"}
              ]}
            ]}}}"#,
        )
        .unwrap();
        let snapshot = AntigravityProvider::snapshot_from_cli_report(&report).unwrap();
        let primary = snapshot.primary_rate_window.unwrap();
        assert_eq!(primary.label, "Gemini 5-hour");
        assert_eq!(primary.used, Some(50));
        assert_eq!(primary.window_minutes, 300);
    }

    #[test]
    fn test_cli_report_rejects_bad_reports() {
        for body in [
            r#"{"status": "FAILED", "command": {"name": "usage", "data": {"groups": []}}}"#,
            r#"{"status": "SUCCESS", "command": {"name": "status", "data": {"groups": []}}}"#,
            r#"{"status": "SUCCESS", "command": {"name": "usage", "data": {"groups": [
              {"name": "Gemini Models", "buckets": [
                {"id": "gemini-weekly", "name": "Weekly Limit Remaining", "disabled": true,
                 "remaining_fraction": 1.0}
              ]}]}}}"#,
        ] {
            let report: CliUsageReport = serde_json::from_str(body).unwrap();
            assert!(
                AntigravityProvider::snapshot_from_cli_report(&report).is_err(),
                "must reject: {body}"
            );
        }
    }

    #[test]
    fn test_resolve_plan_name() {
        let plan =
            |paid: Option<&str>, plan_type: Option<&str>, tier: Option<&str>| CodeAssistPlan {
                plan_info: plan_type.map(|p| CodeAssistPlanInfo {
                    plan_type: Some(p.into()),
                }),
                current_tier: tier.map(|t| CodeAssistTier {
                    id: Some(t.into()),
                    name: Some(format!("{t} name")),
                }),
                paid_tier: paid.map(|p| CodeAssistTier {
                    id: None,
                    name: Some(p.into()),
                }),
            };
        // Paid tier name is authoritative (the Plus case).
        assert_eq!(
            AntigravityProvider::resolve_plan_name(&plan(Some("Plus"), None, Some("free-tier"))),
            Some("Plus".into())
        );
        assert_eq!(
            AntigravityProvider::resolve_plan_name(&plan(
                Some("Gemini Code Assist in Google One AI Pro"),
                None,
                Some("standard-tier")
            )),
            Some("Gemini Code Assist in Google One AI Pro".into())
        );
        assert_eq!(
            AntigravityProvider::resolve_plan_name(&plan(None, Some("Pro"), Some("standard-tier"))),
            Some("Pro".into())
        );
        assert_eq!(
            AntigravityProvider::resolve_plan_name(&plan(None, None, Some("standard-tier"))),
            Some("Paid".into())
        );
        assert_eq!(
            AntigravityProvider::resolve_plan_name(&plan(None, None, Some("free-tier"))),
            Some("Free".into())
        );
        assert_eq!(
            AntigravityProvider::resolve_plan_name(&plan(None, None, Some("legacy-tier"))),
            Some("Legacy".into())
        );
        assert_eq!(
            AntigravityProvider::resolve_plan_name(&plan(None, None, None)),
            None
        );
    }

    /// `loadCodeAssist` reporting `paidTier.name = "Plus"` upgrades the
    /// remote snapshot plan to Plus.
    #[tokio::test]
    async fn test_remote_enrichment_upgrades_plan_to_plus() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1internal:fetchAvailableModels"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(MODELS, "application/json"))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1internal:loadCodeAssist"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{"currentTier": {"id": "free-tier"}, "paidTier": {"name": "Plus"}}"#,
                "application/json",
            ))
            .mount(&server)
            .await;

        let provider = AntigravityProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("access_token".into(), "ya29-test".into());
        let snapshot = provider.fetch_via_remote(&ctx).await.unwrap();
        let plan = snapshot.plan.unwrap();
        assert_eq!(plan.name, "Plus");
        assert_eq!(plan.tier.as_deref(), Some("free-tier"));
    }

    /// A `loadCodeAssist` without plan info keeps the generic fallback plan.
    #[tokio::test]
    async fn test_remote_fallback_plan_without_enrichment() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1internal:fetchAvailableModels"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(MODELS, "application/json"))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1internal:loadCodeAssist"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{"allowedTiers": [{"id": "standard-tier"}]}"#,
                "application/json",
            ))
            .mount(&server)
            .await;

        let provider = AntigravityProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("access_token".into(), "ya29-test".into());
        let snapshot = provider.fetch_via_remote(&ctx).await.unwrap();
        let plan = snapshot.plan.unwrap();
        assert_eq!(plan.name, "Antigravity");
        assert_eq!(plan.billing_period.as_deref(), Some("daily"));
    }
}
