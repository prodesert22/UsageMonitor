use anyhow::Result;
use usage_monitor_core::ProviderState;
use usage_monitor_core::config::{AppConfig, DEFAULT_ACCOUNT};
use usage_monitor_core::provider::opencode_go;
use usage_monitor_core::provider::registry::ProviderRegistry;

use crate::cli::{AccountCmd, ProviderCmd, WORKSPACE_PROVIDER, WorkspaceCmd};

pub(crate) fn state_label(state: ProviderState) -> &'static str {
    match state {
        ProviderState::Enabled => "enabled",
        ProviderState::Disabled => "disabled",
        ProviderState::AutoEnabled => "enabled (auto)",
        ProviderState::AutoDisabled => "disabled (auto)",
    }
}

pub(crate) fn mask_value(key: &str, value: &str) -> String {
    let secret = ["cookie", "api_key", "token", "access_token"].contains(&key);
    if secret && value.chars().count() > 12 {
        let prefix: String = value.chars().take(8).collect();
        format!("{}… ({} chars)", prefix, value.chars().count())
    } else {
        value.to_string()
    }
}

pub(crate) fn save_config(config: &AppConfig) -> Result<std::path::PathBuf> {
    let path = AppConfig::default_path()
        .ok_or_else(|| anyhow::anyhow!("cannot resolve config path (HOME not set)"))?;
    config
        .save_to_path(&path)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(path)
}

pub(crate) fn set_enabled(
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

pub(crate) fn handle_provider_cmd(
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
            print_config_value(provider_id, DEFAULT_ACCOUNT, &key, &value, &config)
        }
        ProviderCmd::Unset { key } => {
            config.unset_account_config(provider_id, DEFAULT_ACCOUNT, &key);
            print_config_removed(provider_id, DEFAULT_ACCOUNT, &key, &config)
        }
        ProviderCmd::Account(acmd) => handle_account_cmd(config, provider_id, acmd),
    }
}

pub(crate) fn handle_account_cmd(
    mut config: AppConfig,
    provider_id: &str,
    cmd: AccountCmd,
) -> Result<()> {
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
            print_config_value(provider_id, &name, &key, &value, &config)
        }
        AccountCmd::Unset { name, key } => {
            config.unset_account_config(provider_id, &name, &key);
            print_config_removed(provider_id, &name, &key, &config)
        }
        AccountCmd::Enable { name } => set_account_toggle(config, provider_id, &name, Some(true)),
        AccountCmd::Disable { name } => set_account_toggle(config, provider_id, &name, Some(false)),
        AccountCmd::Auto { name } => set_account_toggle(config, provider_id, &name, None),
    }
}

fn print_config_value(
    provider_id: &str,
    account: &str,
    key: &str,
    value: &str,
    config: &AppConfig,
) -> Result<()> {
    let path = save_config(config)?;
    println!(
        "{}.{}.{} = {} ({})",
        provider_id,
        account,
        key,
        mask_value(key, value),
        path.display()
    );
    Ok(())
}

fn print_config_removed(
    provider_id: &str,
    account: &str,
    key: &str,
    config: &AppConfig,
) -> Result<()> {
    let path = save_config(config)?;
    println!(
        "{}.{}.{} removed ({})",
        provider_id,
        account,
        key,
        path.display()
    );
    Ok(())
}

pub(crate) fn handle_workspace(mut config: AppConfig, cmd: WorkspaceCmd) -> Result<()> {
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
