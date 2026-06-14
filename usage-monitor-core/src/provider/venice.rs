use async_trait::async_trait;
use serde::Deserialize;
use serde::de::{self, Deserializer};

use crate::error::SpendPanelError;
use crate::model::{CreditsSnapshot, PlanInfo, RateWindow, RateWindowStatus, UsageSnapshot};
use crate::provider::{ProviderContext, ProviderMetadata, UsageProvider};

#[derive(Debug, serde::Deserialize)]
struct BalanceResponse {
    #[serde(rename = "canConsume")]
    can_consume: bool,
    #[serde(rename = "consumptionCurrency")]
    consumption_currency: Option<String>,
    balances: Balances,
    #[serde(
        rename = "diemEpochAllocation",
        default,
        deserialize_with = "deserialize_opt_f64"
    )]
    diem_epoch_allocation: Option<f64>,
}

#[derive(Debug, serde::Deserialize)]
struct Balances {
    #[serde(default, deserialize_with = "deserialize_opt_f64")]
    diem: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_opt_f64")]
    usd: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
struct VeniceUsage {
    can_consume: bool,
    consumption_currency: Option<String>,
    diem_balance: Option<f64>,
    usd_balance: Option<f64>,
    diem_epoch_allocation: Option<f64>,
}

pub struct VeniceProvider {
    metadata: ProviderMetadata,
    balance_url: Option<String>,
}

impl VeniceProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: "venice",
                name: "Venice",
                description: "Venice DIEM/USD API balance monitor",
                auth_methods: &["api_key", "env"],
                website: Some("https://venice.ai"),
            },
            balance_url: None,
        }
    }

    pub fn with_balance_url(url: &str) -> Self {
        let mut p = Self::new();
        p.balance_url = Some(url.to_string());
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
        for env in ["VENICE_API_KEY", "VENICE_KEY"] {
            if let Ok(value) = std::env::var(env) {
                let cleaned = Self::clean(&value);
                if !cleaned.is_empty() {
                    return Ok(cleaned);
                }
            }
        }
        Err(SpendPanelError::AuthFailed(
            "venice".into(),
            "no API key found in config, token, VENICE_API_KEY, or VENICE_KEY".into(),
        ))
    }

    fn balance_url(&self, ctx: &ProviderContext) -> String {
        ctx.config
            .get("balance_url")
            .or_else(|| ctx.config.get("api_url"))
            .or_else(|| ctx.config.get("base_url"))
            .map(String::as_str)
            .filter(|v| !v.is_empty())
            .map(Self::clean)
            .or_else(|| self.balance_url.clone())
            .unwrap_or_else(|| "https://api.venice.ai/api/v1/billing/balance".into())
    }

    fn build_client(ctx: &ProviderContext) -> Result<reqwest::Client, SpendPanelError> {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(ctx.timeout_secs))
            .build()
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))
    }

    fn parse_response(body: &str) -> Result<VeniceUsage, SpendPanelError> {
        let decoded: BalanceResponse = serde_json::from_str(body)
            .map_err(|e| SpendPanelError::ParseError("venice".into(), e.to_string()))?;
        Ok(VeniceUsage {
            can_consume: decoded.can_consume,
            consumption_currency: decoded.consumption_currency,
            diem_balance: decoded.balances.diem,
            usd_balance: decoded.balances.usd,
            diem_epoch_allocation: decoded.diem_epoch_allocation,
        })
    }

    async fn fetch_balance(
        client: &reqwest::Client,
        url: String,
        api_key: &str,
    ) -> Result<VeniceUsage, SpendPanelError> {
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
                "venice".into(),
                format!("invalid API key (HTTP {})", status.as_u16()),
            ));
        }
        if !status.is_success() {
            return Err(SpendPanelError::ProviderError(
                "venice".into(),
                format!("HTTP {}", status),
            ));
        }
        Self::parse_response(&body)
    }

    fn balance_window(usage: &VeniceUsage) -> RateWindow {
        let active = usage
            .consumption_currency
            .as_deref()
            .map(str::to_ascii_uppercase);
        let (label, ratio) = if !usage.can_consume {
            ("Balance unavailable for API calls".into(), 1.0)
        } else if active.as_deref() == Some("USD") && usage.usd_balance.unwrap_or(0.0) > 0.0 {
            (
                format!("${:.2} USD remaining", usage.usd_balance.unwrap()),
                0.0,
            )
        } else if active.as_deref() != Some("USD") {
            if let (Some(diem), Some(allocation)) =
                (usage.diem_balance, usage.diem_epoch_allocation)
                && allocation > 0.0
            {
                let used = ((allocation - diem) / allocation).clamp(0.0, 1.0);
                (
                    format!("DIEM {:.2} / {:.2} epoch allocation", diem, allocation),
                    used,
                )
            } else if usage.diem_balance.unwrap_or(0.0) > 0.0 {
                (
                    format!("DIEM {:.2} remaining", usage.diem_balance.unwrap()),
                    0.0,
                )
            } else if usage.usd_balance.unwrap_or(0.0) > 0.0 {
                (
                    format!("${:.2} USD remaining", usage.usd_balance.unwrap()),
                    0.0,
                )
            } else {
                ("No Venice API balance available".into(), 1.0)
            }
        } else if usage.usd_balance.unwrap_or(0.0) > 0.0 {
            (
                format!("${:.2} USD remaining", usage.usd_balance.unwrap()),
                0.0,
            )
        } else {
            ("No Venice API balance available".into(), 1.0)
        };

        RateWindow {
            label,
            window_minutes: 0,
            usage_ratio: ratio,
            limit: None,
            used: None,
            remaining: None,
            resets_at: None,
            status: RateWindowStatus::from_ratio(ratio),
        }
    }

    fn snapshot_from_usage(usage: VeniceUsage) -> UsageSnapshot {
        let mut snapshot = UsageSnapshot::new("venice");
        snapshot.primary_rate_window = Some(Self::balance_window(&usage));
        if let Some(usd) = usage.usd_balance {
            snapshot.credits = Some(CreditsSnapshot::new(usd, "USD"));
        } else if let Some(diem) = usage.diem_balance {
            let mut credits = CreditsSnapshot::new(diem, "DIEM");
            credits.total = usage.diem_epoch_allocation;
            snapshot.credits = Some(credits);
        }
        let mut features = Vec::new();
        if let Some(currency) = &usage.consumption_currency {
            features.push(format!("consumption currency: {}", currency));
        }
        features.push(format!("can consume: {}", usage.can_consume));
        snapshot.plan = Some(PlanInfo {
            name: "Venice API".into(),
            tier: None,
            features,
            price: None,
            currency: usage.consumption_currency.clone(),
            billing_period: None,
        });
        snapshot
    }
}

impl Default for VeniceProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl UsageProvider for VeniceProvider {
    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    fn detect_credentials(&self) -> bool {
        ["VENICE_API_KEY", "VENICE_KEY"]
            .iter()
            .any(|env| std::env::var(env).is_ok_and(|v| !Self::clean(&v).is_empty()))
    }

    async fn fetch_usage(&self, ctx: &ProviderContext) -> Result<UsageSnapshot, SpendPanelError> {
        let api_key = Self::resolve_api_key(ctx)?;
        let client = Self::build_client(ctx)?;
        let usage = Self::fetch_balance(&client, self.balance_url(ctx), &api_key).await?;
        Ok(Self::snapshot_from_usage(usage))
    }
}

fn deserialize_opt_f64<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    match value {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::Number(n)) => n
            .as_f64()
            .ok_or_else(|| de::Error::custom("number cannot be represented as f64"))
            .map(Some),
        Some(serde_json::Value::String(s)) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                trimmed.parse::<f64>().map(Some).map_err(de::Error::custom)
            }
        }
        Some(other) => Err(de::Error::custom(format!(
            "expected number/string/null, got {other}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn test_provider_metadata() {
        let meta = VeniceProvider::new().metadata().clone();
        assert_eq!(meta.id, "venice");
        assert_eq!(meta.name, "Venice");
    }

    #[test]
    fn test_parses_string_encoded_balances_and_allocation() {
        let json = r#"{"canConsume":true,"consumptionCurrency":"DIEM","balances":{"diem":"90.50","usd":"25.75"},"diemEpochAllocation":"100.0"}"#;
        let usage = VeniceProvider::parse_response(json).unwrap();
        assert_eq!(usage.diem_balance, Some(90.50));
        assert_eq!(usage.usd_balance, Some(25.75));
        assert_eq!(usage.diem_epoch_allocation, Some(100.0));
    }

    #[test]
    fn test_diem_allocation_progress() {
        let usage = VeniceUsage {
            can_consume: true,
            consumption_currency: Some("DIEM".into()),
            diem_balance: Some(75.0),
            usd_balance: None,
            diem_epoch_allocation: Some(100.0),
        };
        let window = VeniceProvider::balance_window(&usage);
        assert_eq!(window.label, "DIEM 75.00 / 100.00 epoch allocation");
        assert_eq!(window.usage_ratio, 0.25);
    }

    #[test]
    fn test_usd_display_when_active_currency_usd() {
        let usage = VeniceUsage {
            can_consume: true,
            consumption_currency: Some("USD".into()),
            diem_balance: Some(50.0),
            usd_balance: Some(12.34),
            diem_epoch_allocation: Some(100.0),
        };
        let window = VeniceProvider::balance_window(&usage);
        assert_eq!(window.label, "$12.34 USD remaining");
        assert_eq!(window.usage_ratio, 0.0);
    }

    #[test]
    fn test_can_consume_false_exhausts_window() {
        let usage = VeniceUsage {
            can_consume: false,
            consumption_currency: Some("USD".into()),
            diem_balance: None,
            usd_balance: Some(100.0),
            diem_epoch_allocation: None,
        };
        let window = VeniceProvider::balance_window(&usage);
        assert_eq!(window.label, "Balance unavailable for API calls");
        assert_eq!(window.usage_ratio, 1.0);
    }

    #[test]
    fn test_zero_balances() {
        let usage = VeniceProvider::parse_response(
            r#"{"canConsume":true,"consumptionCurrency":"USD","balances":{"diem":0,"usd":0},"diemEpochAllocation":null}"#,
        )
        .unwrap();
        let window = VeniceProvider::balance_window(&usage);
        assert_eq!(window.label, "No Venice API balance available");
        assert_eq!(window.usage_ratio, 1.0);
    }

    #[tokio::test]
    async fn test_fetch_usage_sends_bearer_token() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/balance"))
            .and(header("authorization", "Bearer ven-test"))
            .and(header("accept", "application/json"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{"canConsume":true,"consumptionCurrency":"USD","balances":{"diem":null,"usd":15.5},"diemEpochAllocation":null}"#,
                "application/json",
            ))
            .mount(&server)
            .await;
        let provider = VeniceProvider::with_balance_url(&format!("{}/balance", server.uri()));
        let snapshot = provider
            .fetch_usage(&ProviderContext::with_api_key("ven-test"))
            .await
            .unwrap();
        assert_eq!(
            snapshot.primary_rate_window.unwrap().label,
            "$15.50 USD remaining"
        );
        assert_eq!(snapshot.credits.unwrap().balance, 15.5);
    }

    #[tokio::test]
    async fn test_fetch_usage_401_is_auth_failed() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/balance"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let provider = VeniceProvider::with_balance_url(&format!("{}/balance", server.uri()));
        let err = provider
            .fetch_usage(&ProviderContext::with_api_key("bad"))
            .await
            .unwrap_err();
        assert!(matches!(err, SpendPanelError::AuthFailed(_, _)));
    }
}
