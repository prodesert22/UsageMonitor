use super::{payload::*, *};
use usage_monitor_cli::provider::registry::AccountTarget;
use usage_monitor_cli::{RateWindow, UsageSnapshot};

#[test]
fn test_ratio_percentage_clamps_and_rounds() {
    assert_eq!(ratio_percentage(-0.1), 0);
    assert_eq!(ratio_percentage(0.424), 42);
    assert_eq!(ratio_percentage(0.995), 100);
    assert_eq!(ratio_percentage(2.0), 100);
}

#[test]
fn test_widget_summary_uses_max_percentage_and_warning_class() {
    let mut snap = UsageSnapshot::new("claude");
    snap.account_id = Some("work".into());
    snap.account_label = Some("Work Claude".into());
    snap.primary_rate_window = Some(RateWindow::new(42, 100, "Session", 300));
    snap.secondary_rate_window = Some(RateWindow::new(84, 100, "Weekly", 10_080));
    let payload = WidgetSummary::from_providers(vec![WidgetProvider::from_snapshot(&snap)]);
    assert_eq!(payload.text, "84%");
    assert_eq!(payload.percentage, 84);
    assert_eq!(payload.class_name, "warning");
    assert!(
        payload.tooltip.contains("Claude — Work Claude"),
        "got: {}",
        payload.tooltip
    );
    assert!(
        payload.tooltip.contains("Weekly 84%"),
        "got: {}",
        payload.tooltip
    );
    assert_eq!(payload.providers[0].windows.len(), 2);
}

#[test]
fn test_widget_summary_errors_are_stale_waybar_compatible_json() {
    let target = AccountTarget {
        provider_id: "openai".into(),
        account_id: "default".into(),
        label: None,
        explicit: false,
    };
    let payload = WidgetSummary::from_providers(vec![WidgetProvider::from_error(
        &target,
        "missing API key".into(),
    )]);
    let json = serde_json::to_string(&payload).unwrap();
    assert_eq!(payload.text, "⚠");
    assert_eq!(payload.class_name, "stale");
    assert!(payload.has_errors);
    assert!(json.contains("\"class\":\"stale\""), "got: {json}");
    assert!(json.contains("missing API key"), "got: {json}");
}

#[test]
fn test_widget_tooltip_does_not_duplicate_reset_word() {
    let mut snap = UsageSnapshot::new("claude");
    let mut window = RateWindow::new(50, 100, "Session", 300);
    window.resets_at = Some(chrono::Utc::now() + chrono::Duration::seconds(30));
    snap.primary_rate_window = Some(window);
    let payload = WidgetSummary::from_providers(vec![WidgetProvider::from_snapshot(&snap)]);
    let reset = payload.providers[0].windows[0]
        .resets_at
        .as_deref()
        .unwrap();
    assert!(reset.starts_with("Resets "), "got: {reset}");
    let tooltip = &payload.providers[0].tooltip_line();
    assert!(
        tooltip.contains("Resets at")
            || tooltip.contains("Resets tomorrow at")
            || tooltip.contains("Resets yesterday at")
            || tooltip.contains("Resets "),
        "got: {}",
        tooltip
    );
}
