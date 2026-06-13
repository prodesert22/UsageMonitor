use anyhow::Result;
use clap::{Parser, Subcommand};
use usage_monitor_core::config::AppConfig;
use usage_monitor_core::provider::registry::ProviderRegistry;
use usage_monitor_core::provider::ProviderContext;
use usage_monitor_core::{ProviderState, RateWindow, UsageSnapshot};

#[derive(Parser)]
#[command(name = "usage-monitor", about = "AI API usage monitor for your terminal", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List available providers and their enabled state
    List,
    /// Enable a provider (persisted in the config file)
    Enable {
        /// Provider ID (e.g. claude, codex, anthropic, openai)
        provider: String,
    },
    /// Disable a provider (persisted in the config file)
    Disable {
        /// Provider ID (e.g. claude, codex, anthropic, openai)
        provider: String,
    },
    /// Return a provider to auto-detection (remove the explicit toggle)
    Auto {
        /// Provider ID (e.g. claude, codex, anthropic, openai)
        provider: String,
    },
    /// Fetch usage for a provider, or for all enabled providers when omitted
    Fetch {
        /// Provider ID (e.g. claude, codex, anthropic, openai)
        provider: Option<String>,
        /// Print the full snapshot as JSON
        #[arg(long)]
        json: bool,
        /// API key (alternative to environment variables)
        #[arg(long)]
        api_key: Option<String>,
        /// Credentials file path (claude/codex providers)
        #[arg(long)]
        credentials_path: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let registry = ProviderRegistry::with_defaults();
    let config = AppConfig::load().map_err(|e| anyhow::anyhow!("{}", e))?;

    match cli.command {
        Command::List => {
            let mut metas = registry.all_metadata();
            metas.sort_by_key(|m| m.id);
            for meta in metas {
                let state = registry
                    .provider_state(meta.id, &config)
                    .expect("registered provider");
                let label = match state {
                    ProviderState::Enabled => "enabled",
                    ProviderState::Disabled => "disabled",
                    ProviderState::AutoEnabled => "enabled (auto)",
                    ProviderState::AutoDisabled => "disabled (auto)",
                };
                println!("{:<12} {:<16} {} — {}", meta.id, label, meta.name, meta.description);
            }
        }
        Command::Enable { provider } => set_enabled(&registry, config, &provider, Some(true))?,
        Command::Disable { provider } => set_enabled(&registry, config, &provider, Some(false))?,
        Command::Auto { provider } => set_enabled(&registry, config, &provider, None)?,
        Command::Fetch {
            provider,
            json,
            api_key,
            credentials_path,
        } => {
            let mut ctx = ProviderContext::new();
            if let Some(key) = api_key {
                ctx.config.insert("api_key".into(), key);
            }
            if let Some(path) = credentials_path {
                ctx.config.insert("credentials_path".into(), path);
            }

            match provider {
                Some(id) => {
                    if registry.provider_state(&id, &config) == Some(ProviderState::Disabled) {
                        anyhow::bail!(
                            "provider '{}' is disabled; enable it with `usage-monitor-cli enable {}`",
                            id,
                            id
                        );
                    }
                    let snapshot = registry
                        .fetch(&id, &ctx)
                        .await
                        .map_err(|e| anyhow::anyhow!("{}", e))?;
                    print_result(&snapshot, json)?;
                }
                None => {
                    let ids = registry.enabled_ids(&config);
                    if ids.is_empty() {
                        println!("No enabled providers. Use `enable <provider>` or set up credentials.");
                        return Ok(());
                    }
                    let mut first = true;
                    for id in ids {
                        if !first {
                            println!();
                        }
                        first = false;
                        match registry.fetch(&id, &ctx).await {
                            Ok(snapshot) => print_result(&snapshot, json)?,
                            Err(e) => println!("{}: error: {}", id, e),
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

fn set_enabled(
    registry: &ProviderRegistry,
    mut config: AppConfig,
    provider: &str,
    enabled: Option<bool>,
) -> Result<()> {
    if registry.get(provider).is_none() {
        anyhow::bail!("unknown provider '{}'", provider);
    }
    match enabled {
        Some(value) => config.set_provider_enabled(provider, value),
        None => config.clear_provider_enabled(provider),
    }
    let path = AppConfig::default_path()
        .ok_or_else(|| anyhow::anyhow!("cannot resolve config path (HOME not set)"))?;
    config
        .save_to_path(&path)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let state = registry
        .provider_state(provider, &config)
        .expect("registered provider");
    let label = match state {
        ProviderState::Enabled => "enabled".to_string(),
        ProviderState::Disabled => "disabled".to_string(),
        ProviderState::AutoEnabled => "auto (currently enabled)".to_string(),
        ProviderState::AutoDisabled => "auto (currently disabled)".to_string(),
    };
    println!("{}: {} ({})", provider, label, path.display());
    Ok(())
}

fn print_result(snapshot: &UsageSnapshot, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(snapshot)?);
    } else {
        print_snapshot(snapshot);
    }
    Ok(())
}

fn print_snapshot(snap: &UsageSnapshot) {
    println!("Provider: {}", snap.provider_id);
    println!("Collected at: {}", snap.collected_at.format("%Y-%m-%d %H:%M:%S UTC"));

    if let Some(plan) = &snap.plan {
        println!("Plan: {}", plan.name);
    }

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

    if let Some(credits) = &snap.credits {
        match (credits.used, credits.total) {
            (Some(used), Some(total)) => {
                println!("Credits: {:.2}/{:.2} {} used", used, total, credits.currency)
            }
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
            println!("  {}  {:.2} {}{}", day.date, day.cost, cost.currency, tokens);
        }
    }
}

const ANSI_GREEN: &str = "\x1b[32m";
const ANSI_YELLOW: &str = "\x1b[33m";
const ANSI_RED: &str = "\x1b[31m";
const ANSI_RESET: &str = "\x1b[0m";

/// Color for a usage ratio: green below 70%, yellow from 70%, red from 90%.
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

fn print_window(w: &RateWindow) {
    let pct = w.usage_ratio * 100.0;
    let filled = (w.usage_ratio * 20.0).round() as usize;
    let bar: String = "█".repeat(filled) + &"░".repeat(20 - filled.min(20));
    let resets = w
        .resets_at
        .map(|r| format!("  resets {}", r.format("%Y-%m-%d %H:%M UTC")))
        .unwrap_or_default();

    let (color, reset) = if use_color() {
        (usage_color(w.usage_ratio), ANSI_RESET)
    } else {
        ("", "")
    };
    println!(
        "{:<22} [{}{}{}] {}{:>5.1}%{}{}",
        w.label, color, bar, reset, color, pct, reset, resets
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usage_color_thresholds() {
        assert_eq!(usage_color(0.0), ANSI_GREEN);
        assert_eq!(usage_color(0.69), ANSI_GREEN);
        assert_eq!(usage_color(0.70), ANSI_YELLOW);
        assert_eq!(usage_color(0.89), ANSI_YELLOW);
        assert_eq!(usage_color(0.90), ANSI_RED);
        assert_eq!(usage_color(1.0), ANSI_RED);
    }
}
