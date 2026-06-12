use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

/// Cost snapshot for a provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CostSnapshot {
    pub total_cost: Option<f64>,
    pub currency: String,
    pub daily_costs: Vec<DailyCost>,
    pub spend_limit: Option<SpendLimit>,
}

/// Daily cost.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DailyCost {
    pub date: NaiveDate,
    pub cost: f64,
    pub tokens_input: Option<u64>,
    pub tokens_output: Option<u64>,
    pub requests: Option<u64>,
}

/// Spend limit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpendLimit {
    pub limit: f64,
    pub used: f64,
    pub period: String,
}

impl SpendLimit {
    pub fn ratio(&self) -> f64 {
        if self.limit > 0.0 {
            (self.used / self.limit).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spend_limit_ratio() {
        let sl = SpendLimit {
            limit: 100.0,
            used: 45.0,
            period: "monthly".into(),
        };
        assert!((sl.ratio() - 0.45).abs() < f64::EPSILON);
    }

    #[test]
    fn test_spend_limit_zero_limit() {
        let sl = SpendLimit {
            limit: 0.0,
            used: 50.0,
            period: "monthly".into(),
        };
        assert_eq!(sl.ratio(), 0.0);
    }

    #[test]
    fn test_daily_cost_serialization() {
        let dc = DailyCost {
            date: NaiveDate::from_ymd_opt(2026, 6, 12).unwrap(),
            cost: 1.20,
            tokens_input: Some(45000),
            tokens_output: Some(12000),
            requests: Some(450),
        };
        let json = serde_json::to_string(&dc).unwrap();
        let back: DailyCost = serde_json::from_str(&json).unwrap();
        assert_eq!(dc.date, back.date);
        assert_eq!(dc.cost, back.cost);
    }

    #[test]
    fn test_cost_snapshot_new() {
        let cs = CostSnapshot {
            total_cost: None,
            currency: "USD".into(),
            daily_costs: vec![],
            spend_limit: None,
        };
        assert_eq!(cs.currency, "USD");
        assert!(cs.daily_costs.is_empty());
    }
}
