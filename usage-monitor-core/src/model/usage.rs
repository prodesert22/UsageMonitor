use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Status of a rate limit window.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RateWindowStatus {
    Normal,
    Warning,
    Critical,
    Exhausted,
    Unknown,
}

impl RateWindowStatus {
    pub fn from_ratio(ratio: f64) -> Self {
        if ratio >= 1.0 {
            Self::Exhausted
        } else if ratio >= 0.95 {
            Self::Critical
        } else if ratio >= 0.80 {
            Self::Warning
        } else if ratio >= 0.0 {
            Self::Normal
        } else {
            Self::Unknown
        }
    }
}

/// Rate limit window with current usage, limit, and reset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RateWindow {
    pub label: String,
    pub window_minutes: u32,
    pub usage_ratio: f64,
    pub limit: Option<u64>,
    pub used: Option<u64>,
    pub remaining: Option<u64>,
    pub resets_at: Option<DateTime<Utc>>,
    pub status: RateWindowStatus,
}

impl RateWindow {
    pub fn new(used: u64, limit: u64, label: impl Into<String>, window_minutes: u32) -> Self {
        let ratio = if limit > 0 {
            (used as f64) / (limit as f64)
        } else {
            0.0
        };
        let ratio = ratio.clamp(0.0, 1.0);

        Self {
            label: label.into(),
            window_minutes,
            usage_ratio: ratio,
            limit: Some(limit),
            used: Some(used),
            remaining: Some(limit.saturating_sub(used)),
            resets_at: None,
            status: RateWindowStatus::from_ratio(ratio),
        }
    }

    /// Creates an "empty" window (no data).
    pub fn unknown(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            window_minutes: 0,
            usage_ratio: 0.0,
            limit: None,
            used: None,
            remaining: None,
            resets_at: None,
            status: RateWindowStatus::Unknown,
        }
    }
}

/// Named window (e.g. "Sonnet 5-hour", "Pro Weekly").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamedRateWindow {
    pub id: String,
    pub label: String,
    pub window: RateWindow,
}

/// Complete usage snapshot for a provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageSnapshot {
    pub provider_id: String,
    /// Account this snapshot was fetched for. `None` for the implicit single
    /// account (provider with no configured accounts).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    /// Human-friendly account label, when one is configured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_label: Option<String>,
    /// Account email/identity extracted from the provider's credentials, when
    /// available (e.g. the Codex OAuth id_token). Independent of `account_label`,
    /// which the registry overwrites with the configured label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_email: Option<String>,
    pub collected_at: DateTime<Utc>,
    pub primary_rate_window: Option<RateWindow>,
    pub secondary_rate_window: Option<RateWindow>,
    pub tertiary_rate_window: Option<RateWindow>,
    pub extra_rate_windows: Vec<NamedRateWindow>,
    pub credits: Option<CreditsSnapshot>,
    pub cost: Option<CostSnapshot>,
    pub plan: Option<PlanInfo>,
}

impl UsageSnapshot {
    pub fn new(provider_id: impl Into<String>) -> Self {
        Self {
            provider_id: provider_id.into(),
            account_id: None,
            account_label: None,
            account_email: None,
            collected_at: Utc::now(),
            primary_rate_window: None,
            secondary_rate_window: None,
            tertiary_rate_window: None,
            extra_rate_windows: Vec::new(),
            credits: None,
            cost: None,
            plan: None,
        }
    }
}

// Re-export from submodules to avoid circular dependency
use super::cost::CostSnapshot;
use super::credits::CreditsSnapshot;

/// Plan/subscription information.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanInfo {
    pub name: String,
    pub tier: Option<String>,
    pub features: Vec<String>,
    pub price: Option<f64>,
    pub currency: Option<String>,
    pub billing_period: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_window_new_calculates_ratio() {
        let w = RateWindow::new(45, 100, "RPM", 1);
        assert_eq!(w.usage_ratio, 0.45);
        assert_eq!(w.remaining, Some(55));
        assert_eq!(w.status, RateWindowStatus::Normal);
    }

    #[test]
    fn test_rate_window_exhausted() {
        let w = RateWindow::new(100, 100, "RPM", 1);
        assert_eq!(w.usage_ratio, 1.0);
        assert_eq!(w.remaining, Some(0));
        assert_eq!(w.status, RateWindowStatus::Exhausted);
    }

    #[test]
    fn test_rate_window_warning_at_80() {
        let w = RateWindow::new(80, 100, "test", 1);
        assert_eq!(w.status, RateWindowStatus::Warning);
    }

    #[test]
    fn test_rate_window_critical_at_95() {
        let w = RateWindow::new(95, 100, "test", 1);
        assert_eq!(w.status, RateWindowStatus::Critical);
    }

    #[test]
    fn test_rate_window_normal_below_80() {
        let w = RateWindow::new(79, 100, "test", 1);
        assert_eq!(w.status, RateWindowStatus::Normal);
    }

    #[test]
    fn test_rate_window_uses_saturating_sub_for_remaining() {
        let w = RateWindow::new(150, 100, "test", 1);
        assert_eq!(w.usage_ratio, 1.0);
        assert_eq!(w.remaining, Some(0));
    }

    #[test]
    fn test_rate_window_no_limit() {
        let w = RateWindow::new(0, 0, "test", 1);
        assert_eq!(w.usage_ratio, 0.0);
        assert_eq!(w.limit, Some(0));
        assert_eq!(w.remaining, Some(0));
    }

    #[test]
    fn test_rate_window_unknown() {
        let w = RateWindow::unknown("unused");
        assert_eq!(w.status, RateWindowStatus::Unknown);
        assert!(w.limit.is_none());
        assert!(w.used.is_none());
    }

    #[test]
    fn test_usage_snapshot_new() {
        let s = UsageSnapshot::new("openai");
        assert_eq!(s.provider_id, "openai");
        assert!(s.primary_rate_window.is_none());
        assert!(s.credits.is_none());
    }

    #[test]
    fn test_usage_snapshot_serialization_roundtrip() {
        let s = UsageSnapshot {
            provider_id: "test".into(),
            account_id: None,
            account_label: None,
            account_email: None,
            collected_at: Utc::now(),
            primary_rate_window: Some(RateWindow::new(50, 100, "test", 60)),
            secondary_rate_window: None,
            tertiary_rate_window: None,
            extra_rate_windows: vec![NamedRateWindow {
                id: "extra".into(),
                label: "Extra Window".into(),
                window: RateWindow::new(10, 20, "extra", 300),
            }],
            credits: None,
            cost: None,
            plan: None,
        };

        let json = serde_json::to_string(&s).unwrap();
        let deserialized: UsageSnapshot = serde_json::from_str(&json).unwrap();

        assert_eq!(s.provider_id, deserialized.provider_id);
        assert_eq!(
            s.primary_rate_window.unwrap().usage_ratio,
            deserialized.primary_rate_window.unwrap().usage_ratio
        );
    }

    #[test]
    fn test_status_from_ratio_negative_becomes_unknown() {
        // negative ratio should not happen in practice, but test the fallback
        let s = RateWindowStatus::from_ratio(-0.1);
        assert_eq!(s, RateWindowStatus::Unknown);
    }
}
