//! Mistral API spend provider (admin.mistral.ai, browser cookie auth).
//!
//! Ports CodexBar's read of `GET /api/billing/v2/usage`, aggregating the
//! current month's metered cost across every usage category into a
//! `CostSnapshot`.

use async_trait::async_trait;
use std::collections::HashMap;

use crate::error::SpendPanelError;
use crate::model::{CostSnapshot, UsageSnapshot};
use crate::provider::{ProviderContext, ProviderMetadata, UsageProvider};

/// Mistral API spend provider.
pub struct MistralProvider {
    metadata: ProviderMetadata,
    base_url: Option<String>,
}

impl MistralProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: "mistral",
                name: "Mistral",
                description: "Mistral API monthly spend monitor (browser cookie)",
                auth_methods: &["cookie", "env"],
                website: Some("https://mistral.ai"),
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
        self.base_url
            .as_deref()
            .unwrap_or("https://admin.mistral.ai")
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
        if let Ok(v) = std::env::var("MISTRAL_COOKIE") {
            let c = Self::clean(&v);
            if !c.is_empty() {
                return Ok(c);
            }
        }
        Err(SpendPanelError::AuthFailed(
            "mistral".into(),
            "no session cookie in cookie config or MISTRAL_COOKIE".into(),
        ))
    }

    fn build_client(ctx: &ProviderContext) -> Result<reqwest::Client, SpendPanelError> {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(ctx.timeout_secs))
            .build()
            .map_err(|e| SpendPanelError::NetworkError(e.to_string()))
    }

    fn price_index(json: &serde_json::Value) -> HashMap<String, f64> {
        let mut index = HashMap::new();
        if let Some(prices) = json.get("prices").and_then(|p| p.as_array()) {
            for price in prices {
                let metric = price.get("billing_metric").and_then(|v| v.as_str());
                let group = price.get("billing_group").and_then(|v| v.as_str());
                let value = price
                    .get("price")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<f64>().ok());
                if let (Some(metric), Some(group), Some(value)) = (metric, group, value) {
                    index.insert(format!("{}::{}", metric, group), value);
                }
            }
        }
        index
    }

    /// Recursively sums `input`/`output`/`cached` usage-entry arrays into a cost,
    /// using the price index keyed by `metric::group`.
    fn accumulate_cost(node: &serde_json::Value, prices: &HashMap<String, f64>, total: &mut f64) {
        match node {
            serde_json::Value::Object(map) => {
                for (key, value) in map {
                    let entries = value
                        .as_array()
                        .filter(|_| matches!(key.as_str(), "input" | "output" | "cached"));
                    if let Some(entries) = entries {
                        for entry in entries {
                            let units = entry
                                .get("value_paid")
                                .and_then(|v| v.as_i64())
                                .or_else(|| entry.get("value").and_then(|v| v.as_i64()))
                                .unwrap_or(0);
                            let metric = entry.get("billing_metric").and_then(|v| v.as_str());
                            let group = entry.get("billing_group").and_then(|v| v.as_str());
                            let price = match (metric, group) {
                                (Some(m), Some(g)) => prices.get(&format!("{}::{}", m, g)),
                                _ => None,
                            };
                            if let Some(price) = price {
                                *total += units as f64 * price;
                            }
                        }
                        continue;
                    }
                    Self::accumulate_cost(value, prices, total);
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    Self::accumulate_cost(item, prices, total);
                }
            }
            _ => {}
        }
    }

    fn parse(body: &str) -> Result<UsageSnapshot, SpendPanelError> {
        let json: serde_json::Value = serde_json::from_str(body)
            .map_err(|e| SpendPanelError::ParseError("mistral".into(), e.to_string()))?;
        let prices = Self::price_index(&json);
        let mut total = 0.0;
        // Sum each top-level usage category (skip the prices array itself).
        if let Some(obj) = json.as_object() {
            for (key, value) in obj {
                if key == "prices" {
                    continue;
                }
                Self::accumulate_cost(value, &prices, &mut total);
            }
        }
        let currency = json
            .get("currency")
            .and_then(|v| v.as_str())
            .unwrap_or("EUR")
            .to_string();

        let mut snapshot = UsageSnapshot::new("mistral");
        snapshot.cost = Some(CostSnapshot {
            total_cost: Some(total.max(0.0)),
            currency,
            daily_costs: Vec::new(),
            spend_limit: None,
        });
        Ok(snapshot)
    }
}

impl Default for MistralProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl UsageProvider for MistralProvider {
    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    fn detect_credentials(&self) -> bool {
        std::env::var("MISTRAL_COOKIE")
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false)
    }

    async fn fetch_usage(&self, ctx: &ProviderContext) -> Result<UsageSnapshot, SpendPanelError> {
        let cookie = Self::resolve_cookie(ctx)?;
        let client = Self::build_client(ctx)?;
        let now = chrono::Utc::now();
        let (month, year) = (chrono::Datelike::month(&now), chrono::Datelike::year(&now));
        let url = format!(
            "{}/api/billing/v2/usage?month={}&year={}",
            self.api_base().trim_end_matches('/'),
            month,
            year
        );
        let mut req = client
            .get(url)
            .header("Accept", "*/*")
            .header("Cookie", &cookie)
            .header("Referer", "https://admin.mistral.ai/organization/usage")
            .header("Origin", "https://admin.mistral.ai");
        if let Some(csrf) = ctx.config.get("csrf_token").filter(|v| !v.is_empty()) {
            req = req.header("X-CSRFTOKEN", csrf);
        }
        let resp = req
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
                "mistral".into(),
                format!("session cookie rejected (HTTP {})", status.as_u16()),
            ));
        }
        if !status.is_success() {
            return Err(SpendPanelError::ProviderError(
                "mistral".into(),
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

    const SAMPLE: &str = r#"{
      "currency": "USD",
      "prices": [
        {"billing_metric": "tokens_in", "billing_group": "mistral-large", "price": "0.001"},
        {"billing_metric": "tokens_out", "billing_group": "mistral-large", "price": "0.003"}
      ],
      "completion": {
        "models": {
          "mistral-large": {
            "input": [{"value": 1000, "billing_metric": "tokens_in", "billing_group": "mistral-large"}],
            "output": [{"value_paid": 500, "billing_metric": "tokens_out", "billing_group": "mistral-large"}]
          }
        }
      }
    }"#;

    #[test]
    fn test_metadata() {
        assert_eq!(MistralProvider::new().metadata().id, "mistral");
    }

    #[test]
    fn test_parse_aggregates_cost() {
        let snap = MistralProvider::parse(SAMPLE).unwrap();
        let cost = snap.cost.unwrap();
        // 1000*0.001 + 500*0.003 = 1.0 + 1.5 = 2.5
        assert_eq!(cost.total_cost, Some(2.5));
        assert_eq!(cost.currency, "USD");
    }

    #[test]
    fn test_aggregates_across_categories() {
        // Cost from both completion and ocr categories is summed.
        let body = r#"{
          "currency": "USD",
          "prices": [
            {"billing_metric": "t_in", "billing_group": "g", "price": "0.01"},
            {"billing_metric": "pages", "billing_group": "ocr", "price": "0.5"}
          ],
          "completion": {"models": {"m": {
            "input": [{"value": 100, "billing_metric": "t_in", "billing_group": "g"}]
          }}},
          "ocr": {"models": {"o": {
            "input": [{"value_paid": 4, "billing_metric": "pages", "billing_group": "ocr"}]
          }}}
        }"#;
        // 100*0.01 + 4*0.5 = 1.0 + 2.0 = 3.0
        assert_eq!(
            MistralProvider::parse(body)
                .unwrap()
                .cost
                .unwrap()
                .total_cost,
            Some(3.0)
        );
    }

    #[test]
    fn test_default_currency_eur() {
        let snap = MistralProvider::parse(r#"{"prices":[],"completion":{"models":{}}}"#).unwrap();
        let cost = snap.cost.unwrap();
        assert_eq!(cost.currency, "EUR");
        assert_eq!(cost.total_cost, Some(0.0));
    }

    #[tokio::test]
    async fn test_fetch_success() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/billing/v2/usage"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(SAMPLE, "application/json"))
            .mount(&server)
            .await;
        let provider = MistralProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("cookie".into(), "sid=abc".into());
        let snap = provider.fetch_usage(&ctx).await.unwrap();
        assert_eq!(snap.cost.unwrap().total_cost, Some(2.5));
    }

    #[tokio::test]
    async fn test_fetch_401() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/billing/v2/usage"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        let provider = MistralProvider::with_base_url(&server.uri());
        let mut ctx = ProviderContext::new();
        ctx.config.insert("cookie".into(), "bad".into());
        assert!(matches!(
            provider.fetch_usage(&ctx).await.unwrap_err(),
            SpendPanelError::AuthFailed(_, _)
        ));
    }
}
