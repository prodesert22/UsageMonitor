use async_trait::async_trait;

use crate::error::SpendPanelError;
use crate::model::{CreditsSnapshot, UsageSnapshot};
use crate::provider::{ProviderContext, ProviderMetadata, UsageProvider};

#[derive(Debug, serde::Deserialize)]
struct DeepSeekBalanceResponse {
    is_available: bool,
    balance_infos: Vec<DeepSeekBalanceInfo>,
}

#[derive(Debug, serde::Deserialize)]
struct DeepSeekBalanceInfo {
    currency: String,
    total_balance: String,
    granted_balance: String,
    topped_up_balance: String,
}

#[derive(Debug, Clone, PartialEq)]
struct ParsedBalance {
    is_available: bool,
    currency: String,
    total_balance: f64,
    granted_balance: f64,
    topped_up_balance: f64,
}

/// DeepSeek API balance provider.
pub struct DeepSeekProvider {
    metadata: ProviderMetadata,
    /// Base URL override for tests.
    base_url: Option<String>,
}

impl DeepSeekProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: "deepseek",
                name: "DeepSeek",
                description: "DeepSeek API balance monitor",
                auth_methods: &["api_key", "env"],
                website: Some("https://platform.deepseek.com"),
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

    fn api_base<'a>(&'a self, ctx: &'a ProviderContext) -> &'a str {
        ctx.config
            .get("base_url")
            .map(String::as_str)
            .filter(|value| !value.is_empty())
            .or(self.base_url.as_deref())
            .unwrap_or("https://api.deepseek.com")
    }

    fn clean_key(raw: &str) -> String {
        let mut value = raw.trim();
        if value.len() >= 2
            && ((value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\'')))
        {
            value = &value[1..value.len() - 1];
        }
        value.trim().to_string()
    }

    fn detect_credentials_from(primary: Option<&str>, fallback: Option<&str>) -> bool {
        primary
            .or(fallback)
            .map(Self::clean_key)
            .is_some_and(|key| !key.is_empty())
    }

    fn resolve_api_key(ctx: &ProviderContext) -> Result<String, SpendPanelError> {
        for key in ["api_key", "token"] {
            if let Some(value) = ctx.config.get(key) {
                let cleaned = Self::clean_key(value);
                if !cleaned.is_empty() {
                    return Ok(cleaned);
                }
            }
        }

        for env in ["DEEPSEEK_API_KEY", "DEEPSEEK_KEY"] {
            if let Ok(value) = std::env::var(env) {
                let cleaned = Self::clean_key(&value);
                if !cleaned.is_empty() {
                    return Ok(cleaned);
                }
            }
        }

        Err(SpendPanelError::AuthFailed(
            "deepseek".into(),
            "no API key found in config, token, DEEPSEEK_API_KEY, or DEEPSEEK_KEY".into(),
        ))
    }

    fn build_client(ctx: &ProviderContext) -> Result<reqwest::Client, SpendPanelError> {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(ctx.timeout_secs))
            .build()
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))
    }

    async fn fetch_balance(
        base_url: &str,
        client: &reqwest::Client,
        api_key: &str,
    ) -> Result<DeepSeekBalanceResponse, SpendPanelError> {
        let resp = client
            .get(format!("{}/user/balance", base_url.trim_end_matches('/')))
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
                "deepseek".into(),
                format!("invalid API key (HTTP {})", status.as_u16()),
            ));
        }
        if !status.is_success() {
            return Err(SpendPanelError::ProviderError(
                "deepseek".into(),
                format!("HTTP {}: {}", status, body),
            ));
        }

        serde_json::from_str(&body)
            .map_err(|e| SpendPanelError::ParseError("deepseek".into(), e.to_string()))
    }

    fn parse_balance_info(info: &DeepSeekBalanceInfo) -> Result<ParsedBalance, SpendPanelError> {
        let parse = |label: &str, value: &str| {
            value.parse::<f64>().map_err(|_| {
                SpendPanelError::ParseError(
                    "deepseek".into(),
                    format!("non-numeric {} balance value: {}", label, value),
                )
            })
        };

        Ok(ParsedBalance {
            is_available: true,
            currency: info.currency.clone(),
            total_balance: parse("total", &info.total_balance)?,
            granted_balance: parse("granted", &info.granted_balance)?,
            topped_up_balance: parse("topped_up", &info.topped_up_balance)?,
        })
    }

    fn select_balance(resp: DeepSeekBalanceResponse) -> Result<ParsedBalance, SpendPanelError> {
        let mut balances = resp
            .balance_infos
            .iter()
            .map(Self::parse_balance_info)
            .collect::<Result<Vec<_>, _>>()?;

        if balances.is_empty() {
            return Ok(ParsedBalance {
                is_available: false,
                currency: "USD".into(),
                total_balance: 0.0,
                granted_balance: 0.0,
                topped_up_balance: 0.0,
            });
        }

        for balance in &mut balances {
            balance.is_available = resp.is_available;
        }

        let selected = balances
            .iter()
            .find(|b| b.currency == "USD" && b.total_balance > 0.0)
            .or_else(|| balances.iter().find(|b| b.total_balance > 0.0))
            .or_else(|| balances.iter().find(|b| b.currency == "USD"))
            .unwrap_or(&balances[0]);

        Ok(selected.clone())
    }

    fn snapshot_from_balance(balance: ParsedBalance) -> UsageSnapshot {
        let mut credits = CreditsSnapshot::new(balance.total_balance, balance.currency.clone());
        credits.bonus = Some(balance.granted_balance);
        credits.purchased = Some(balance.topped_up_balance);

        let mut snapshot = UsageSnapshot::new("deepseek");
        snapshot.credits = Some(credits);
        if !balance.is_available || balance.total_balance <= 0.0 {
            snapshot.primary_rate_window = Some(crate::model::RateWindow::new(1, 1, "Balance", 0));
        } else {
            snapshot.primary_rate_window = Some(crate::model::RateWindow::new(0, 1, "Balance", 0));
        }
        snapshot
    }
}

impl Default for DeepSeekProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl UsageProvider for DeepSeekProvider {
    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    fn detect_credentials(&self) -> bool {
        Self::detect_credentials_from(
            std::env::var("DEEPSEEK_API_KEY").ok().as_deref(),
            std::env::var("DEEPSEEK_KEY").ok().as_deref(),
        )
    }

    async fn fetch_usage(&self, ctx: &ProviderContext) -> Result<UsageSnapshot, SpendPanelError> {
        let api_key = Self::resolve_api_key(ctx)?;
        let client = Self::build_client(ctx)?;
        let response = Self::fetch_balance(self.api_base(ctx), &client, &api_key).await?;
        let balance = Self::select_balance(response)?;
        Ok(Self::snapshot_from_balance(balance))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn response(body: &str) -> DeepSeekBalanceResponse {
        serde_json::from_str(body).unwrap()
    }

    #[test]
    fn test_provider_metadata() {
        let provider = DeepSeekProvider::new();
        let meta = provider.metadata();
        assert_eq!(meta.id, "deepseek");
        assert_eq!(meta.name, "DeepSeek");
        assert!(meta.auth_methods.contains(&"api_key"));
    }

    #[test]
    fn test_clean_key_trims_and_unquotes() {
        assert_eq!(DeepSeekProvider::clean_key("  sk-test  "), "sk-test");
        assert_eq!(DeepSeekProvider::clean_key("\"sk-test\""), "sk-test");
        assert_eq!(DeepSeekProvider::clean_key("'sk-test'"), "sk-test");
    }

    #[test]
    fn test_resolve_api_key_from_context_api_key() {
        let ctx = ProviderContext::with_api_key(" sk-test ");
        assert_eq!(DeepSeekProvider::resolve_api_key(&ctx).unwrap(), "sk-test");
    }

    #[test]
    fn test_resolve_api_key_from_context_token() {
        let mut ctx = ProviderContext::new();
        ctx.config.insert("token".into(), "sk-token".into());
        assert_eq!(DeepSeekProvider::resolve_api_key(&ctx).unwrap(), "sk-token");
    }

    #[test]
    fn test_resolve_api_key_missing_is_error() {
        let err = DeepSeekProvider::resolve_api_key(&ProviderContext::new()).unwrap_err();
        assert!(matches!(err, SpendPanelError::AuthFailed(_, _)));
    }

    #[test]
    fn test_select_balance_prefers_funded_usd() {
        let json = r#"{
          "is_available": true,
          "balance_infos": [
            {"currency":"CNY","total_balance":"100.00","granted_balance":"0.00","topped_up_balance":"100.00"},
            {"currency":"USD","total_balance":"20.00","granted_balance":"5.00","topped_up_balance":"15.00"}
          ]
        }"#;
        let balance = DeepSeekProvider::select_balance(response(json)).unwrap();
        assert_eq!(balance.currency, "USD");
        assert_eq!(balance.total_balance, 20.0);
        assert!(balance.is_available);
    }

    #[test]
    fn test_select_balance_prefers_positive_cny_over_empty_usd() {
        let json = r#"{
          "is_available": true,
          "balance_infos": [
            {"currency":"USD","total_balance":"0.00","granted_balance":"0.00","topped_up_balance":"0.00"},
            {"currency":"CNY","total_balance":"100.00","granted_balance":"0.00","topped_up_balance":"100.00"}
          ]
        }"#;
        let balance = DeepSeekProvider::select_balance(response(json)).unwrap();
        assert_eq!(balance.currency, "CNY");
        assert_eq!(balance.total_balance, 100.0);
    }

    #[test]
    fn test_select_balance_empty_returns_unavailable_usd_zero() {
        let balance = DeepSeekProvider::select_balance(response(
            r#"{"is_available":true,"balance_infos":[]}"#,
        ))
        .unwrap();
        assert_eq!(balance.currency, "USD");
        assert_eq!(balance.total_balance, 0.0);
        assert!(!balance.is_available);
    }

    #[test]
    fn test_select_balance_malformed_number_fails() {
        let err = DeepSeekProvider::select_balance(response(
            r#"{"is_available":true,"balance_infos":[{"currency":"USD","total_balance":"NaN?","granted_balance":"0.00","topped_up_balance":"0.00"}]}"#,
        ))
        .unwrap_err();
        assert!(matches!(err, SpendPanelError::ParseError(_, _)));
    }

    #[test]
    fn test_snapshot_contains_credit_breakdown() {
        let snapshot = DeepSeekProvider::snapshot_from_balance(ParsedBalance {
            is_available: true,
            currency: "USD".into(),
            total_balance: 50.0,
            granted_balance: 10.0,
            topped_up_balance: 40.0,
        });
        let credits = snapshot.credits.unwrap();
        assert_eq!(credits.balance, 50.0);
        assert_eq!(credits.currency, "USD");
        assert_eq!(credits.bonus, Some(10.0));
        assert_eq!(credits.purchased, Some(40.0));
        assert_eq!(snapshot.primary_rate_window.unwrap().used, Some(0));
    }

    #[tokio::test]
    async fn test_fetch_usage_success() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/user/balance"))
            .and(header("authorization", "Bearer sk-test"))
            .and(header("accept", "application/json"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{"is_available":true,"balance_infos":[{"currency":"USD","total_balance":"50.00","granted_balance":"10.00","topped_up_balance":"40.00"}]}"#,
                "application/json",
            ))
            .mount(&server)
            .await;

        let provider = DeepSeekProvider::with_base_url(&server.uri());
        let snapshot = provider
            .fetch_usage(&ProviderContext::with_api_key("sk-test"))
            .await
            .unwrap();
        let credits = snapshot.credits.unwrap();
        assert_eq!(credits.balance, 50.0);
        assert_eq!(credits.bonus, Some(10.0));
        assert_eq!(credits.purchased, Some(40.0));
    }

    #[tokio::test]
    async fn test_fetch_usage_401_is_auth_failed() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/user/balance"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let provider = DeepSeekProvider::with_base_url(&server.uri());
        let err = provider
            .fetch_usage(&ProviderContext::with_api_key("bad"))
            .await
            .unwrap_err();
        assert!(matches!(err, SpendPanelError::AuthFailed(_, _)));
    }
}
