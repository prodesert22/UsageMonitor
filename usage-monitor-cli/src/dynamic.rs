use anyhow::Result;
use usage_monitor_core::config::AppConfig;
use usage_monitor_core::provider::registry::ProviderRegistry;

use crate::cli::{AccountCmd, ProviderCmd};
use crate::commands::handle_provider_cmd;

pub(crate) fn handle_dynamic_provider_cmd(
    registry: &ProviderRegistry,
    config: AppConfig,
    args: Vec<String>,
) -> Result<()> {
    let (provider_id, rest) = args
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("missing provider command"))?;
    if maybe_print_dynamic_help(provider_id, rest) {
        return Ok(());
    }
    let cmd = parse_provider_cmd(provider_id, rest)?;
    handle_provider_cmd(registry, config, provider_id, cmd)
}

fn arg_is_help(s: &str) -> bool {
    matches!(s, "help" | "-h" | "--help")
}

fn maybe_print_dynamic_help(provider_id: &str, rest: &[String]) -> bool {
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
            let Some((sub, sub_tail)) = tail.split_first() else {
                print_account_help(provider_id);
                return true;
            };
            if arg_is_help(sub) {
                print_account_help(provider_id);
                return true;
            }
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
