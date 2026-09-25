use usage_monitor_cli::provider::registry::{AccountTarget, ProviderRegistry};
use usage_monitor_cli::{RateWindow, UsageSnapshot};

use crate::output::fmt_reset;

use super::model::{WidgetProvider, WidgetWindow};

pub(super) fn widget_targets(
    registry: &ProviderRegistry,
    config: &usage_monitor_cli::config::AppConfig,
    provider: Option<&str>,
) -> anyhow::Result<Vec<AccountTarget>> {
    let targets = match provider {
        Some(id) => {
            if registry.get(id).is_none() {
                anyhow::bail!("unknown provider '{}'", id);
            }
            if registry.provider_state(id, config)
                == Some(usage_monitor_cli::ProviderState::Disabled)
            {
                anyhow::bail!(
                    "provider '{}' is disabled; enable it with `usage-monitor-cli enable {}`",
                    id,
                    id
                );
            }
            registry.provider_targets(id, config)
        }
        None => registry.enabled_targets(config),
    };
    Ok(targets)
}

pub(super) fn provider_from_snapshot(snapshot: &UsageSnapshot) -> WidgetProvider {
    let slotted: Vec<(String, &RateWindow)> = [
        ("primary", &snapshot.primary_rate_window),
        ("secondary", &snapshot.secondary_rate_window),
        ("tertiary", &snapshot.tertiary_rate_window),
    ]
    .into_iter()
    .filter_map(|(id, window)| window.as_ref().map(|w| (id.to_string(), w)))
    .collect();
    let windows = slotted
        .iter()
        .map(|(id, window)| (id.clone(), *window))
        .chain(
            snapshot
                .extra_rate_windows
                .iter()
                .filter(|named| !duplicates_slotted(&slotted, named))
                .map(|named| (named.id.clone(), &named.window)),
        )
        .map(|(id, window)| WidgetWindow::from_window(id, window))
        .collect::<Vec<_>>();
    let max_percentage = windows.iter().map(|w| w.percentage).fold(0.0, f64::max);
    WidgetProvider {
        provider_id: snapshot.provider_id.clone(),
        display_name: provider_display_name(&snapshot.provider_id),
        account_id: snapshot.account_id.clone(),
        account_label: snapshot.account_label.clone(),
        account_email: snapshot.account_email.clone(),
        plan: snapshot.plan.as_ref().map(|plan| plan.name.clone()),
        windows,
        max_percentage,
        status: widget_class(max_percentage).to_string(),
        error: None,
        credits: snapshot
            .credits
            .as_ref()
            .and_then(|credits| serde_json::to_value(credits).ok()),
        cost: snapshot
            .cost
            .as_ref()
            .and_then(|cost| serde_json::to_value(cost).ok()),
    }
}

pub(super) fn provider_from_error(target: &AccountTarget, error: String) -> WidgetProvider {
    WidgetProvider {
        provider_id: target.provider_id.clone(),
        display_name: provider_display_name(&target.provider_id),
        account_id: if target.explicit {
            Some(target.account_id.clone())
        } else {
            None
        },
        account_label: target.label.clone(),
        account_email: None,
        plan: None,
        windows: Vec::new(),
        max_percentage: 0.0,
        status: "stale".into(),
        error: Some(error),
        credits: None,
        cost: None,
    }
}

pub(super) fn provider_title(provider: &WidgetProvider) -> String {
    match (
        provider.account_label.as_deref(),
        provider.account_id.as_deref(),
    ) {
        (Some(label), _) => format!("{} — {}", provider.display_name, label),
        (None, Some(id)) => format!("{} ({})", provider.display_name, id),
        (None, None) => provider.display_name.clone(),
    }
}

pub(super) fn provider_tooltip_line(provider: &WidgetProvider) -> String {
    let title = provider.title();
    if let Some(error) = &provider.error {
        return format!("{title}: error — {error}");
    }
    if provider.windows.is_empty() {
        return format!("{title}: no usage windows");
    }
    let parts = provider
        .windows
        .iter()
        .map(|window| {
            let reset = window
                .resets_at
                .as_ref()
                .map(|value| format!(" {value}"))
                .unwrap_or_default();
            format!(
                "{} {}{}",
                window.label,
                format_percent(window.percentage),
                reset
            )
        })
        .collect::<Vec<_>>()
        .join(" · ");
    format!("{title}: {parts}")
}

/// True when a named extra window repeats a slotted window (same label,
/// percentage, and reset): providers such as Antigravity list their quota
/// buckets both as primary/secondary windows and as named extras, and the
/// extensions would otherwise render each value twice.
fn duplicates_slotted(
    slotted: &[(String, &RateWindow)],
    named: &usage_monitor_cli::NamedRateWindow,
) -> bool {
    slotted.iter().any(|(_, window)| {
        window.label == named.label
            && ratio_percentage(window.usage_ratio) == ratio_percentage(named.window.usage_ratio)
            && window.resets_at == named.window.resets_at
    })
}

pub(super) fn window_from_rate(id: String, window: &RateWindow) -> WidgetWindow {
    WidgetWindow {
        id,
        label: window.label.clone(),
        percentage: ratio_percentage(window.usage_ratio),
        status: window.status,
        used: window.used,
        limit: window.limit,
        remaining: window.remaining,
        resets_at: window.resets_at.map(widget_reset_description),
    }
}

fn widget_reset_description(value: chrono::DateTime<chrono::Utc>) -> String {
    let text = fmt_reset(value);
    let Some(first) = text.get(0..1) else {
        return text;
    };
    format!("{}{}", first.to_uppercase(), &text[1..])
}

pub(super) fn ratio_percentage(ratio: f64) -> f64 {
    ((ratio.clamp(0.0, 1.0) * 100.0) * 10.0).round() / 10.0
}

pub(super) fn widget_class(percentage: f64) -> &'static str {
    match percentage {
        95.0..=100.0 => "critical",
        80.0..=94.0 => "warning",
        _ => "ok",
    }
}

pub(super) fn format_percent(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{}%", value as i64)
    } else {
        format!("{:.1}%", value)
    }
}

fn provider_display_name(provider_id: &str) -> String {
    match provider_id {
        "anthropic" => "Anthropic".into(),
        "claude" => "Claude".into(),
        "codex" => "Codex".into(),
        "openai" => "OpenAI".into(),
        "opencode-go" => "OpenCode Go".into(),
        "openrouter" => "OpenRouter".into(),
        "deepseek" => "DeepSeek".into(),
        "groq" => "Groq".into(),
        "llmproxy" => "LLM Proxy".into(),
        "deepgram" => "Deepgram".into(),
        "abacus" => "Abacus".into(),
        "minimax" => "MiniMax".into(),
        "kimik2" => "Kimi K2".into(),
        "kimi" => "Kimi".into(),
        "zai" => "Z.ai".into(),
        "elevenlabs" => "ElevenLabs".into(),
        "mistral" => "Mistral".into(),
        "cursor" => "Cursor".into(),
        "gemini" => "Gemini".into(),
        "antigravity" => "Antigravity".into(),
        other => other.replace(['-', '_'], " ").to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use usage_monitor_cli::NamedRateWindow;

    fn snapshot_with_duplicate_extra() -> UsageSnapshot {
        let mut snapshot = UsageSnapshot::new("antigravity");
        let mut primary = RateWindow::new(3, 100, "Gemini weekly", 10080);
        primary.resets_at = chrono::DateTime::parse_from_rfc3339("2026-10-02T19:05:01Z")
            .ok()
            .map(|d| d.with_timezone(&chrono::Utc));
        snapshot.primary_rate_window = Some(primary.clone());
        snapshot.extra_rate_windows = vec![
            NamedRateWindow {
                id: "antigravity-quota-summary-gemini-weekly".into(),
                label: primary.label.clone(),
                window: primary,
            },
            NamedRateWindow {
                id: "antigravity-quota-summary-3p-weekly".into(),
                label: "Claude/GPT weekly".into(),
                window: RateWindow::new(0, 100, "Claude/GPT weekly", 10080),
            },
        ];
        snapshot
    }

    #[test]
    fn test_duplicate_extra_windows_are_dropped() {
        let provider = provider_from_snapshot(&snapshot_with_duplicate_extra());
        let ids: Vec<&str> = provider.windows.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, ["primary", "antigravity-quota-summary-3p-weekly"]);
        // Labels ride along so extensions can show real window names.
        assert_eq!(provider.windows[0].label, "Gemini weekly");
    }

    #[test]
    fn test_distinct_extra_windows_are_kept() {
        let mut snapshot = UsageSnapshot::new("codex");
        snapshot.primary_rate_window = Some(RateWindow::new(10, 100, "Session (5h)", 300));
        snapshot.extra_rate_windows = vec![NamedRateWindow {
            id: "Additional".into(),
            label: "Additional".into(),
            window: RateWindow::new(0, 100, "Additional", 0),
        }];
        let provider = provider_from_snapshot(&snapshot);
        let ids: Vec<&str> = provider.windows.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, ["primary", "Additional"]);
    }
}
