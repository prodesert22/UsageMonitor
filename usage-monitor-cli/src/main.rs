mod cli;
mod commands;
mod dynamic;
mod fetch;
mod output;
mod widget;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command, OpencodeGoCmd, WidgetCmd};
use usage_monitor_core::config::AppConfig;
use usage_monitor_core::provider::registry::ProviderRegistry;

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
                    commands::state_label(state),
                    meta.name,
                    meta.description
                );
            }
        }
        Command::Enable { provider } => {
            commands::set_enabled(&registry, config, &provider, Some(true))?
        }
        Command::Disable { provider } => {
            commands::set_enabled(&registry, config, &provider, Some(false))?
        }
        Command::Auto { provider } => commands::set_enabled(&registry, config, &provider, None)?,
        Command::OpencodeGo(cmd) => match cmd {
            OpencodeGoCmd::Show => commands::handle_provider_cmd(
                &registry,
                config,
                "opencode-go",
                cli::ProviderCmd::Show,
            )?,
            OpencodeGoCmd::Set { key, value } => commands::handle_provider_cmd(
                &registry,
                config,
                "opencode-go",
                cli::ProviderCmd::Set { key, value },
            )?,
            OpencodeGoCmd::Unset { key } => commands::handle_provider_cmd(
                &registry,
                config,
                "opencode-go",
                cli::ProviderCmd::Unset { key },
            )?,
            OpencodeGoCmd::Account(acmd) => commands::handle_provider_cmd(
                &registry,
                config,
                "opencode-go",
                cli::ProviderCmd::Account(acmd),
            )?,
            OpencodeGoCmd::Workspace(cmd) => commands::handle_workspace(config, cmd)?,
        },
        Command::Claude(cmd) => commands::handle_provider_cmd(&registry, config, "claude", cmd)?,
        Command::Codex(cmd) => commands::handle_provider_cmd(&registry, config, "codex", cmd)?,
        Command::Anthropic(cmd) => {
            commands::handle_provider_cmd(&registry, config, "anthropic", cmd)?
        }
        Command::OpenAI(cmd) => commands::handle_provider_cmd(&registry, config, "openai", cmd)?,
        Command::Fetch {
            provider,
            account,
            json,
            api_key,
            credentials_path,
        } => {
            fetch::run_fetch(
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
        Command::Widget(cmd) => match cmd {
            WidgetCmd::Waybar(args) => widget::run_widget(&registry, &config, args, false).await?,
            WidgetCmd::Kde(args) => {
                widget::run_widget(&registry, &config, args.target, args.pretty).await?
            }
        },
        Command::Provider(args) => dynamic::handle_dynamic_provider_cmd(&registry, config, args)?,
    }
    Ok(())
}
