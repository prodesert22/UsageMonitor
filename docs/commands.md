# Commands & usage

The full CLI command reference and worked examples. For the configuration file
and the multi-account model, see [configuration.md](configuration.md); for
desktop widgets, see [widgets/README.md](widgets/README.md).

## Commands

| Command | Description |
|---------|-------------|
| `list` | List providers with their resolved state (`enabled`, `disabled`, or `(auto)` from credential detection) |
| `fetch [provider] [--account <name>]` | Fetch usage. Without a provider, fetches all enabled providers concurrently; with one, fetches it (refused if explicitly disabled). `--account` restricts to a single account |
| `widget waybar [provider] [--account <name>]` | Emit single-line JSON for a Waybar custom module |
| `widget kde [provider] [--account <name>] [--pretty]` | Emit the JSON payload consumed by the KDE Plasma widget helper |
| `widget install <kde\|waybar\|all> [--force]` | Install an embedded desktop widget (KDE via `kpackagetool6`, Waybar wrapper into `~/.local/bin`) |
| `widget uninstall <kde\|waybar\|all>` | Remove an installed widget |
| `widget sync` | Reinstall any installed widget older than the CLI (run at login by an autostart entry, so upgrades apply automatically) |
| `widget doctor` | Print resolved widget install paths, versions, and tool availability |
| `enable <provider>` | Force a provider on, regardless of detection |
| `disable <provider>` | Force a provider off; it is skipped by `fetch` and direct fetches are refused |
| `auto <provider>` | Remove the explicit toggle and return to credential auto-detection |
| `<provider> show` | Show any registered provider's state and configured accounts; secret values are masked |
| `<provider> set <key> <value>` | Set a config value on the `default` account (e.g. `token`, `api_key`, `credentials_path`) |
| `<provider> unset <key>` | Remove a config key from the `default` account |
| `opencode-go workspace add <id\|url> [name] [--account <name>]` | Add an OpenCode Go workspace; accepts a `wrk_...` id or the dashboard URL, with an optional display name |
| `opencode-go workspace remove <id\|url> [--account <name>]` | Remove a workspace; an empty list returns to auto-discovery |
| `opencode-go workspace list [--account <name>]` | List configured workspaces |

All state persists in `~/.config/usage-monitor/config.toml`. Providers are
auto-enabled when their credentials are detected (credential files for
`claude`/`codex`/`gemini`/`antigravity`, or API-key/cookie env vars for the
rest) or when accounts are configured for them; explicit toggles always win.

## Multiple accounts

Each provider can hold several named **accounts** — useful for monitoring more
than one login of the same service (e.g. a personal and a work Claude). An
account carries its own credentials plus an optional label and enable toggle.

| Command | Description |
|---------|-------------|
| `<provider> account list` | List the provider's configured accounts |
| `<provider> account add <name> [--label <label>]` | Create an account |
| `<provider> account remove <name>` | Delete an account |
| `<provider> account set <name> <key> <value>` | Set a config value on an account |
| `<provider> account unset <name> <key>` | Remove a config key from an account |
| `<provider> account enable <name>` | Enable an account |
| `<provider> account disable <name>` | Disable an account (skipped by `fetch`) |
| `<provider> account auto <name>` | Remove the explicit account toggle |

When a provider has **no** configured accounts it still works: a single
implicit `default` account is used, relying on credential auto-detection. The
bare `<provider> set`/`unset` commands operate on that `default` account.

The auto-detected `default` is **not** replaced when you add a named account —
both are fetched. So a machine logged into Claude Code that also adds a `work`
account ends up monitoring both. To drop the auto-detected default while
keeping the named accounts, disable it: `<provider> account disable default`.
(Configuring an explicit `default` account also takes over that slot.)

`fetch` emits one block per enabled account, headed by the account label (or
`provider (name)` when unlabeled).

## Examples

```bash
# Everything that is enabled, at a glance
usage-monitor-cli fetch

# Claude subscription usage (reads Claude Code CLI credentials)
usage-monitor-cli fetch claude

# One-off fetch with an explicit key
usage-monitor-cli fetch anthropic --api-key sk-ant-...

# Persist an API key instead of passing the flag every time
usage-monitor-cli anthropic set api_key sk-ant-...
usage-monitor-cli anthropic show

# OpenCode Go: manual token + workspaces (see providers/opencode-go.md)
usage-monitor-cli opencode-go set token '<Cookie header, auth=Fe26 value, or bare Fe26 value>'
usage-monitor-cli enable opencode-go
usage-monitor-cli opencode-go workspace add https://opencode.ai/workspace/wrk_xxx/go

# Other provider config examples
usage-monitor-cli openai set api_key sk-...
usage-monitor-cli claude set credentials_path ~/.claude/.credentials.json
usage-monitor-cli codex set credentials_path ~/.codex/auth.json
usage-monitor-cli openrouter set api_key sk-or-...
usage-monitor-cli deepseek set api_key sk-...

# Multiple Claude logins side by side
usage-monitor-cli claude account add personal --label "Personal"
usage-monitor-cli claude account set personal credentials_path ~/.claude/.credentials.json
usage-monitor-cli claude account set work credentials_path ~/work/.claude/.credentials.json
usage-monitor-cli claude show
usage-monitor-cli fetch claude              # one block per account
usage-monitor-cli fetch claude --account work

# Machine-readable output
usage-monitor-cli fetch claude --json

# Desktop widget payloads
usage-monitor-cli widget waybar
usage-monitor-cli widget kde --pretty
```

The config file groups settings per provider, then per account (full reference
in [configuration.md](configuration.md)):

```toml
[providers.claude.accounts.personal]
label = "Personal"
credentials_path = "~/.claude/.credentials.json"

[providers.claude.accounts.work]
credentials_path = "~/work/.claude/.credentials.json"
```
