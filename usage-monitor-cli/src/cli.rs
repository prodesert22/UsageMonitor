use clap::{Args, Parser, Subcommand, ValueEnum};

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
    #[command(name = "gemini", subcommand)]
    Gemini(GeminiCmd),
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
    Gnome(WidgetTargetArgs),
    Install(WidgetInstallArgs),
    Uninstall(WidgetInstallArgs),
    /// Reinstall any already-installed widget whose version is older than this
    /// binary. Wired into login autostart so upgrades apply automatically.
    /// With a target, only that widget is synced.
    Sync(WidgetSyncArgs),
    /// Show installed vs binary versions without changing anything.
    CheckUpdate(WidgetCheckUpdateArgs),
    /// Print the release notes for a version (the releases/vX.Y.Z.md file in
    /// the repo first, GitHub Releases API next, embedded CHANGELOG.md offline).
    Changelog(WidgetChangelogArgs),
    Doctor,
}

#[derive(Args, Clone)]
pub(crate) struct WidgetInstallArgs {
    #[arg(value_enum)]
    pub(crate) target: WidgetInstallTarget,
    #[arg(long)]
    pub(crate) force: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum WidgetInstallTarget {
    Kde,
    Waybar,
    Gnome,
    All,
}

#[derive(Args, Clone)]
pub(crate) struct WidgetSyncArgs {
    #[arg(value_enum)]
    pub(crate) target: Option<WidgetInstallTarget>,
}

#[derive(Args, Clone)]
pub(crate) struct WidgetCheckUpdateArgs {
    #[arg(value_enum)]
    pub(crate) target: Option<WidgetInstallTarget>,
    #[arg(long)]
    pub(crate) pretty: bool,
}

#[derive(Args, Clone)]
pub(crate) struct WidgetChangelogArgs {
    pub(crate) version: String,
    #[arg(long)]
    pub(crate) pretty: bool,
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
pub(crate) enum GeminiCmd {
    /// Log in to Google and store Gemini credentials (browser OAuth)
    Login,
    /// Show Gemini credential status
    Status,
    /// Remove stored Gemini credentials
    Logout,
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
}
