use async_trait::async_trait;

use crate::error::SpendPanelError;
use crate::model::{CreditsSnapshot, PlanInfo, UsageSnapshot};
use crate::provider::{ProviderContext, ProviderMetadata, UsageProvider};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MoonshotRegion {
    International,
    China,
}

impl MoonshotRegion {
    fn from_raw(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "china" | "cn" => Self::China,
            _ => Self::International,
        }
    }

    fn base_url(self) -> &'static str {
        match self {
            Self::International => "https://api.moonshot.ai",
            Self::China => "https://api.moonshot.cn",
        }
    }

    fn display_name(self) -> &'static str {
        match self {
            Self::International => "International (api.moonshot.ai)",
            Self::China => "China (api.moonshot.cn)",
        }
    }
}

#[derive(Debug, serde::Deserialize)]
struct BalanceResponse {
    code: i64,
    data: BalanceData,
    scode: String,
    status: bool,
}

#[derive(Debug, serde::Deserialize)]
struct BalanceData {
    available_balance: f64,
    voucher_balance: f64,
    cash_balance: f64,
}

#[derive(Debug, Clone, PartialEq)]
struct MoonshotUsage {
    available_balance: f64,
    voucher_balance: f64,
    cash_balance: f64,
    region: MoonshotRegion,
}

pub struct MoonshotProvider {
    metadata: ProviderMetadata,
    base_url: Option<String>,
}

impl MoonshotProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: "moonshot",
                name: "Moonshot / Kimi API",
                description: "Moonshot / Kimi API balance monitor",
                auth_methods: &["api_key", "env"],
                website: Some("https://platform.moonshot.ai"),
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
        for env in ["MOONSHOT_API_KEY", "MOONSHOT_KEY"] {
            if let Ok(value) = std::env::var(env) {
                let cleaned = Self::clean(&value);
                if !cleaned.is_empty() {
                    return Ok(cleaned);
                }
            }
        }
        Err(SpendPanelError::AuthFailed(
            "moonshot".into(),
            "no API key found in config, token, MOONSHOT_API_KEY, or MOONSHOT_KEY".into(),
        ))
    }

    fn resolve_region(ctx: &ProviderContext) -> MoonshotRegion {
        ctx.config
            .get("region")
            .map(|v| Self::clean(v))
            .or_else(|| {
                std::env::var("MOONSHOT_REGION")
                    .ok()
                    .map(|v| Self::clean(&v))
            })
            .map(|v| MoonshotRegion::from_raw(&v))
            .unwrap_or(MoonshotRegion::International)
    }

    fn balance_url(&self, ctx: &ProviderContext, region: MoonshotRegion) -> String {
        let base = ctx
            .config
            .get("api_url")
            .or_else(|| ctx.config.get("base_url"))
            .map(String::as_str)
            .filter(|v| !v.is_empty())
            .map(Self::clean)
            .or_else(|| self.base_url.clone())
            .unwrap_or_else(|| region.base_url().into());
        let base = if base.starts_with("http://") || base.starts_with("https://") {
            base
        } else {
            format!("https://{}", base)
        };
        format!("{}/v1/users/me/balance", base.trim_end_matches('/'))
    }

    fn build_client(ctx: &ProviderContext) -> Result<reqwest::Client, SpendPanelError> {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(ctx.timeout_secs))
            .build()
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))
    }

    fn parse_response(
        body: &str,
        region: MoonshotRegion,
    ) -> Result<MoonshotUsage, SpendPanelError> {
        let response: BalanceResponse = serde_json::from_str(body)
            .map_err(|e| SpendPanelError::ParseError("moonshot".into(), e.to_string()))?;
        if response.code != 0 || !response.status {
            return Err(SpendPanelError::ProviderError(
                "moonshot".into(),
                format!("code {}, scode {}", response.code, response.scode),
            ));
        }
        Ok(MoonshotUsage {
            available_balance: response.data.available_balance,
            voucher_balance: response.data.voucher_balance,
            cash_balance: response.data.cash_balance,
            region,
        })
    }

    async fn fetch_balance(
        client: &reqwest::Client,
        url: String,
        api_key: &str,
        region: MoonshotRegion,
    ) -> Result<MoonshotUsage, SpendPanelError> {
        let resp = client
            .get(url)
            .header("Authorization", format!("Bearer {}", api_key))
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
                "moonshot".into(),
                format!("invalid API key (HTTP {})", status.as_u16()),
            ));
        }
        if !status.is_success() {
            return Err(SpendPanelError::ProviderError(
                "moonshot".into(),
                format!("HTTP {}", status),
            ));
        }
        Self::parse_response(&body, region)
    }

    fn snapshot_from_usage(usage: MoonshotUsage) -> UsageSnapshot {
        let mut snapshot = UsageSnapshot::new("moonshot");
        let mut credits = CreditsSnapshot::new(usage.available_balance, "USD");
        credits.bonus = Some(usage.voucher_balance);
        credits.purchased = Some(usage.cash_balance);
        snapshot.credits = Some(credits);
        let mut features = vec![format!("region: {}", usage.region.display_name())];
        if usage.cash_balance < 0.0 {
            features.push(format!("cash deficit: ${:.2}", usage.cash_balance.abs()));
        }
        snapshot.plan = Some(PlanInfo {
            name: format!("Balance: ${:.2}", usage.available_balance),
            tier: None,
            features,
            price: None,
            currency: Some("USD".into()),
            billing_period: None,
        });
        snapshot
    }
}

impl Default for MoonshotProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl UsageProvider for MoonshotProvider {
    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    fn detect_credentials(&self) -> bool {
        ["MOONSHOT_API_KEY", "MOONSHOT_KEY"]
            .iter()
            .any(|env| std::env::var(env).is_ok_and(|v| !Self::clean(&v).is_empty()))
    }

    async fn fetch_usage(&self, ctx: &ProviderContext) -> Result<UsageSnapshot, SpendPanelError> {
        let api_key = Self::resolve_api_key(ctx)?;
        let region = Self::resolve_region(ctx);
        let url = self.balance_url(ctx, region);
        let client = Self::build_client(ctx)?;
        let usage = Self::fetch_balance(&client, url, &api_key, region).await?;
        Ok(Self::snapshot_from_usage(usage))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const SAMPLE: &str = r#"{
      "code": 0,
      "data": {"available_balance": 49.58, "voucher_balance": 50.00, "cash_balance": 12.34},
      "scode": "0x0",
      "status": true
    }"#;

    #[test]
    fn test_provider_metadata() {
        let meta = MoonshotProvider::new().metadata().clone();
        assert_eq!(meta.id, "moonshot");
        assert_eq!(meta.name, "Moonshot / Kimi API");
    }

    #[test]
    fn test_regions() {
        assert_eq!(
            MoonshotRegion::International.base_url(),
            "https://api.moonshot.ai"
        );
        assert_eq!(MoonshotRegion::China.base_url(), "https://api.moonshot.cn");
        assert_eq!(MoonshotRegion::from_raw("cn"), MoonshotRegion::China);
    }

    #[test]
    fn test_parse_documented_response() {
        let usage =
            MoonshotProvider::parse_response(SAMPLE, MoonshotRegion::International).unwrap();
        assert_eq!(usage.available_balance, 49.58);
        assert_eq!(usage.voucher_balance, 50.0);
        assert_eq!(usage.cash_balance, 12.34);
        let snapshot = MoonshotProvider::snapshot_from_usage(usage);
        assert_eq!(snapshot.credits.as_ref().unwrap().balance, 49.58);
        assert_eq!(snapshot.credits.as_ref().unwrap().bonus, Some(50.0));
        assert_eq!(snapshot.credits.as_ref().unwrap().purchased, Some(12.34));
        assert_eq!(snapshot.plan.unwrap().name, "Balance: $49.58");
    }

    #[test]
    fn test_negative_cash_balance_is_deficit_feature() {
        let json = r#"{"code":0,"data":{"available_balance":49.58,"voucher_balance":50.0,"cash_balance":-0.42},"scode":"0x0","status":true}"#;
        let usage = MoonshotProvider::parse_response(json, MoonshotRegion::International).unwrap();
        let snapshot = MoonshotProvider::snapshot_from_usage(usage);
        assert!(
            snapshot
                .plan
                .unwrap()
                .features
                .iter()
                .any(|f| f.contains("deficit"))
        );
    }

    #[test]
    fn test_api_code_failure_returns_provider_error() {
        let json = r#"{"code":401,"data":{"available_balance":0,"voucher_balance":0,"cash_balance":0},"scode":"unauthorized","status":false}"#;
        let err =
            MoonshotProvider::parse_response(json, MoonshotRegion::International).unwrap_err();
        assert!(matches!(err, SpendPanelError::ProviderError(_, _)));
    }

    #[tokio::test]
    async fn test_fetch_usage_sends_bearer_token() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/users/me/balance"))
            .and(header("authorization", "Bearer live-token"))
            .and(header("accept", "application/json"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(SAMPLE, "application/json"))
            .mount(&server)
            .await;
        let provider = MoonshotProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::with_api_key(" live-token ");
        ctx.config.insert("region".into(), "china".into());
        let snapshot = provider.fetch_usage(&ctx).await.unwrap();
        assert_eq!(snapshot.credits.unwrap().balance, 49.58);
    }

    #[tokio::test]
    async fn test_fetch_usage_401_is_auth_failed() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/users/me/balance"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let provider = MoonshotProvider::with_base_url(&server.uri());
        let err = provider
            .fetch_usage(&ProviderContext::with_api_key("bad"))
            .await
            .unwrap_err();
        assert!(matches!(err, SpendPanelError::AuthFailed(_, _)));
    }
}
