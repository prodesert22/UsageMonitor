use anyhow::Result;
use std::io::Read;
use usage_monitor_cli::ProviderState;
use usage_monitor_cli::config::{AppConfig, DEFAULT_ACCOUNT};
use usage_monitor_cli::provider::gemini_oauth::GeminiOAuth;
use usage_monitor_cli::provider::registry::ProviderRegistry;

use crate::cli::{AccountCmd, GeminiCmd, ProviderCmd};

pub(crate) fn state_label(state: ProviderState) -> &'static str {
    match state {
        ProviderState::Enabled => "enabled",
        ProviderState::Disabled => "disabled",
        ProviderState::AutoEnabled => "enabled (auto)",
        ProviderState::AutoDisabled => "disabled (auto)",
    }
}

pub(crate) fn mask_value(key: &str, value: &str) -> String {
    let secret = [
        "cookie",
        "api_key",
        "token",
        "access_token",
        "client_secret",
    ]
    .contains(&key);
    if secret {
        format!("(redacted, {} chars)", value.chars().count())
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::mask_value;

    #[test]
    fn masks_secret_values_without_a_prefix() {
        let value = "client-secret-value";
        assert_eq!(mask_value("client_secret", value), "(redacted, 19 chars)");
        assert_eq!(mask_value("api_key", "short"), "(redacted, 5 chars)");
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

/// Router for `usage-monitor-cli gemini ...`: config commands reuse the generic
/// provider plumbing; login/status/logout drive the built-in OAuth flow.
pub(crate) async fn handle_gemini_cmd(
    registry: &ProviderRegistry,
    config: AppConfig,
    cmd: GeminiCmd,
) -> Result<()> {
    match cmd {
        GeminiCmd::Show => handle_provider_cmd(registry, config, "gemini", ProviderCmd::Show),
        GeminiCmd::Set { key, value } => {
            handle_provider_cmd(registry, config, "gemini", ProviderCmd::Set { key, value })
        }
        GeminiCmd::Unset { key } => {
            handle_provider_cmd(registry, config, "gemini", ProviderCmd::Unset { key })
        }
        GeminiCmd::Account(acmd) => {
            handle_provider_cmd(registry, config, "gemini", ProviderCmd::Account(acmd))
        }
        GeminiCmd::Login => gemini_login(&config).await,
        GeminiCmd::Status => gemini_status(&config),
        GeminiCmd::Logout => gemini_logout(&config),
    }
}

/// Resolves the gemini credentials path, honoring a configured
/// `credentials_path` on the default account.
fn gemini_creds_path(config: &AppConfig) -> std::path::PathBuf {
    let configured = config
        .account_config("gemini", DEFAULT_ACCOUNT)
        .and_then(|m| m.get("credentials_path").cloned());
    GeminiOAuth::resolve_creds_path(configured.as_deref())
}

async fn gemini_login(config: &AppConfig) -> Result<()> {
    let oauth = GeminiOAuth::prod().with_creds_path(gemini_creds_path(config));
    oauth
        .login(true, None)
        .await
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(())
}

#[derive(serde::Deserialize)]
struct CredsView {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expiry_date: Option<f64>,
}

fn gemini_status(config: &AppConfig) -> Result<()> {
    let path = gemini_creds_path(config);
    if !path.exists() {
        println!("gemini: no credentials at {}", path.display());
        println!("  run `usage-monitor-cli gemini login` to sign in with Google");
        return Ok(());
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("cannot read {}: {}", path.display(), e))?;
    let creds: CredsView = serde_json::from_str(&raw)
        .map_err(|e| anyhow::anyhow!("cannot parse {}: {}", path.display(), e))?;
    println!("gemini credentials ({})", path.display());
    match creds.access_token.filter(|t| !t.is_empty()) {
        Some(token) => println!("  access_token = {}", mask_value("access_token", &token)),
        None => println!("  access_token = (none)"),
    }
    let has_refresh = creds.refresh_token.is_some_and(|t| !t.is_empty());
    println!(
        "  refresh_token = {}",
        if has_refresh { "present" } else { "missing" }
    );
    match creds.expiry_date {
        Some(ms) => match chrono::DateTime::from_timestamp_millis(ms as i64) {
            Some(expires) => {
                let state = if expires > chrono::Utc::now() {
                    "valid"
                } else {
                    "expired"
                };
                println!(
                    "  expires = {} ({state})",
                    expires
                        .with_timezone(&chrono::Utc)
                        .format("%Y-%m-%d %H:%M:%S UTC")
                );
            }
            None => println!("  expires = (invalid)"),
        },
        None => println!("  expires = (unknown)"),
    }
    Ok(())
}

fn gemini_logout(config: &AppConfig) -> Result<()> {
    let path = gemini_creds_path(config);
    if path.exists() {
        std::fs::remove_file(&path)
            .map_err(|e| anyhow::anyhow!("cannot remove {}: {}", path.display(), e))?;
        println!("removed {}", path.display());
    } else {
        println!("no credentials to remove at {}", path.display());
    }
    Ok(())
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
        AccountCmd::SetStdin { name, key } => {
            let mut value = String::new();
            std::io::stdin()
                .read_to_string(&mut value)
                .map_err(|e| anyhow::anyhow!("cannot read account value from stdin: {}", e))?;
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
}
