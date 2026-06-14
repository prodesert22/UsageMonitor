use anyhow::Result;
use usage_monitor_core::provider::registry::ProviderRegistry;

use crate::fetch::provider_context;

mod model;
mod payload;

pub(crate) use model::{WidgetProvider, WidgetSummary};

pub(crate) async fn run_widget(
    registry: &ProviderRegistry,
    config: &usage_monitor_core::config::AppConfig,
    args: crate::cli::WidgetTargetArgs,
    pretty: bool,
) -> Result<()> {
    let payload = widget_payload(
        registry,
        config,
        args.provider.as_deref(),
        args.account.as_deref(),
    )
    .await?;
    if pretty {
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!("{}", serde_json::to_string(&payload)?);
    }
    Ok(())
}

async fn widget_payload(
    registry: &ProviderRegistry,
    config: &usage_monitor_core::config::AppConfig,
    provider: Option<&str>,
    account: Option<&str>,
) -> Result<WidgetSummary> {
    let mut targets = payload::widget_targets(registry, config, provider)?;
    if let Some(acct) = account {
        targets.retain(|t| t.account_id == acct);
        if targets.is_empty() {
            anyhow::bail!("no account '{}' configured for the given provider", acct);
        }
    }
    if targets.is_empty() {
        return Ok(WidgetSummary::empty(
            "No enabled providers. Use `usage-monitor-cli enable <provider>` or set up credentials.",
        ));
    }
    let results = registry
        .fetch_targets(targets, |target| {
            provider_context(config, target, None, None)
        })
        .await;
    let providers = results
        .into_iter()
        .map(|(target, result)| match result {
            Ok(snapshot) => WidgetProvider::from_snapshot(&snapshot),
            Err(e) => WidgetProvider::from_error(&target, e.to_string()),
        })
        .collect::<Vec<_>>();
    Ok(WidgetSummary::from_providers(providers))
}

#[cfg(test)]
mod tests;
