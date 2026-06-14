use anyhow::Result;
use chrono::Datelike;
use clap::{Parser, Subcommand};
use usage_monitor_core::config::{AppConfig, DEFAULT_ACCOUNT};
use usage_monitor_core::provider::ProviderContext;
use usage_monitor_core::provider::registry::{AccountTarget, ProviderRegistry};
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
    Claude(ProviderCmd),
    /// Codex provider commands
    #[command(name = "codex", subcommand)]
    Codex(ProviderCmd),
    /// Anthropic provider commands
    #[command(name = "anthropic", subcommand)]
    Anthropic(ProviderCmd),
    /// OpenAI provider commands
    #[command(name = "openai", subcommand)]
    OpenAI(ProviderCmd),
    /// Fetch usage for a provider, or for all enabled providers when omitted
    Fetch {
        /// Provider ID (e.g. claude, codex, anthropic, openai)
        provider: Option<String>,
        /// Restrict to a single account (by name)
        #[arg(long)]
        account: Option<String>,
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
    /// Provider-specific config commands for any registered provider
    #[command(external_subcommand)]
    Provider(Vec<String>),
}

/// Provider config commands. The bare `set`/`unset` operate on the `default`
/// account; `account` manages named accounts for multi-login providers.
#[derive(Subcommand)]
enum ProviderCmd {
    /// Show the provider's state and configured accounts (secrets masked)
    Show,
    /// Set a config value on the default account
    Set { key: String, value: String },
    /// Remove a config key from the default account
    Unset { key: String },
    /// Manage named accounts
    #[command(subcommand)]
    Account(AccountCmd),
}

#[derive(Subcommand)]
enum AccountCmd {
    /// List configured accounts
    List,
    /// Add an account
    Add {
        name: String,
        /// Display label
        #[arg(long)]
        label: Option<String>,
    },
    /// Remove an account
    Remove { name: String },
    /// Set a config value on an account
    Set {
        name: String,
        key: String,
        value: String,
    },
    /// Remove a config key from an account
    Unset { name: String, key: String },
    /// Enable an account
    Enable { name: String },
    /// Disable an account
    Disable { name: String },
    /// Return an account to auto (remove the explicit toggle)
    Auto { name: String },
}

#[derive(Subcommand)]
enum OpencodeGoCmd {
    /// Show the provider's state and configured accounts (secrets masked)
    Show,
    /// Set a config value on the default account (token, cookie, etc.)
    Set { key: String, value: String },
    /// Remove a config key from the default account
    Unset { key: String },
    /// Manage named accounts
    #[command(subcommand)]
    Account(AccountCmd),
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
        /// Account to attach the workspace to
        #[arg(long, default_value = DEFAULT_ACCOUNT)]
        account: String,
    },
    /// Remove a workspace
    Remove {
        workspace: String,
        /// Account the workspace belongs to
        #[arg(long, default_value = DEFAULT_ACCOUNT)]
        account: String,
    },
    /// List configured workspaces
    List {
        /// Account to list workspaces for
        #[arg(long, default_value = DEFAULT_ACCOUNT)]
        account: String,
    },
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
                println!(
                    "{:<12} {:<16} {} — {}",
                    meta.id,
                    state_label(state),
                    meta.name,
                    meta.description
                );
            }
        }
        Command::Enable { provider } => set_enabled(&registry, config, &provider, Some(true))?,
        Command::Disable { provider } => set_enabled(&registry, config, &provider, Some(false))?,
        Command::Auto { provider } => set_enabled(&registry, config, &provider, None)?,
        Command::OpencodeGo(cmd) => match cmd {
            OpencodeGoCmd::Show => {
                handle_provider_cmd(&registry, config, "opencode-go", ProviderCmd::Show)?
            }
            OpencodeGoCmd::Set { key, value } => handle_provider_cmd(
                &registry,
                config,
                "opencode-go",
                ProviderCmd::Set { key, value },
            )?,
            OpencodeGoCmd::Unset { key } => {
                handle_provider_cmd(&registry, config, "opencode-go", ProviderCmd::Unset { key })?
            }
            OpencodeGoCmd::Account(acmd) => {
                handle_provider_cmd(&registry, config, "opencode-go", ProviderCmd::Account(acmd))?
            }
            OpencodeGoCmd::Workspace(cmd) => handle_workspace(config, cmd)?,
        },
        Command::Claude(cmd) => handle_provider_cmd(&registry, config, "claude", cmd)?,
        Command::Codex(cmd) => handle_provider_cmd(&registry, config, "codex", cmd)?,
        Command::Anthropic(cmd) => handle_provider_cmd(&registry, config, "anthropic", cmd)?,
        Command::OpenAI(cmd) => handle_provider_cmd(&registry, config, "openai", cmd)?,
        Command::Fetch {
            provider,
            account,
            json,
            api_key,
            credentials_path,
        } => {
            run_fetch(
                &registry,
                &config,
                provider.as_deref(),
                account.as_deref(),
                json,
                api_key.as_deref(),
                credentials_path.as_deref(),
            )
            .await?
        }
        Command::Provider(args) => handle_dynamic_provider_cmd(&registry, config, args)?,
    }

    Ok(())
}

fn handle_dynamic_provider_cmd(
    registry: &ProviderRegistry,
    config: AppConfig,
    args: Vec<String>,
) -> Result<()> {
    let (provider_id, rest) = args
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("missing provider command"))?;
    // Intercept help at every level *before* parsing, so a help flag never gets
    // consumed as an account name or value (e.g. `account add -h`).
    if maybe_print_dynamic_help(provider_id, rest) {
        return Ok(());
    }
    let cmd = parse_provider_cmd(provider_id, rest)?;
    handle_provider_cmd(registry, config, provider_id, cmd)
}

fn arg_is_help(s: &str) -> bool {
    matches!(s, "help" | "-h" | "--help")
}

/// Prints contextual help for the dynamic provider command tree. Returns `true`
/// when help was printed (caller should stop), `false` to continue parsing.
fn maybe_print_dynamic_help(provider_id: &str, rest: &[String]) -> bool {
    // Bare `<provider>` → provider help.
    let Some((cmd, tail)) = rest.split_first() else {
        print_provider_help(provider_id);
        return true;
    };
    match cmd.as_str() {
        c if arg_is_help(c) => {
            print_provider_help(provider_id);
            true
        }
        "account" => {
            // `account` alone, or `account help`/`account -h` → account help.
            let Some((sub, sub_tail)) = tail.split_first() else {
                print_account_help(provider_id);
                return true;
            };
            if arg_is_help(sub) {
                print_account_help(provider_id);
                return true;
            }
            // `account <sub> ... -h` → that subcommand's usage.
            if sub_tail.iter().any(|a| arg_is_help(a)) {
                print_account_sub_help(provider_id, sub);
                return true;
            }
            false
        }
        "show" | "set" | "unset" => {
            if tail.iter().any(|a| arg_is_help(a)) {
                print_provider_help(provider_id);
                return true;
            }
            false
        }
        _ => {
            // Unknown leading command with a help flag → show provider help.
            if tail.iter().any(|a| arg_is_help(a)) {
                print_provider_help(provider_id);
                return true;
            }
            false
        }
    }
}

fn print_account_help(provider_id: &str) {
    println!("Manage named accounts for the {provider_id} provider.\n");
    println!("Usage: usage-monitor-cli {provider_id} account <command>\n");
    println!("Commands:");
    println!("  list                       List configured accounts");
    println!("  add <name> [--label <l>]   Add an account");
    println!("  remove <name>              Remove an account");
    println!("  set <name> <key> <value>   Set a config value on an account");
    println!("  unset <name> <key>         Remove a config key from an account");
    println!("  enable <name>              Enable an account");
    println!("  disable <name>             Disable an account");
    println!("  auto <name>                Return an account to auto-detection");
}

fn print_account_sub_help(provider_id: &str, sub: &str) {
    let usage = match sub {
        "list" => "account list",
        "add" => "account add <name> [--label <label>]",
        "remove" => "account remove <name>",
        "set" => "account set <name> <key> <value>",
        "unset" => "account unset <name> <key>",
        "enable" => "account enable <name>",
        "disable" => "account disable <name>",
        "auto" => "account auto <name>",
        _ => {
            print_account_help(provider_id);
            return;
        }
    };
    println!("Usage: usage-monitor-cli {provider_id} {usage}");
}

fn print_provider_help(provider_id: &str) {
    println!("Configure and inspect the {provider_id} provider.\n");
    println!("Usage: usage-monitor-cli {provider_id} <command>\n");
    println!("Commands:");
    println!("  show                       Show state and configured accounts (secrets masked)");
    println!("  set <key> <value>          Set a config value on the default account");
    println!("  unset <key>                Remove a config key from the default account");
    println!("  account <command>          Manage named accounts (see below)");
    println!("  help                       Print this help\n");
    println!("Account commands:");
    println!("  account list                       List configured accounts");
    println!("  account add <name> [--label <l>]   Add an account");
    println!("  account remove <name>              Remove an account");
    println!("  account set <name> <key> <value>   Set a config value on an account");
    println!("  account unset <name> <key>         Remove a config key from an account");
    println!("  account enable <name>              Enable an account");
    println!("  account disable <name>             Disable an account");
    println!("  account auto <name>                Return an account to auto-detection\n");
    println!("Enable/disable the provider itself with:");
    println!("  usage-monitor-cli enable {provider_id}");
    println!("  usage-monitor-cli disable {provider_id}");
    println!("  usage-monitor-cli auto {provider_id}");
}

fn parse_provider_cmd(provider_id: &str, args: &[String]) -> Result<ProviderCmd> {
    let Some((cmd, rest)) = args.split_first() else {
        anyhow::bail!(
            "missing command for provider '{}'; expected show, set, unset, or account",
            provider_id
        );
    };

    match cmd.as_str() {
        "show" => {
            ensure_no_extra(provider_id, cmd, rest)?;
            Ok(ProviderCmd::Show)
        }
        "set" => match rest {
            [key, value] => Ok(ProviderCmd::Set {
                key: key.clone(),
                value: value.clone(),
            }),
            _ => anyhow::bail!("usage: {} set <key> <value>", provider_id),
        },
        "unset" => match rest {
            [key] => Ok(ProviderCmd::Unset { key: key.clone() }),
            _ => anyhow::bail!("usage: {} unset <key>", provider_id),
        },
        "account" => Ok(ProviderCmd::Account(parse_account_cmd(provider_id, rest)?)),
        _ => anyhow::bail!(
            "unknown command '{}' for provider '{}'; expected show, set, unset, or account",
            cmd,
            provider_id
        ),
    }
}

fn parse_account_cmd(provider_id: &str, args: &[String]) -> Result<AccountCmd> {
    let Some((cmd, rest)) = args.split_first() else {
        anyhow::bail!("missing account command for provider '{}'", provider_id);
    };

    match cmd.as_str() {
        "list" => {
            ensure_no_extra(provider_id, "account list", rest)?;
            Ok(AccountCmd::List)
        }
        "add" => parse_account_add(provider_id, rest),
        "remove" => match rest {
            [name] => Ok(AccountCmd::Remove { name: name.clone() }),
            _ => anyhow::bail!("usage: {} account remove <name>", provider_id),
        },
        "set" => match rest {
            [name, key, value] => Ok(AccountCmd::Set {
                name: name.clone(),
                key: key.clone(),
                value: value.clone(),
            }),
            _ => anyhow::bail!("usage: {} account set <name> <key> <value>", provider_id),
        },
        "unset" => match rest {
            [name, key] => Ok(AccountCmd::Unset {
                name: name.clone(),
                key: key.clone(),
            }),
            _ => anyhow::bail!("usage: {} account unset <name> <key>", provider_id),
        },
        "enable" => match rest {
            [name] => Ok(AccountCmd::Enable { name: name.clone() }),
            _ => anyhow::bail!("usage: {} account enable <name>", provider_id),
        },
        "disable" => match rest {
            [name] => Ok(AccountCmd::Disable { name: name.clone() }),
            _ => anyhow::bail!("usage: {} account disable <name>", provider_id),
        },
        "auto" => match rest {
            [name] => Ok(AccountCmd::Auto { name: name.clone() }),
            _ => anyhow::bail!("usage: {} account auto <name>", provider_id),
        },
        _ => anyhow::bail!(
            "unknown account command '{}' for provider '{}'; expected list, add, remove, set, unset, enable, disable, or auto",
            cmd,
            provider_id
        ),
    }
}

fn parse_account_add(provider_id: &str, args: &[String]) -> Result<AccountCmd> {
    match args {
        [name] => Ok(AccountCmd::Add {
            name: name.clone(),
            label: None,
        }),
        [name, flag, label] if flag == "--label" => Ok(AccountCmd::Add {
            name: name.clone(),
            label: Some(label.clone()),
        }),
        _ => anyhow::bail!(
            "usage: {} account add <name> [--label <label>]",
            provider_id
        ),
    }
}

fn ensure_no_extra(provider_id: &str, command: &str, rest: &[String]) -> Result<()> {
    if rest.is_empty() {
        Ok(())
    } else {
        anyhow::bail!("usage: {} {}", provider_id, command)
    }
}

async fn run_fetch(
    registry: &ProviderRegistry,
    config: &AppConfig,
    provider: Option<&str>,
    account: Option<&str>,
    json: bool,
    api_key: Option<&str>,
    credentials_path: Option<&str>,
) -> Result<()> {
    let mut targets = match provider {
        Some(id) => {
            if registry.get(id).is_none() {
                anyhow::bail!("unknown provider '{}'", id);
            }
            if registry.provider_state(id, config) == Some(ProviderState::Disabled) {
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

    if let Some(acct) = account {
        targets.retain(|t| t.account_id == acct);
        if targets.is_empty() {
            anyhow::bail!("no account '{}' configured for the given provider", acct);
        }
    }

    if targets.is_empty() {
        println!("No enabled providers. Use `enable <provider>` or set up credentials.");
        return Ok(());
    }

    let results = registry
        .fetch_targets(targets, |target| {
            provider_context(config, target, api_key, credentials_path)
        })
        .await;

    let mut first = true;
    for (target, result) in results {
        if !first {
            println!();
        }
        first = false;
        match result {
            Ok(snapshot) => print_result(&snapshot, json)?,
            Err(e) => println!("{}: error: {}", target_title(&target), e),
        }
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

fn state_label(state: ProviderState) -> &'static str {
    match state {
        ProviderState::Enabled => "enabled",
        ProviderState::Disabled => "disabled",
        ProviderState::AutoEnabled => "enabled (auto)",
        ProviderState::AutoDisabled => "disabled (auto)",
    }
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

fn handle_provider_cmd(
    registry: &ProviderRegistry,
    mut config: AppConfig,
    provider_id: &str,
    cmd: ProviderCmd,
) -> Result<()> {
    if registry.get(provider_id).is_none() {
        anyhow::bail!("unknown provider '{}'", provider_id);
    }
    match cmd {
        ProviderCmd::Show => show_provider(registry, &config, provider_id),
        ProviderCmd::Set { key, value } => {
            config.set_account_config(provider_id, DEFAULT_ACCOUNT, &key, &value);
            let path = save_config(&config)?;
            println!(
                "{}.{}.{} = {} ({})",
                provider_id,
                DEFAULT_ACCOUNT,
                key,
                mask_value(&key, &value),
                path.display()
            );
            Ok(())
        }
        ProviderCmd::Unset { key } => {
            config.unset_account_config(provider_id, DEFAULT_ACCOUNT, &key);
            let path = save_config(&config)?;
            println!(
                "{}.{}.{} removed ({})",
                provider_id,
                DEFAULT_ACCOUNT,
                key,
                path.display()
            );
            Ok(())
        }
        ProviderCmd::Account(acmd) => handle_account_cmd(config, provider_id, acmd),
    }
}

fn handle_account_cmd(mut config: AppConfig, provider_id: &str, cmd: AccountCmd) -> Result<()> {
    match cmd {
        AccountCmd::List => {
            let ids = config.account_ids(provider_id);
            if ids.is_empty() {
                println!("(no accounts configured — auto-detection will be used)");
            } else {
                for id in ids {
                    print_account(&config, provider_id, &id);
                }
            }
            Ok(())
        }
        AccountCmd::Add { name, label } => {
            let created = config.add_account(provider_id, &name, label.as_deref());
            let path = save_config(&config)?;
            if created {
                println!("{}.{} added ({})", provider_id, name, path.display());
            } else {
                println!(
                    "{}.{} already exists ({})",
                    provider_id,
                    name,
                    path.display()
                );
            }
            Ok(())
        }
        AccountCmd::Remove { name } => {
            if !config.remove_account(provider_id, &name) {
                anyhow::bail!("no account '{}' for provider '{}'", name, provider_id);
            }
            let path = save_config(&config)?;
            println!("{}.{} removed ({})", provider_id, name, path.display());
            Ok(())
        }
        AccountCmd::Set { name, key, value } => {
            config.set_account_config(provider_id, &name, &key, &value);
            let path = save_config(&config)?;
            println!(
                "{}.{}.{} = {} ({})",
                provider_id,
                name,
                key,
                mask_value(&key, &value),
                path.display()
            );
            Ok(())
        }
        AccountCmd::Unset { name, key } => {
            config.unset_account_config(provider_id, &name, &key);
            let path = save_config(&config)?;
            println!(
                "{}.{}.{} removed ({})",
                provider_id,
                name,
                key,
                path.display()
            );
            Ok(())
        }
        AccountCmd::Enable { name } => set_account_toggle(config, provider_id, &name, Some(true)),
        AccountCmd::Disable { name } => set_account_toggle(config, provider_id, &name, Some(false)),
        AccountCmd::Auto { name } => set_account_toggle(config, provider_id, &name, None),
    }
}

fn set_account_toggle(
    mut config: AppConfig,
    provider_id: &str,
    name: &str,
    enabled: Option<bool>,
) -> Result<()> {
    match enabled {
        Some(value) => config.set_account_enabled(provider_id, name, value),
        None => config.clear_account_enabled(provider_id, name),
    }
    let path = save_config(&config)?;
    let label = match enabled {
        Some(true) => "enabled",
        Some(false) => "disabled",
        None => "auto",
    };
    println!("{}.{}: {} ({})", provider_id, name, label, path.display());
    Ok(())
}

fn show_provider(registry: &ProviderRegistry, config: &AppConfig, provider_id: &str) -> Result<()> {
    let state = registry
        .provider_state(provider_id, config)
        .expect("registered provider");
    println!("provider = {}", provider_id);
    println!("state = {}", state_label(state));

    // The implicit auto-detected default is fetched alongside named accounts
    // unless an explicit `default` account overrides it.
    let detected = registry
        .get(provider_id)
        .is_some_and(|p| p.detect_credentials());
    if detected && config.account(provider_id, DEFAULT_ACCOUNT).is_none() {
        println!("[default] (auto-detected)");
    }

    let ids = config.account_ids(provider_id);
    if ids.is_empty() {
        if !detected {
            println!("(no accounts configured — auto-detection will be used)");
        }
    } else {
        for id in ids {
            print_account(config, provider_id, &id);
        }
    }
    Ok(())
}

fn print_account(config: &AppConfig, provider_id: &str, account: &str) {
    match config.account_label(provider_id, account) {
        Some(label) => println!("[{}] {}", account, label),
        None => println!("[{}]", account),
    }
    if let Some(false) = config.account_enabled(provider_id, account) {
        println!("  disabled");
    }
    if let Some(token) = config.account_token(provider_id, account) {
        println!("  token = {}", mask_value("token", token));
    }
    if let Some(map) = config.account_config(provider_id, account) {
        let mut keys: Vec<&String> = map.keys().collect();
        keys.sort();
        for key in keys {
            println!("  {} = {}", key, mask_value(key, &map[key]));
        }
    }
    if provider_id == WORKSPACE_PROVIDER {
        for entry in config.account_workspaces(provider_id, account) {
            match usage_monitor_core::provider::opencode_go::parse_workspace_entry(entry) {
                Some(ws) => match &ws.name {
                    Some(name) => println!("  {:<28} {}", ws.id, name),
                    None => println!("  {}", ws.id),
                },
                None => println!("  {}", entry),
            }
        }
    }
}

fn handle_workspace(mut config: AppConfig, cmd: WorkspaceCmd) -> Result<()> {
    use usage_monitor_core::provider::opencode_go;

    match cmd {
        WorkspaceCmd::Add {
            workspace,
            name,
            account,
        } => {
            let current = config
                .account_workspaces(WORKSPACE_PROVIDER, &account)
                .to_vec();
            let ids = opencode_go::add_workspace(&current, &workspace, name.as_deref())
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            config.set_account_workspaces(WORKSPACE_PROVIDER, &account, ids.clone());
            let path = save_config(&config)?;
            println!("workspaces = [{}] ({})", ids.join(", "), path.display());
        }
        WorkspaceCmd::Remove { workspace, account } => {
            let current = config
                .account_workspaces(WORKSPACE_PROVIDER, &account)
                .to_vec();
            let ids = opencode_go::remove_workspace(&current, &workspace)
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            config.set_account_workspaces(WORKSPACE_PROVIDER, &account, ids.clone());
            let path = save_config(&config)?;
            if ids.is_empty() {
                println!("workspaces = [] — auto-discovery ({})", path.display());
            } else {
                println!("workspaces = [{}] ({})", ids.join(", "), path.display());
            }
        }
        WorkspaceCmd::List { account } => {
            let current = config
                .account_workspaces(WORKSPACE_PROVIDER, &account)
                .to_vec();
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

/// Builds the fetch context for an account target: values from the account's
/// config, overridden by CLI flags.
fn provider_context(
    config: &AppConfig,
    target: &AccountTarget,
    api_key: Option<&str>,
    credentials_path: Option<&str>,
) -> ProviderContext {
    let mut ctx = ProviderContext::new();
    if let Some(account) = config.account(&target.provider_id, &target.account_id) {
        for (k, v) in &account.config {
            ctx.config.insert(k.clone(), v.clone());
        }
        if let Some(token) = &account.token {
            ctx.config.insert("token".into(), token.clone());
        }
        // Workspace array bridges into the provider as a comma-separated value.
        if !account.workspaces.is_empty() {
            ctx.config
                .insert("workspaces".into(), account.workspaces.join(","));
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
    let path = save_config(&config)?;

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

/// Display title for a snapshot's header (provider plus account, when set).
fn snapshot_title(snap: &UsageSnapshot) -> String {
    match (snap.account_label.as_deref(), snap.account_id.as_deref()) {
        (Some(label), _) => format!("{} — {}", snap.provider_id, label),
        (None, Some(id)) => format!("{} ({})", snap.provider_id, id),
        (None, None) => snap.provider_id.clone(),
    }
}

/// Display title for a fetch error line.
fn target_title(target: &AccountTarget) -> String {
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

fn snapshot_text_width(snap: &UsageSnapshot, title: &str) -> usize {
    let mut width = title.chars().count();
    width = width.max(
        format!("Collected at: {}", fmt_local_datetime(snap.collected_at))
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
        .map(|r| format!("  {}", fmt_reset(r)))
        .unwrap_or_default()
}

/// Formats an instant in the system-local timezone, showing the local UTC
/// offset, e.g. `00:16 14/06/2026 (UTC-03:00)`. Text is English; the JSON output
/// keeps the raw UTC timestamp untouched.
fn fmt_local_datetime(dt: chrono::DateTime<chrono::Utc>) -> String {
    let local = dt.with_timezone(&chrono::Local);
    format!(
        "{} {} (UTC{})",
        local.format("%H:%M"),
        local.format("%d/%m/%Y"),
        local.format("%:z")
    )
}

/// Formats a reset instant relative to now, in the local timezone: today → just
/// the time; tomorrow/yesterday → a relative word; otherwise the weekday plus
/// date. Example: `resets tomorrow at 14:30`.
fn fmt_reset(dt: chrono::DateTime<chrono::Utc>) -> String {
    let local = dt.with_timezone(&chrono::Local);
    let now = chrono::Local::now();
    let days = (local.date_naive() - now.date_naive()).num_days();
    let time = local.format("%H:%M");

    // Include the year only when it differs from the current one.
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
    fn test_fmt_local_datetime_shows_local_offset() {
        let s = fmt_local_datetime(chrono::Utc::now());
        // `HH:MM dd/mm/yyyy (UTC±hh:mm)`
        assert!(s.contains("(UTC"), "got: {s}");
        assert!(s.contains('/'), "got: {s}");
        assert!(s.contains(':'), "got: {s}");
    }

    #[test]
    fn test_fmt_reset_today_is_time_only() {
        // A few seconds from now is still "today" in local time.
        let dt = chrono::Utc::now() + chrono::Duration::seconds(5);
        let s = fmt_reset(dt);
        assert!(s.starts_with("resets at "), "got: {s}");
        assert!(!s.contains("tomorrow"), "got: {s}");
    }

    #[test]
    fn test_fmt_reset_tomorrow() {
        let dt = (chrono::Local::now() + chrono::Duration::days(1)).with_timezone(&chrono::Utc);
        assert!(fmt_reset(dt).starts_with("resets tomorrow at "), "got: {}", fmt_reset(dt));
    }

    #[test]
    fn test_fmt_reset_far_shows_weekday() {
        let dt = (chrono::Local::now() + chrono::Duration::days(5)).with_timezone(&chrono::Utc);
        let s = fmt_reset(dt);
        // English weekday name, no "tomorrow"/"today".
        let weekdays = [
            "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday",
        ];
        assert!(weekdays.iter().any(|w| s.contains(w)), "got: {s}");
        assert!(s.contains(" at "), "got: {s}");
    }

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

    #[test]
    fn test_snapshot_title() {
        let mut snap = UsageSnapshot::new("claude");
        assert_eq!(snapshot_title(&snap), "claude");
        snap.account_id = Some("work".into());
        assert_eq!(snapshot_title(&snap), "claude (work)");
        snap.account_label = Some("Work Claude".into());
        assert_eq!(snapshot_title(&snap), "claude — Work Claude");
    }
}
