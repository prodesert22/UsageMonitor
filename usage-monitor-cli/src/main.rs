use anyhow::Result;
use clap::{Parser, Subcommand};
use std::collections::HashMap;
use usage_monitor_core::config::AppConfig;
use usage_monitor_core::provider::ProviderContext;
use usage_monitor_core::provider::registry::ProviderRegistry;
use usage_monitor_core::{ProviderState, RateWindow, UsageSnapshot};

#[derive(Parser)]
#[command(
    name = "usage-monitor",
    about = "AI API usage monitor for your terminal",
    version
)]
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
    /// OpenCode Go provider commands
    #[command(name = "opencode-go", subcommand)]
    OpencodeGo(OpencodeGoCmd),
    /// Claude provider commands
    #[command(name = "claude", subcommand)]
    Claude(ProviderConfigCmd),
    /// Codex provider commands
    #[command(name = "codex", subcommand)]
    Codex(ProviderConfigCmd),
    /// Anthropic provider commands
    #[command(name = "anthropic", subcommand)]
    Anthropic(ProviderConfigCmd),
    /// OpenAI provider commands
    #[command(name = "openai", subcommand)]
    OpenAI(ProviderConfigCmd),
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

#[derive(Subcommand)]
enum ProviderConfigCmd {
    /// Show a provider's config (secret values are masked)
    Show,
    /// Set a config value
    Set { key: String, value: String },
    /// Remove a config key
    Unset { key: String },
}

#[derive(Subcommand)]
enum OpencodeGoCmd {
    /// Show config (token, cookie, workspaces, enabled state)
    Show,
    /// Set a config key (token, cookie, etc.)
    Set { key: String, value: String },
    /// Remove a config key
    Unset { key: String },
    /// Manage tracked workspaces (accepts wrk_... ids or dashboard URLs)
    #[command(subcommand)]
    Workspace(WorkspaceCmd),
}

#[derive(Subcommand)]
enum WorkspaceCmd {
    /// Add a workspace: `opencode-go workspace add wrk_xxx` or a dashboard URL
    /// like `https://opencode.ai/workspace/wrk_xxx/go`. The optional name
    /// overrides the one discovered from the dashboard.
    Add {
        workspace: String,
        /// Display name (e.g. `opencode-go workspace add wrk_xxx "Production"`)
        name: Option<String>,
    },
    /// Remove a workspace
    Remove { workspace: String },
    /// List configured workspaces
    List,
}

const WORKSPACE_PROVIDER: &str = "opencode-go";

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
                println!(
                    "{:<12} {:<16} {} — {}",
                    meta.id, label, meta.name, meta.description
                );
            }
        }
        Command::Enable { provider } => set_enabled(&registry, config, &provider, Some(true))?,
        Command::Disable { provider } => set_enabled(&registry, config, &provider, Some(false))?,
        Command::Auto { provider } => set_enabled(&registry, config, &provider, None)?,
        Command::OpencodeGo(cmd) => match cmd {
            OpencodeGoCmd::Show => {
                handle_provider_config(&registry, config, "opencode-go", ProviderConfigCmd::Show)?
            }
            OpencodeGoCmd::Set { key, value } => handle_provider_config(
                &registry,
                config,
                "opencode-go",
                ProviderConfigCmd::Set { key, value },
            )?,
            OpencodeGoCmd::Unset { key } => handle_provider_config(
                &registry,
                config,
                "opencode-go",
                ProviderConfigCmd::Unset { key },
            )?,
            OpencodeGoCmd::Workspace(cmd) => handle_workspace(config, cmd)?,
        },
        Command::Claude(cmd) => handle_provider_config(&registry, config, "claude", cmd)?,
        Command::Codex(cmd) => handle_provider_config(&registry, config, "codex", cmd)?,
        Command::Anthropic(cmd) => handle_provider_config(&registry, config, "anthropic", cmd)?,
        Command::OpenAI(cmd) => handle_provider_config(&registry, config, "openai", cmd)?,
        Command::Fetch {
            provider,
            json,
            api_key,
            credentials_path,
        } => match provider {
            Some(id) => {
                if registry.provider_state(&id, &config) == Some(ProviderState::Disabled) {
                    anyhow::bail!(
                        "provider '{}' is disabled; enable it with `usage-monitor-cli enable {}`",
                        id,
                        id
                    );
                }
                let ctx = provider_context(
                    &config,
                    &id,
                    api_key.as_deref(),
                    credentials_path.as_deref(),
                );
                let snapshot = registry
                    .fetch(&id, &ctx)
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                print_result(&snapshot, json)?;
            }
            None => {
                let ids = registry.enabled_ids(&config);
                if ids.is_empty() {
                    println!(
                        "No enabled providers. Use `enable <provider>` or set up credentials."
                    );
                    return Ok(());
                }
                let ctx_overrides: HashMap<String, ProviderContext> = ids
                    .iter()
                    .map(|id| {
                        (
                            id.clone(),
                            provider_context(
                                &config,
                                id,
                                api_key.as_deref(),
                                credentials_path.as_deref(),
                            ),
                        )
                    })
                    .collect();
                let results = registry.fetch_enabled(&config, Some(&ctx_overrides)).await;

                let mut first = true;
                for (id, result) in results {
                    if !first {
                        println!();
                    }
                    first = false;
                    match result {
                        Ok(snapshot) => print_result(&snapshot, json)?,
                        Err(e) => println!("{}: error: {}", id, e),
                    }
                }
            }
        },
    }

    Ok(())
}

fn save_config(config: &AppConfig) -> Result<std::path::PathBuf> {
    let path = AppConfig::default_path()
        .ok_or_else(|| anyhow::anyhow!("cannot resolve config path (HOME not set)"))?;
    config
        .save_to_path(&path)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(path)
}

/// Masks secret-looking config values for display.
fn mask_value(key: &str, value: &str) -> String {
    let secret = ["cookie", "api_key", "token", "access_token"].contains(&key);
    if secret && value.len() > 12 {
        format!("{}… ({} chars)", &value[..8], value.len())
    } else {
        value.to_string()
    }
}

fn handle_provider_config(
    registry: &ProviderRegistry,
    mut config: AppConfig,
    provider_id: &str,
    cmd: ProviderConfigCmd,
) -> Result<()> {
    if registry.get(provider_id).is_none() {
        anyhow::bail!("unknown provider '{}'", provider_id);
    }
    match cmd {
        ProviderConfigCmd::Show => {
            let enabled = registry
                .provider_state(provider_id, &config)
                .expect("registered provider");
            let label = match enabled {
                ProviderState::Enabled => "enabled",
                ProviderState::Disabled => "disabled",
                ProviderState::AutoEnabled => "enabled (auto)",
                ProviderState::AutoDisabled => "disabled (auto)",
            };
            println!("provider = {}", provider_id);
            println!("state = {}", label);
            if let Some(token) = config.provider_token(provider_id) {
                println!("token = {}", mask_value("token", token));
            }
            if let Some(map) = config.provider_config(provider_id) {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                for key in keys {
                    println!("{} = {}", key, mask_value(key, &map[key]));
                }
            }
            if provider_id == WORKSPACE_PROVIDER {
                let current = config.provider_workspaces(WORKSPACE_PROVIDER).to_vec();
                if current.is_empty() {
                    println!("(no workspaces configured — auto-discovery will be used)");
                } else {
                    for entry in &current {
                        match usage_monitor_core::provider::opencode_go::parse_workspace_entry(
                            entry,
                        ) {
                            Some(ws) => match &ws.name {
                                Some(name) => println!("{:<30} {}", ws.id, name),
                                None => println!("{}", ws.id),
                            },
                            None => println!("{}", entry),
                        }
                    }
                }
            }
        }
        ProviderConfigCmd::Set { key, value } => {
            config.set_provider_config(provider_id, &key, &value);
            let path = save_config(&config)?;
            println!(
                "{}.{} = {} ({})",
                provider_id,
                key,
                mask_value(&key, &value),
                path.display()
            );
        }
        ProviderConfigCmd::Unset { key } => {
            config.unset_provider_config(provider_id, &key);
            let path = save_config(&config)?;
            println!("{}.{} removed ({})", provider_id, key, path.display());
        }
    }
    Ok(())
}

fn handle_workspace(mut config: AppConfig, cmd: WorkspaceCmd) -> Result<()> {
    use usage_monitor_core::provider::opencode_go;

    let current = config.provider_workspaces(WORKSPACE_PROVIDER).to_vec();

    match cmd {
        WorkspaceCmd::Add { workspace, name } => {
            let ids = opencode_go::add_workspace(&current, &workspace, name.as_deref())
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            config.set_provider_workspaces(WORKSPACE_PROVIDER, ids.clone());
            let path = save_config(&config)?;
            println!("workspaces = [{}] ({})", ids.join(", "), path.display());
        }
        WorkspaceCmd::Remove { workspace } => {
            let ids = opencode_go::remove_workspace(&current, &workspace)
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            config.set_provider_workspaces(WORKSPACE_PROVIDER, ids.clone());
            let path = save_config(&config)?;
            if ids.is_empty() {
                println!("workspaces = [] — auto-discovery ({})", path.display());
            } else {
                println!("workspaces = [{}] ({})", ids.join(", "), path.display());
            }
        }
        WorkspaceCmd::List => {
            if current.is_empty() {
                println!("(no workspaces configured — auto-discovery will be used)");
            } else {
                for entry in &current {
                    match opencode_go::parse_workspace_entry(entry) {
                        Some(ws) => match &ws.name {
                            Some(name) => println!("{:<30} {}", ws.id, name),
                            None => println!("{}", ws.id),
                        },
                        None => println!("{}", entry),
                    }
                }
            }
        }
    }
    Ok(())
}

/// Builds the fetch context for a provider: values from the config file's
/// `[providers.<id>.config]` section, overridden by CLI flags.
fn provider_context(
    config: &AppConfig,
    provider: &str,
    api_key: Option<&str>,
    credentials_path: Option<&str>,
) -> ProviderContext {
    let mut ctx = ProviderContext::new();
    if let Some(settings) = config.providers.get(provider) {
        for (k, v) in &settings.config {
            ctx.config.insert(k.clone(), v.clone());
        }
        if let Some(token) = &settings.token {
            ctx.config.insert("token".into(), token.clone());
        }
        // Workspace array bridges into the provider as a comma-separated value.
        if !settings.workspaces.is_empty() {
            ctx.config
                .insert("workspaces".into(), settings.workspaces.join(","));
        }
    }
    if let Some(key) = api_key {
        ctx.config.insert("api_key".into(), key.to_string());
    }
    if let Some(path) = credentials_path {
        ctx.config
            .insert("credentials_path".into(), path.to_string());
    }
    ctx
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
    let width = snapshot_text_width(snap);
    print_block_header(&snap.provider_id, width);
    println!(
        "Collected at: {}",
        snap.collected_at.format("%Y-%m-%d %H:%M:%S UTC")
    );

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
            (Some(used), Some(total)) => {
                println!(
                    "Credits: {:.2}/{:.2} {} used",
                    used, total, credits.currency
                )
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

fn split_opencode_workspace_label(label: &str) -> Option<(String, String)> {
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

fn block_header_lines(title: &str, width: usize) -> (String, String, String) {
    let width = width.max(title.chars().count()).max(1);
    ("_".repeat(width), title.to_string(), "─".repeat(width))
}

fn snapshot_text_width(snap: &UsageSnapshot) -> usize {
    let mut width = snap.provider_id.chars().count();
    width = width.max(
        format!(
            "Collected at: {}",
            snap.collected_at.format("%Y-%m-%d %H:%M:%S UTC")
        )
        .chars()
        .count(),
    );

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
            (Some(used), Some(total)) => {
                format!(
                    "Credits: {:.2}/{:.2} {} used",
                    used, total, credits.currency
                )
            }
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

fn reset_suffix(w: &RateWindow) -> String {
    w.resets_at
        .map(|r| format!("  resets {}", r.format("%Y-%m-%d %H:%M UTC")))
        .unwrap_or_default()
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
    print_window_with_label(w, &w.label);
}

fn print_window_with_label(w: &RateWindow, label: &str) {
    let pct = w.usage_ratio * 100.0;
    let filled = (w.usage_ratio * 20.0).round() as usize;
    let bar: String = "█".repeat(filled) + &"░".repeat(20 - filled.min(20));
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

    #[test]
    fn test_split_opencode_workspace_label() {
        assert_eq!(
            split_opencode_workspace_label("Default Rolling (5h)"),
            Some(("Default".to_string(), "Rolling (5h)".to_string()))
        );
        assert_eq!(
            split_opencode_workspace_label("teste2 Monthly"),
            Some(("teste2".to_string(), "Monthly".to_string()))
        );
        assert_eq!(split_opencode_workspace_label("Seven day sonnet"), None);
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
