use usage_monitor_core::provider::registry::{AccountTarget, ProviderRegistry};
use usage_monitor_core::{RateWindow, UsageSnapshot};

use crate::output::fmt_reset;

use super::model::{WidgetProvider, WidgetWindow};

pub(super) fn widget_targets(
    registry: &ProviderRegistry,
    config: &usage_monitor_core::config::AppConfig,
    provider: Option<&str>,
) -> anyhow::Result<Vec<AccountTarget>> {
    let targets = match provider {
        Some(id) => {
            if registry.get(id).is_none() {
                anyhow::bail!("unknown provider '{}'", id);
            }
            if registry.provider_state(id, config)
                == Some(usage_monitor_core::ProviderState::Disabled)
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
    let windows = snapshot
        .primary_rate_window
        .iter()
        .map(|w| ("primary".to_string(), w))
        .chain(
            snapshot
                .secondary_rate_window
                .iter()
                .map(|w| ("secondary".to_string(), w)),
        )
        .chain(
            snapshot
                .tertiary_rate_window
                .iter()
                .map(|w| ("tertiary".to_string(), w)),
        )
        .chain(
            snapshot
                .extra_rate_windows
                .iter()
                .map(|named| (named.id.clone(), &named.window)),
        )
        .map(|(id, window)| WidgetWindow::from_window(id, window))
        .collect::<Vec<_>>();
    let max_percentage = windows.iter().map(|w| w.percentage).max().unwrap_or(0);
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
        max_percentage: 0,
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
            format!("{} {}%{}", window.label, window.percentage, reset)
        })
        .collect::<Vec<_>>()
        .join(" · ");
    format!("{title}: {parts}")
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
        resets_at: window.resets_at.map(fmt_reset),
    }
}

pub(super) fn ratio_percentage(ratio: f64) -> u8 {
    (ratio.clamp(0.0, 1.0) * 100.0).round() as u8
}

pub(super) fn widget_class(percentage: u8) -> &'static str {
    match percentage {
        95..=100 => "critical",
        80..=94 => "warning",
        _ => "ok",
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
        "zai" => "Z.ai".into(),
        "elevenlabs" => "ElevenLabs".into(),
        "mistral" => "Mistral".into(),
        "cursor" => "Cursor".into(),
        "gemini" => "Gemini".into(),
        other => other.replace(['-', '_'], " ").to_string(),
    }
}
