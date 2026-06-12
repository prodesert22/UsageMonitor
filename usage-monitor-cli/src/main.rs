use anyhow::Result;
use clap::{Parser, Subcommand};
use usage_monitor_core::provider::registry::ProviderRegistry;
use usage_monitor_core::provider::ProviderContext;
use usage_monitor_core::{RateWindow, UsageSnapshot};

#[derive(Parser)]
#[command(name = "usage-monitor", about = "AI API usage monitor for your terminal", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List available providers
    List,
    /// Fetch usage for a provider
    Fetch {
        /// Provider ID (e.g. claude, anthropic, openai)
        provider: String,
        /// Print the full snapshot as JSON
        #[arg(long)]
        json: bool,
        /// API key (alternative to environment variables)
        #[arg(long)]
        api_key: Option<String>,
        /// Credentials file path (claude provider)
        #[arg(long)]
        credentials_path: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let registry = ProviderRegistry::with_defaults();

    match cli.command {
        Command::List => {
            let mut metas = registry.all_metadata();
            metas.sort_by_key(|m| m.id);
            for meta in metas {
                println!("{:<12} {} — {}", meta.id, meta.name, meta.description);
            }
        }
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

            let snapshot = registry
                .fetch(&provider, &ctx)
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))?;

            if json {
                println!("{}", serde_json::to_string_pretty(&snapshot)?);
            } else {
                print_snapshot(&snapshot);
            }
        }
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

fn print_window(w: &RateWindow) {
    let pct = w.usage_ratio * 100.0;
    let filled = (w.usage_ratio * 20.0).round() as usize;
    let bar: String = "█".repeat(filled) + &"░".repeat(20 - filled.min(20));
    let resets = w
        .resets_at
        .map(|r| format!("  resets {}", r.format("%Y-%m-%d %H:%M UTC")))
        .unwrap_or_default();
    println!("{:<22} [{}] {:>5.1}%{}", w.label, bar, pct, resets);
}
