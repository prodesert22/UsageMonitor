use serde::Serialize;
use usage_monitor_cli::{RateWindow, RateWindowStatus, UsageSnapshot};

#[derive(Debug, Serialize, PartialEq)]
pub(crate) struct WidgetSummary {
    pub(crate) text: String,
    pub(crate) tooltip: String,
    #[serde(rename = "class")]
    pub(crate) class_name: String,
    pub(crate) percentage: u8,
    pub(crate) has_errors: bool,
    pub(crate) providers: Vec<WidgetProvider>,
    pub(crate) updated_at: String,
}

impl WidgetSummary {
    pub(crate) fn empty(message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            text: "—".into(),
            tooltip: message,
            class_name: "stale".into(),
            percentage: 0,
            has_errors: false,
            providers: Vec::new(),
            updated_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        }
    }

    pub(crate) fn from_providers(providers: Vec<WidgetProvider>) -> Self {
        let has_errors = providers.iter().any(|provider| provider.error.is_some());
        let ok_providers = providers
            .iter()
            .filter(|provider| provider.error.is_none())
            .collect::<Vec<_>>();
        if ok_providers.is_empty() {
            let tooltip = providers
                .iter()
                .map(WidgetProvider::tooltip_line)
                .collect::<Vec<_>>()
                .join("\n");
            return Self {
                text: "⚠".into(),
                tooltip: if tooltip.is_empty() {
                    "Usage Monitor: no provider data".into()
                } else {
                    tooltip
                },
                class_name: "stale".into(),
                percentage: 0,
                has_errors,
                providers,
                updated_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            };
        }
        let percentage = ok_providers
            .iter()
            .map(|provider| provider.max_percentage)
            .max()
            .unwrap_or(0);
        let class_name = match (super::payload::widget_class(percentage), has_errors) {
            ("ok", true) => "stale".to_string(),
            (class_name, _) => class_name.to_string(),
        };
        let tooltip = providers
            .iter()
            .map(WidgetProvider::tooltip_line)
            .collect::<Vec<_>>()
            .join("\n");
        Self {
            text: format!("{}%", percentage),
            tooltip,
            class_name,
            percentage,
            has_errors,
            providers,
            updated_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        }
    }
}

#[derive(Debug, Serialize, PartialEq)]
pub(crate) struct WidgetProvider {
    pub(crate) provider_id: String,
    pub(crate) display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) account_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) account_email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) plan: Option<String>,
    pub(crate) windows: Vec<WidgetWindow>,
    pub(crate) max_percentage: u8,
    pub(crate) status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) credits: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) cost: Option<serde_json::Value>,
}

impl WidgetProvider {
    pub(crate) fn from_snapshot(snapshot: &UsageSnapshot) -> Self {
        super::payload::provider_from_snapshot(snapshot)
    }
    pub(crate) fn from_error(
        target: &usage_monitor_cli::provider::registry::AccountTarget,
        error: String,
    ) -> Self {
        super::payload::provider_from_error(target, error)
    }
    pub(super) fn title(&self) -> String {
        super::payload::provider_title(self)
    }
    pub(super) fn tooltip_line(&self) -> String {
        super::payload::provider_tooltip_line(self)
    }
}

#[derive(Debug, Serialize, PartialEq)]
pub(crate) struct WidgetWindow {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) percentage: u8,
    pub(crate) status: RateWindowStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) used: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) limit: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) remaining: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) resets_at: Option<String>,
}

impl WidgetWindow {
    pub(crate) fn from_window(id: String, window: &RateWindow) -> Self {
        super::payload::window_from_rate(id, window)
    }
}
