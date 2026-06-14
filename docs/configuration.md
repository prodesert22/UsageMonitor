# Configuration

All state persists in a single TOML file:

```
$XDG_CONFIG_HOME/usage-monitor/config.toml
# or, when XDG_CONFIG_HOME is unset:
~/.config/usage-monitor/config.toml
```

You rarely edit it by hand — the `<provider> set`, `<provider> account`, and
`enable`/`disable`/`auto` commands manage it for you. A missing file is treated as
empty (everything auto-detected).

## File layout

```toml
[providers.<id>]
enabled = true                 # explicit toggle (omit for auto-detection)

[providers.<id>.accounts.default]
api_key = "sk-..."             # arbitrary key/value config for the provider
token = "..."

[providers.<id>.accounts.<name>]
label = "Work"                 # optional display label
enabled = true                 # per-account toggle
# ...provider-specific keys...
```

- A provider can have many **accounts**; `default` is the implicit one.
- Account values are free-form key/value pairs the provider reads from its
  `ProviderContext` (`api_key`, `token`, `cookie`, `credentials_path`,
  `base_url`, `organization`, …). See each [provider page](providers/README.md)
  for the keys it accepts.
- Secret-looking values (`api_key`, `token`, `cookie`, `access_token`) are masked
  in `show` output.

## Enable / disable resolution

A provider's effective state is resolved per `list`/`fetch`:

| Config toggle | Credentials / accounts present | Resolved state |
|---------------|-------------------------------|----------------|
| `enabled = true` | — | **Enabled** |
| `enabled = false` | — | **Disabled** |
| (none) | yes | **AutoEnabled** |
| (none) | no | **AutoDisabled** |

"Credentials present" means the provider's `detect_credentials()` returned true
(an env var is set, or a credentials file exists) **or** at least one account is
configured. Explicit toggles always win over auto-detection.

Commands:

```bash
usage-monitor-cli enable <provider>     # force on
usage-monitor-cli disable <provider>    # force off (fetch refuses it)
usage-monitor-cli auto <provider>       # remove the toggle, back to detection
usage-monitor-cli list                  # see every provider's resolved state
```

## Accounts

Every provider supports named accounts so one service can track several logins or
keys:

```bash
usage-monitor-cli <provider> account add work --label "Work"
usage-monitor-cli <provider> account set work api_key sk-work-...
usage-monitor-cli <provider> account list
usage-monitor-cli <provider> account disable work
usage-monitor-cli <provider> account remove work
usage-monitor-cli fetch <provider> --account work
```

The auto-detected `default` account is fetched **alongside** named accounts
unless you disable it (`<provider> account disable default`) or configure an
explicit `default` account that takes over the slot.

> **OAuth providers** (`claude`, `codex`) rotate session-bound tokens, so you
> cannot copy a credentials file between accounts — each needs its own live login
> in its own config directory. See [codex](providers/codex.md) for details.

## Environment variables

Two kinds:

- **Credential detection / fallback** — most providers auto-detect from an env var
  (e.g. `DEEPSEEK_API_KEY`, `GROK_TOKEN`, `OLLAMA_COOKIE`, or a credentials file
  like `~/.gemini/oauth_creds.json`). The exact variables are listed on each
  provider page and in the [provider index](providers/README.md).
- **App behavior**:
  - `XDG_CONFIG_HOME` — overrides the config directory.
  - `NO_COLOR` — disables colored output (also auto-disabled when stdout is not a
    terminal).

A `--api-key` CLI flag and a `--credentials-path` flag override the stored config
for a single fetch.

## Output

- Default: human-readable text with colored usage bars (green < 70%, yellow ≥ 70%,
  red ≥ 90%).
- `fetch --json`: machine-readable `UsageSnapshot` JSON, one object per account.
