use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Credits/balance snapshot for a provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreditsSnapshot {
    pub balance: f64,
    pub currency: String,
    pub total: Option<f64>,
    pub used: Option<f64>,
    pub renews_at: Option<DateTime<Utc>>,
    pub bonus: Option<f64>,
    pub purchased: Option<f64>,
}

impl CreditsSnapshot {
    pub fn new(balance: f64, currency: impl Into<String>) -> Self {
        Self {
            balance,
            currency: currency.into(),
            total: None,
            used: None,
            renews_at: None,
            bonus: None,
            purchased: None,
        }
    }

    pub fn usage_ratio(&self) -> Option<f64> {
        match (self.used, self.total) {
            (Some(used), Some(total)) if total > 0.0 => Some((used / total).clamp(0.0, 1.0)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_credits_new() {
        let c = CreditsSnapshot::new(50.0, "USD");
        assert_eq!(c.balance, 50.0);
        assert_eq!(c.currency, "USD");
        assert!(c.total.is_none());
    }

    #[test]
    fn test_credits_usage_ratio() {
        let c = CreditsSnapshot {
            balance: 10.0,
            currency: "USD".into(),
            total: Some(100.0),
            used: Some(90.0),
            renews_at: None,
            bonus: None,
            purchased: None,
        };
        assert_eq!(c.usage_ratio(), Some(0.9));
    }

    #[test]
    fn test_credits_usage_ratio_no_total() {
        let c = CreditsSnapshot::new(50.0, "USD");
        assert_eq!(c.usage_ratio(), None);
    }

    #[test]
    fn test_credits_usage_ratio_zero_total() {
        let c = CreditsSnapshot {
            balance: 0.0,
            currency: "USD".into(),
            total: Some(0.0),
            used: Some(0.0),
            renews_at: None,
            bonus: None,
            purchased: None,
        };
        assert_eq!(c.usage_ratio(), None);
    }

    #[test]
    fn test_credits_serialization() {
        let c = CreditsSnapshot::new(12.50, "USD");
        let json = serde_json::to_string(&c).unwrap();
        let back: CreditsSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(c.balance, back.balance);
        assert_eq!(c.currency, back.currency);
    }
}
