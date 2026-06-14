use anyhow::Result;
use chrono::Datelike;
use usage_monitor_core::provider::registry::AccountTarget;
use usage_monitor_core::{RateWindow, UsageSnapshot};

pub(crate) fn print_result(snapshot: &UsageSnapshot, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(snapshot)?);
    } else {
        print_snapshot(snapshot);
    }
    Ok(())
}

pub(crate) fn snapshot_title(snap: &UsageSnapshot) -> String {
    match (snap.account_label.as_deref(), snap.account_id.as_deref()) {
        (Some(label), _) => format!("{} — {}", snap.provider_id, label),
        (None, Some(id)) => format!("{} ({})", snap.provider_id, id),
        (None, None) => snap.provider_id.clone(),
    }
}

pub(crate) fn target_title(target: &AccountTarget) -> String {
    match &target.label {
        Some(label) => format!("{} — {}", target.provider_id, label),
        None if target.explicit => format!("{} ({})", target.provider_id, target.account_id),
        None => target.provider_id.clone(),
    }
}

fn print_snapshot(snap: &UsageSnapshot) {
    let title = snapshot_title(snap);
    let width = snapshot_text_width(snap, &title);
    print_block_header(&title, width);
    println!("Collected at: {}", fmt_local_datetime(snap.collected_at));
    if let Some(email) = &snap.account_email {
        println!("Account: {}", email);
    }
    if let Some(plan) = &snap.plan {
        println!("Plan: {}", plan.name);
    }
    if snap.provider_id == "opencode-go" {
        print_opencode_windows(snap, width);
    } else {
        if let Some(w) = &snap.primary_rate_window {
            print_window(w);
        }
        if let Some(w) = &snap.secondary_rate_window {
            print_window(w);
        }
        if let Some(w) = &snap.tertiary_rate_window {
            print_window(w);
        }
        for named in &snap.extra_rate_windows {
            print_window(&named.window);
        }
    }
    if let Some(credits) = &snap.credits {
        match (credits.used, credits.total) {
            (Some(used), Some(total)) => println!(
                "Credits: {:.2}/{:.2} {} used",
                used, total, credits.currency
            ),
            _ => println!("Credits: {:.2} {}", credits.balance, credits.currency),
        }
    }
    if let Some(cost) = &snap.cost {
        if let Some(total) = cost.total_cost {
            println!("Cost (period): {:.2} {}", total, cost.currency);
        }
        for day in &cost.daily_costs {
            let tokens = match (day.tokens_input, day.tokens_output) {
                (Some(i), Some(o)) => format!("  in: {} out: {}", i, o),
                _ => String::new(),
            };
            println!(
                "  {}  {:.2} {}{}",
                day.date, day.cost, cost.currency, tokens
            );
        }
    }
}

fn print_opencode_windows(snap: &UsageSnapshot, width: usize) {
    let mut current_workspace: Option<String> = None;
    for w in snap
        .primary_rate_window
        .iter()
        .chain(snap.secondary_rate_window.iter())
        .chain(snap.tertiary_rate_window.iter())
        .chain(snap.extra_rate_windows.iter().map(|named| &named.window))
    {
        let (workspace, label) = split_opencode_workspace_label(&w.label)
            .unwrap_or_else(|| ("Workspace".to_string(), w.label.clone()));
        if current_workspace.as_deref() != Some(workspace.as_str()) {
            print_workspace_header(&workspace, width);
            current_workspace = Some(workspace);
        }
        print_window_with_label(w, &label);
    }
}

pub(crate) fn split_opencode_workspace_label(label: &str) -> Option<(String, String)> {
    for suffix in ["Rolling (5h)", "Weekly", "Monthly"] {
        if let Some(prefix) = label.strip_suffix(suffix).map(str::trim_end)
            && !prefix.is_empty()
        {
            return Some((prefix.to_string(), suffix.to_string()));
        }
    }
    None
}

fn print_workspace_header(name: &str, width: usize) {
    print_block_header(name, width);
}
fn print_block_header(title: &str, width: usize) {
    let (top, title, bottom) = block_header_lines(title, width);
    println!("{}", top);
    println!("{}", title);
    println!("{}", bottom);
}
fn print_window(w: &RateWindow) {
    print_window_with_label(w, &w.label);
}
fn print_window_with_label(w: &RateWindow, label: &str) {
    let pct = w.usage_ratio * 100.0;
    let filled = ((w.usage_ratio * 20.0).round() as usize).min(20);
    let bar: String = "█".repeat(filled) + &"░".repeat(20 - filled);
    let resets = reset_suffix(w);
    let (color, reset) = if use_color() {
        (usage_color(w.usage_ratio), ANSI_RESET)
    } else {
        ("", "")
    };
    println!(
        "{:<22} [{}{}{}] {}{:>5.1}%{}{}",
        label, color, bar, reset, color, pct, reset, resets
    );
}
pub(crate) fn block_header_lines(title: &str, width: usize) -> (String, String, String) {
    let width = width.max(title.chars().count()).max(1);
    ("_".repeat(width), title.to_string(), "─".repeat(width))
}

fn snapshot_text_width(snap: &UsageSnapshot, title: &str) -> usize {
    let mut width = title.chars().count();
    width = width.max(
        format!("Collected at: {}", fmt_local_datetime(snap.collected_at))
            .chars()
            .count(),
    );
    if let Some(email) = &snap.account_email {
        width = width.max(format!("Account: {}", email).chars().count());
    }
    if let Some(plan) = &snap.plan {
        width = width.max(format!("Plan: {}", plan.name).chars().count());
    }
    for w in snapshot_windows(snap) {
        if snap.provider_id == "opencode-go" {
            if let Some((workspace, label)) = split_opencode_workspace_label(&w.label) {
                width = width.max(workspace.chars().count());
                width = width.max(window_line_width(w, &label));
            } else {
                width = width.max(window_line_width(w, &w.label));
            }
        } else {
            width = width.max(window_line_width(w, &w.label));
        }
    }
    if let Some(credits) = &snap.credits {
        let line = match (credits.used, credits.total) {
            (Some(used), Some(total)) => format!(
                "Credits: {:.2}/{:.2} {} used",
                used, total, credits.currency
            ),
            _ => format!("Credits: {:.2} {}", credits.balance, credits.currency),
        };
        width = width.max(line.chars().count());
    }
    if let Some(cost) = &snap.cost {
        if let Some(total) = cost.total_cost {
            width = width.max(
                format!("Cost (period): {:.2} {}", total, cost.currency)
                    .chars()
                    .count(),
            );
        }
        for day in &cost.daily_costs {
            let tokens = match (day.tokens_input, day.tokens_output) {
                (Some(i), Some(o)) => format!("  in: {} out: {}", i, o),
                _ => String::new(),
            };
            width = width.max(
                format!(
                    "  {}  {:.2} {}{}",
                    day.date, day.cost, cost.currency, tokens
                )
                .chars()
                .count(),
            );
        }
    }
    width
}

fn snapshot_windows(snap: &UsageSnapshot) -> Vec<&RateWindow> {
    snap.primary_rate_window
        .iter()
        .chain(snap.secondary_rate_window.iter())
        .chain(snap.tertiary_rate_window.iter())
        .chain(snap.extra_rate_windows.iter().map(|named| &named.window))
        .collect()
}
fn window_line_width(w: &RateWindow, label: &str) -> usize {
    let label_width = label.chars().count().max(22);
    label_width + 30 + reset_suffix(w).chars().count()
}
pub(crate) fn reset_suffix(w: &RateWindow) -> String {
    w.resets_at
        .map(|r| format!("  {}", fmt_reset(r)))
        .unwrap_or_default()
}

pub(crate) fn fmt_local_datetime(dt: chrono::DateTime<chrono::Utc>) -> String {
    let local = dt.with_timezone(&chrono::Local);
    format!(
        "{} {} (UTC{})",
        local.format("%H:%M"),
        local.format("%d/%m/%Y"),
        local.format("%:z")
    )
}
pub(crate) fn fmt_reset(dt: chrono::DateTime<chrono::Utc>) -> String {
    let local = dt.with_timezone(&chrono::Local);
    let now = chrono::Local::now();
    let days = (local.date_naive() - now.date_naive()).num_days();
    let time = local.format("%H:%M");
    let date = if local.year() == now.year() {
        local.format("%d/%m").to_string()
    } else {
        local.format("%d/%m/%Y").to_string()
    };
    match days {
        0 => format!("resets at {time}"),
        1 => format!("resets tomorrow at {time}"),
        -1 => format!("resets yesterday at {time}"),
        _ => format!("resets {} {} at {time}", local.format("%A"), date),
    }
}
const ANSI_GREEN: &str = "\x1b[32m";
const ANSI_YELLOW: &str = "\x1b[33m";
const ANSI_RED: &str = "\x1b[31m";
const ANSI_RESET: &str = "\x1b[0m";
fn usage_color(ratio: f64) -> &'static str {
    if ratio >= 0.90 {
        ANSI_RED
    } else if ratio >= 0.70 {
        ANSI_YELLOW
    } else {
        ANSI_GREEN
    }
}
fn use_color() -> bool {
    use std::io::IsTerminal;
    std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_fmt_local_datetime_shows_local_offset() {
        let s = fmt_local_datetime(chrono::Utc::now());
        assert!(s.contains("(UTC"), "got: {s}");
        assert!(s.contains('/'), "got: {s}");
        assert!(s.contains(':'), "got: {s}");
    }
    #[test]
    fn test_fmt_reset_today_is_time_only() {
        let dt = chrono::Utc::now() + chrono::Duration::seconds(5);
        let s = fmt_reset(dt);
        assert!(s.starts_with("resets at "), "got: {s}");
        assert!(!s.contains("tomorrow"), "got: {s}");
    }
    #[test]
    fn test_fmt_reset_tomorrow() {
        let dt = (chrono::Local::now() + chrono::Duration::days(1)).with_timezone(&chrono::Utc);
        assert!(
            fmt_reset(dt).starts_with("resets tomorrow at "),
            "got: {}",
            fmt_reset(dt)
        );
    }
    #[test]
    fn test_fmt_reset_far_shows_weekday() {
        let dt = (chrono::Local::now() + chrono::Duration::days(5)).with_timezone(&chrono::Utc);
        let s = fmt_reset(dt);
        let weekdays = [
            "Monday",
            "Tuesday",
            "Wednesday",
            "Thursday",
            "Friday",
            "Saturday",
            "Sunday",
        ];
        assert!(weekdays.iter().any(|w| s.contains(w)), "got: {s}");
        assert!(s.contains(" at "), "got: {s}");
    }
    #[test]
    fn test_split_opencode_workspace_label() {
        assert_eq!(
            super::split_opencode_workspace_label("Default Rolling (5h)"),
            Some(("Default".to_string(), "Rolling (5h)".to_string()))
        );
        assert_eq!(
            super::split_opencode_workspace_label("teste2 Monthly"),
            Some(("teste2".to_string(), "Monthly".to_string()))
        );
        assert_eq!(
            super::split_opencode_workspace_label("Seven day sonnet"),
            None
        );
    }
    #[test]
    fn test_block_header_lines() {
        let (top, title, bottom) = block_header_lines("opencode-go", 81);
        assert_eq!(top.chars().count(), 81);
        assert_eq!(title, "opencode-go");
        assert_eq!(bottom, "─".repeat(81));
        let (top, _, bottom) = block_header_lines("longer-than-width", 5);
        assert_eq!(top.chars().count(), "longer-than-width".chars().count());
        assert_eq!(bottom.chars().count(), "longer-than-width".chars().count());
    }
}
