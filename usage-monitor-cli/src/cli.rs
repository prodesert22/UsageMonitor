use clap::{Args, Parser, Subcommand};
use usage_monitor_core::config::DEFAULT_ACCOUNT;

#[derive(Parser)]
#[command(
    name = "usage-monitor-cli",
    about = "AI API usage monitor for your terminal",
    version
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Subcommand)]
pub(crate) enum Command {
    List,
    Enable {
        provider: String,
    },
    Disable {
        provider: String,
    },
    Auto {
        provider: String,
    },
    #[command(name = "opencode-go", subcommand)]
    OpencodeGo(OpencodeGoCmd),
    #[command(name = "claude", subcommand)]
    Claude(ProviderCmd),
    #[command(name = "codex", subcommand)]
    Codex(ProviderCmd),
    #[command(name = "anthropic", subcommand)]
    Anthropic(ProviderCmd),
    #[command(name = "openai", subcommand)]
    OpenAI(ProviderCmd),
    Fetch {
        provider: Option<String>,
        #[arg(long)]
        account: Option<String>,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        api_key: Option<String>,
        #[arg(long)]
        credentials_path: Option<String>,
    },
    #[command(subcommand)]
    Widget(WidgetCmd),
    #[command(external_subcommand)]
    Provider(Vec<String>),
}

#[derive(Subcommand)]
pub(crate) enum WidgetCmd {
    Waybar(WidgetTargetArgs),
    Kde(KdeWidgetArgs),
}

#[derive(Args, Clone)]
pub(crate) struct WidgetTargetArgs {
    pub(crate) provider: Option<String>,
    #[arg(long)]
    pub(crate) account: Option<String>,
}

#[derive(Args, Clone)]
pub(crate) struct KdeWidgetArgs {
    #[command(flatten)]
    pub(crate) target: WidgetTargetArgs,
    #[arg(long)]
    pub(crate) pretty: bool,
}

#[derive(Subcommand)]
pub(crate) enum ProviderCmd {
    Show,
    Set {
        key: String,
        value: String,
    },
    Unset {
        key: String,
    },
    #[command(subcommand)]
    Account(AccountCmd),
}

#[derive(Subcommand)]
pub(crate) enum AccountCmd {
    List,
    Add {
        name: String,
        #[arg(long)]
        label: Option<String>,
    },
    Remove {
        name: String,
    },
    Set {
        name: String,
        key: String,
        value: String,
    },
    Unset {
        name: String,
        key: String,
    },
    Enable {
        name: String,
    },
    Disable {
        name: String,
    },
    Auto {
        name: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum OpencodeGoCmd {
    Show,
    Set {
        key: String,
        value: String,
    },
    Unset {
        key: String,
    },
    #[command(subcommand)]
    Account(AccountCmd),
    #[command(subcommand)]
    Workspace(WorkspaceCmd),
}

#[derive(Subcommand)]
pub(crate) enum WorkspaceCmd {
    Add {
        workspace: String,
        name: Option<String>,
        #[arg(long, default_value = DEFAULT_ACCOUNT)]
        account: String,
    },
    Remove {
        workspace: String,
        #[arg(long, default_value = DEFAULT_ACCOUNT)]
        account: String,
    },
    List {
        #[arg(long, default_value = DEFAULT_ACCOUNT)]
        account: String,
    },
}

pub(crate) const WORKSPACE_PROVIDER: &str = "opencode-go";
