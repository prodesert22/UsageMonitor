use anyhow::Result;
use usage_monitor_core::ProviderState;
use usage_monitor_core::config::AppConfig;
use usage_monitor_core::provider::registry::{AccountTarget, ProviderRegistry};

use crate::output::{print_result, target_title};

pub(crate) async fn run_fetch(
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

pub(crate) fn provider_context(
    config: &AppConfig,
    target: &AccountTarget,
    api_key: Option<&str>,
    credentials_path: Option<&str>,
) -> usage_monitor_core::provider::ProviderContext {
    let mut ctx = usage_monitor_core::provider::ProviderContext::new();
    if let Some(account) = config.account(&target.provider_id, &target.account_id) {
        for (k, v) in &account.config {
            ctx.config.insert(k.clone(), v.clone());
        }
        if let Some(token) = &account.token {
            ctx.config.insert("token".into(), token.clone());
        }
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
