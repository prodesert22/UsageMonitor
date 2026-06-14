use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};

use crate::error::SpendPanelError;
use crate::model::{NamedRateWindow, PlanInfo, RateWindow, RateWindowStatus, UsageSnapshot};
use crate::provider::{ProviderContext, ProviderMetadata, UsageProvider};

#[derive(Debug, serde::Deserialize)]
struct SubscriptionResponse {
    tier: Option<String>,
    character_count: u64,
    character_limit: u64,
    voice_slots_used: Option<u64>,
    professional_voice_slots_used: Option<u64>,
    voice_limit: Option<u64>,
    professional_voice_limit: Option<u64>,
    current_overage: Option<Overage>,
    status: Option<String>,
    next_character_count_reset_unix: Option<i64>,
}

#[derive(Debug, serde::Deserialize, Clone, PartialEq)]
struct Overage {
    amount: Option<String>,
    currency: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct ElevenLabsUsage {
    tier: Option<String>,
    character_count: u64,
    character_limit: u64,
    voice_slots_used: Option<u64>,
    professional_voice_slots_used: Option<u64>,
    voice_limit: Option<u64>,
    professional_voice_limit: Option<u64>,
    current_overage: Option<Overage>,
    status: Option<String>,
    resets_at: Option<DateTime<Utc>>,
}

/// ElevenLabs subscription usage provider.
pub struct ElevenLabsProvider {
    metadata: ProviderMetadata,
    base_url: Option<String>,
}

impl ElevenLabsProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: "elevenlabs",
                name: "ElevenLabs",
                description: "ElevenLabs subscription credit usage monitor",
                auth_methods: &["api_key", "env"],
                website: Some("https://elevenlabs.io"),
            },
            base_url: None,
        }
    }

    pub fn with_base_url(url: &str) -> Self {
        let mut p = Self::new();
        p.base_url = Some(url.to_string());
        p
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

    fn resolve_api_key(ctx: &ProviderContext) -> Result<String, SpendPanelError> {
        for key in ["api_key", "token"] {
            if let Some(value) = ctx.config.get(key) {
                let cleaned = Self::clean(value);
                if !cleaned.is_empty() {
                    return Ok(cleaned);
                }
            }
        }
        for env in ["ELEVENLABS_API_KEY", "XI_API_KEY"] {
            if let Ok(value) = std::env::var(env) {
                let cleaned = Self::clean(&value);
                if !cleaned.is_empty() {
                    return Ok(cleaned);
                }
            }
        }
        Err(SpendPanelError::AuthFailed(
            "elevenlabs".into(),
            "no API key found in config, token, ELEVENLABS_API_KEY, or XI_API_KEY".into(),
        ))
    }

    fn api_base(&self, ctx: &ProviderContext) -> String {
        let configured = ctx
            .config
            .get("api_url")
            .or_else(|| ctx.config.get("base_url"))
            .map(String::as_str)
            .filter(|v| !v.is_empty())
            .map(Self::clean)
            .or_else(|| {
                std::env::var("ELEVENLABS_API_URL")
                    .ok()
                    .map(|v| Self::clean(&v))
            })
            .or_else(|| self.base_url.clone())
            .unwrap_or_else(|| "https://api.elevenlabs.io".into());
        let base = if configured.starts_with("http://") || configured.starts_with("https://") {
            configured
        } else {
            format!("https://{}", configured)
        };
        base.trim_end_matches('/').to_string()
    }

    fn subscription_url(base_url: &str) -> String {
        let base = base_url.trim_end_matches('/');
        if base.ends_with("/v1") {
            format!("{}/user/subscription", base)
        } else {
            format!("{}/v1/user/subscription", base)
        }
    }

    fn build_client(ctx: &ProviderContext) -> Result<reqwest::Client, SpendPanelError> {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(ctx.timeout_secs))
            .build()
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))
    }

    async fn fetch_subscription(
        client: &reqwest::Client,
        url: String,
        api_key: &str,
    ) -> Result<SubscriptionResponse, SpendPanelError> {
        let resp = client
            .get(url)
            .header("xi-api-key", api_key)
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
                "elevenlabs".into(),
                format!("invalid API key (HTTP {})", status.as_u16()),
            ));
        }
        if !status.is_success() {
            return Err(SpendPanelError::ProviderError(
                "elevenlabs".into(),
                format!("HTTP {}", status),
            ));
        }
        serde_json::from_str(&body)
            .map_err(|e| SpendPanelError::ParseError("elevenlabs".into(), e.to_string()))
    }

    fn parse_usage(resp: SubscriptionResponse) -> ElevenLabsUsage {
        ElevenLabsUsage {
            tier: resp.tier,
            character_count: resp.character_count,
            character_limit: resp.character_limit,
            voice_slots_used: resp.voice_slots_used,
            professional_voice_slots_used: resp.professional_voice_slots_used,
            voice_limit: resp.voice_limit,
            professional_voice_limit: resp.professional_voice_limit,
            current_overage: resp.current_overage,
            status: resp.status,
            resets_at: resp
                .next_character_count_reset_unix
                .and_then(|ts| Utc.timestamp_opt(ts, 0).single()),
        }
    }

    fn format_int(value: u64) -> String {
        let s = value.to_string();
        let mut out = String::new();
        for (i, ch) in s.chars().rev().enumerate() {
            if i > 0 && i % 3 == 0 {
                out.push(',');
            }
            out.push(ch);
        }
        out.chars().rev().collect()
    }

    fn display_tier(usage: &ElevenLabsUsage) -> Option<String> {
        let tier = usage
            .tier
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        match (tier, usage.status.as_deref().filter(|s| !s.is_empty())) {
            (Some(tier), Some(status)) if status.to_ascii_lowercase() != "active" => Some(format!(
                "{} · {}",
                tier.replace('_', " ")
                    .split_whitespace()
                    .map(capitalize)
                    .collect::<Vec<_>>()
                    .join(" "),
                status
            )),
            (Some(tier), _) => Some(
                tier.replace('_', " ")
                    .split_whitespace()
                    .map(capitalize)
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            (None, Some(status)) => Some(status.to_string()),
            (None, None) => None,
        }
    }

    fn voice_window(id: &str, label: &str, used: u64, limit: u64) -> NamedRateWindow {
        let ratio = if limit > 0 {
            used as f64 / limit as f64
        } else {
            0.0
        };
        NamedRateWindow {
            id: id.into(),
            label: label.into(),
            window: RateWindow {
                label: format!("{} {} / {}", label, used, limit),
                window_minutes: 0,
                usage_ratio: ratio.clamp(0.0, 1.0),
                limit: Some(limit),
                used: Some(used),
                remaining: Some(limit.saturating_sub(used)),
                resets_at: None,
                status: RateWindowStatus::from_ratio(ratio),
            },
        }
    }

    fn snapshot_from_usage(usage: ElevenLabsUsage) -> UsageSnapshot {
        let ratio = if usage.character_limit > 0 {
            usage.character_count as f64 / usage.character_limit as f64
        } else {
            0.0
        };
        let mut snapshot = UsageSnapshot::new("elevenlabs");
        snapshot.primary_rate_window = Some(RateWindow {
            label: format!(
                "Credits {} / {}",
                Self::format_int(usage.character_count),
                Self::format_int(usage.character_limit)
            ),
            window_minutes: 0,
            usage_ratio: ratio.clamp(0.0, 1.0),
            limit: Some(usage.character_limit),
            used: Some(usage.character_count),
            remaining: Some(usage.character_limit.saturating_sub(usage.character_count)),
            resets_at: usage.resets_at,
            status: RateWindowStatus::from_ratio(ratio),
        });
        let mut extra = Vec::new();
        if let (Some(used), Some(limit)) = (usage.voice_slots_used, usage.voice_limit)
            && limit > 0
        {
            extra.push(Self::voice_window(
                "voice-slots",
                "Voice slots",
                used,
                limit,
            ));
        }
        if let (Some(used), Some(limit)) = (
            usage.professional_voice_slots_used,
            usage.professional_voice_limit,
        ) && limit > 0
        {
            extra.push(Self::voice_window(
                "professional-voices",
                "Professional voices",
                used,
                limit,
            ));
        }
        snapshot.extra_rate_windows = extra;
        let mut features = Vec::new();
        if let Some(status) = &usage.status {
            features.push(format!("status: {}", status));
        }
        if let Some(overage) = &usage.current_overage
            && let Some(amount) = &overage.amount
        {
            features.push(format!(
                "overage: {} {}",
                amount,
                overage.currency.as_deref().unwrap_or("")
            ));
        }
        if let Some(name) = Self::display_tier(&usage) {
            snapshot.plan = Some(PlanInfo {
                name,
                tier: usage.tier.clone(),
                features,
                price: None,
                currency: None,
                billing_period: None,
            });
        }
        snapshot
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first
            .to_uppercase()
            .chain(chars.flat_map(char::to_lowercase))
            .collect(),
        None => String::new(),
    }
}

impl Default for ElevenLabsProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl UsageProvider for ElevenLabsProvider {
    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    fn detect_credentials(&self) -> bool {
        ["ELEVENLABS_API_KEY", "XI_API_KEY"]
            .iter()
            .any(|env| std::env::var(env).is_ok_and(|v| !Self::clean(&v).is_empty()))
    }

    async fn fetch_usage(&self, ctx: &ProviderContext) -> Result<UsageSnapshot, SpendPanelError> {
        let api_key = Self::resolve_api_key(ctx)?;
        let client = Self::build_client(ctx)?;
        let url = Self::subscription_url(&self.api_base(ctx));
        let resp = Self::fetch_subscription(&client, url, &api_key).await?;
        Ok(Self::snapshot_from_usage(Self::parse_usage(resp)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const SAMPLE: &str = r#"{
      "tier":"creator",
      "character_count":25000,
      "character_limit":100000,
      "voice_slots_used":2,
      "voice_limit":10,
      "professional_voice_slots_used":1,
      "professional_voice_limit":2,
      "current_overage":{"amount":"0","currency":"usd"},
      "status":"active",
      "next_character_count_reset_unix":1738356858
    }"#;

    fn sample_response() -> SubscriptionResponse {
        serde_json::from_str(SAMPLE).unwrap()
    }

    #[test]
    fn test_provider_metadata() {
        let meta = ElevenLabsProvider::new().metadata().clone();
        assert_eq!(meta.id, "elevenlabs");
        assert_eq!(meta.name, "ElevenLabs");
    }

    #[test]
    fn test_subscription_url_accepts_versioned_or_root_base_urls() {
        assert_eq!(
            ElevenLabsProvider::subscription_url("https://api.elevenlabs.io"),
            "https://api.elevenlabs.io/v1/user/subscription"
        );
        assert_eq!(
            ElevenLabsProvider::subscription_url("https://api.elevenlabs.io/v1"),
            "https://api.elevenlabs.io/v1/user/subscription"
        );
    }

    #[test]
    fn test_parse_subscription_response_into_usage_snapshot() {
        let usage = ElevenLabsProvider::parse_usage(sample_response());
        assert_eq!(usage.character_count, 25_000);
        assert_eq!(usage.character_limit, 100_000);
        let snapshot = ElevenLabsProvider::snapshot_from_usage(usage);
        let primary = snapshot.primary_rate_window.unwrap();
        assert_eq!(primary.usage_ratio, 0.25);
        assert_eq!(primary.used, Some(25_000));
        assert_eq!(primary.remaining, Some(75_000));
        assert_eq!(primary.label, "Credits 25,000 / 100,000");
        assert_eq!(snapshot.extra_rate_windows.len(), 2);
        assert_eq!(snapshot.plan.unwrap().name, "Creator");
    }

    #[test]
    fn test_display_tier_includes_inactive_status() {
        let mut usage = ElevenLabsProvider::parse_usage(sample_response());
        usage.tier = Some("professional_plus".into());
        usage.status = Some("past_due".into());
        assert_eq!(
            ElevenLabsProvider::display_tier(&usage).unwrap(),
            "Professional Plus · past_due"
        );
    }

    #[tokio::test]
    async fn test_fetch_usage_success_sends_xi_api_key_header() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/user/subscription"))
            .and(header("xi-api-key", "xi-test"))
            .and(header("accept", "application/json"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(SAMPLE, "application/json"))
            .mount(&server)
            .await;

        let provider = ElevenLabsProvider::with_base_url(&server.uri());
        let snapshot = provider
            .fetch_usage(&ProviderContext::with_api_key("xi-test"))
            .await
            .unwrap();
        assert_eq!(snapshot.primary_rate_window.unwrap().usage_ratio, 0.25);
    }

    #[tokio::test]
    async fn test_fetch_usage_401_is_auth_failed() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/user/subscription"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let provider = ElevenLabsProvider::with_base_url(&server.uri());
        let err = provider
            .fetch_usage(&ProviderContext::with_api_key("bad"))
            .await
            .unwrap_err();
        assert!(matches!(err, SpendPanelError::AuthFailed(_, _)));
    }
}
