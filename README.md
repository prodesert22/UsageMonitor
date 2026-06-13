# Usage Monitor

AI API usage monitor for your terminal.

Collects, stores, and displays consumption metrics from AI services like
Anthropic Claude, OpenAI, DeepSeek, Groq, and many more — all without
depending on external servers.

A Linux port of [CodexBar](https://github.com/steipete/CodexBar) by
[Peter Steinberger](https://github.com/steipete), reimplemented in Rust.

## Providers

| ID          | Service                       | Auth                                               |
|-------------|-------------------------------|----------------------------------------------------|
| `claude`    | Claude Pro/Max (subscription) | Claude Code OAuth (`~/.claude/.credentials.json`)  |
| `codex`     | Codex / ChatGPT plan          | Codex CLI OAuth (`~/.codex/auth.json`)             |
| `anthropic` | Anthropic API                 | `ANTHROPIC_API_KEY` or `--api-key`                 |
| `openai`    | OpenAI API                    | `OPENAI_API_KEY` or `--api-key`                    |
| `opencode-go` | OpenCode Go workspaces      | Manual session cookie ([docs](docs/providers/opencode-go.md)) |

## Commands

| Command | Description |
|---------|-------------|
| `list` | List providers with their resolved state (`enabled`, `disabled`, or `(auto)` from credential detection) |
| `fetch [provider]` | Fetch usage. Without a provider, fetches all enabled providers; with one, fetches it (refused if explicitly disabled) |
| `enable <provider>` | Force a provider on, regardless of detection |
| `disable <provider>` | Force a provider off; it is skipped by `fetch` and direct fetches are refused |
| `auto <provider>` | Remove the explicit toggle and return to credential auto-detection |
| `<provider> show` | Show a provider's config; secret values are masked. Supported providers: `opencode-go`, `claude`, `codex`, `anthropic`, `openai` |
| `<provider> set <key> <value>` | Set a provider config value (e.g. `token`, `api_key`, `credentials_path`) |
| `<provider> unset <key>` | Remove a provider config key |
| `opencode-go workspace add <id\|url> [name]` | Add an OpenCode Go workspace; accepts a `wrk_...` id or the dashboard URL, with an optional display name |
| `opencode-go workspace remove <id\|url>` | Remove a workspace; an empty list returns to auto-discovery |
| `opencode-go workspace list` | List configured workspaces |

All state persists in `~/.config/usage-monitor/config.toml`. Providers are
auto-enabled when their credentials are detected (credential files for
`claude`/`codex`, API key env vars for `anthropic`/`openai`); explicit
toggles always win.

### Examples

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

# OpenCode Go: manual token + workspaces (see docs/providers/opencode-go.md)
usage-monitor-cli opencode-go set token '<Cookie header, auth=Fe26 value, or bare Fe26 value>'
usage-monitor-cli enable opencode-go
usage-monitor-cli opencode-go workspace add https://opencode.ai/workspace/wrk_xxx/go

# Other provider config examples
usage-monitor-cli openai set api_key sk-...
usage-monitor-cli claude set credentials_path ~/.claude/.credentials.json
usage-monitor-cli codex set credentials_path ~/.codex/auth.json

# Machine-readable output
usage-monitor-cli fetch claude --json
```

## Structure

```
usage-monitor-core/     Core library (models, providers, fetching)
usage-monitor-cli/      Command-line interface
docs/                   Provider extraction specifications
```

## Build

```bash
cargo build
cargo test
```

## Tests

```bash
# All tests
cargo test

# Specific module
cargo test -p usage-monitor-core -- model::usage
cargo test -p usage-monitor-core -- provider::anthropic
```

## Credits

Concept, provider research, and original macOS implementation:
[steipete/CodexBar](https://github.com/steipete/CodexBar) (MIT). This project
ports the idea to Linux as a Rust library + CLI.

## License

MIT
