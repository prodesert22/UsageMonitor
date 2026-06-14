# Usage Monitor

AI API usage monitor for your terminal.

Collects, stores, and displays consumption metrics from AI services like
Anthropic Claude, OpenAI, DeepSeek, Groq, and many more — all without
depending on external servers.

A Linux port of [CodexBar](https://github.com/steipete/CodexBar) by
[Peter Steinberger](https://github.com/steipete), reimplemented in Rust.

## Documentation

Full docs live in [`docs/`](docs/README.md):

- [Installation](docs/installation.md) — prerequisites, build, install, PATH.
- [Architecture](docs/architecture.md) — crates, data model, provider trait,
  registry, fetch flow.
- [Configuration](docs/configuration.md) — `config.toml`, accounts, env vars.
- [Desktop widgets](docs/widgets/README.md) — KDE Plasma 6 and Waybar integration.
- [Quality checks](docs/quality.md) — rustfmt, Clippy, QML/widget tests, size limits.
- [Adding a provider](docs/adding-a-provider.md) — porting guide + checklist.
- [Provider index](docs/providers/README.md) — every provider and its auth.
- [Credits & license](docs/credits.md).

## Build

```bash
cargo build --release
# binary: ./target/release/usage-monitor-cli
```

## Install

```bash
cargo install --path usage-monitor-cli
# installs `usage-monitor-cli` into ~/.cargo/bin (make sure it is on your PATH)
```

## Providers

Native Linux fetchers currently exist for:

| ID          | Service                       | Auth                                               |
|-------------|-------------------------------|----------------------------------------------------|
| [`claude`](docs/providers/claude.md) | Claude Pro/Max (subscription) | Claude Code OAuth (`~/.claude/.credentials.json`)  |
| [`codex`](docs/providers/codex.md) | Codex / ChatGPT plan          | Codex CLI OAuth (`~/.codex/auth.json`)             |
| [`anthropic`](docs/providers/anthropic.md) | Anthropic API                 | `ANTHROPIC_API_KEY` or `--api-key`                 |
| [`openai`](docs/providers/openai.md) | OpenAI API                    | `OPENAI_API_KEY` or `--api-key`                    |
| [`deepseek`](docs/providers/deepseek.md) | DeepSeek API balance          | `DEEPSEEK_API_KEY`, `DEEPSEEK_KEY`, or `--api-key` |
| [`deepgram`](docs/providers/deepgram.md) | Deepgram usage breakdown      | `DEEPGRAM_API_KEY`, optional `DEEPGRAM_PROJECT_ID` |
| [`elevenlabs`](docs/providers/elevenlabs.md) | ElevenLabs subscription credits | `ELEVENLABS_API_KEY`, `XI_API_KEY`, or `--api-key` |
| [`groq`](docs/providers/groq.md) | GroqCloud metrics             | `GROQ_API_KEY`, `GROQ_TOKEN`, or `--api-key` |
| [`llmproxy`](docs/providers/llmproxy.md) | LLM Proxy aggregate quota stats | `LLM_PROXY_API_KEY` + base URL |
| [`moonshot`](docs/providers/moonshot.md) | Moonshot / Kimi API balance | `MOONSHOT_API_KEY`, `MOONSHOT_KEY`, or `--api-key` |
| [`openrouter`](docs/providers/openrouter.md) | OpenRouter credits/API key usage | `OPENROUTER_API_KEY` or `--api-key` |
| [`venice`](docs/providers/venice.md) | Venice DIEM/USD balance | `VENICE_API_KEY`, `VENICE_KEY`, or `--api-key` |
| [`cursor`](docs/providers/cursor.md) | Cursor plan + on-demand usage | Browser cookie/token, `CURSOR_SESSION_TOKEN` |
| [`copilot`](docs/providers/copilot.md) | GitHub Copilot premium/chat quota | `COPILOT_API_TOKEN`, `GITHUB_TOKEN`, `GH_TOKEN` |
| [`perplexity`](docs/providers/perplexity.md) | Perplexity plan/bonus/purchased credits | Browser cookie/token, `PERPLEXITY_SESSION_TOKEN` |
| [`gemini`](docs/providers/gemini.md) | Gemini Code Assist daily quotas | gemini-cli OAuth (`~/.gemini/oauth_creds.json`) |
| [`antigravity`](docs/providers/antigravity.md) | Antigravity Code Assist daily quotas | Google OAuth (`~/.codexbar/antigravity/oauth_creds.json`) |
| [`abacus`](docs/providers/abacus.md) | Abacus AI compute points | Browser cookie, `ABACUS_COOKIE` |
| [`devin`](docs/providers/devin.md) | Devin daily/weekly quota | Bearer token + org, `DEVIN_TOKEN`/`DEVIN_ORG` |
| [`kimi`](docs/providers/kimi.md) | Kimi coding weekly/rate-limit | kimi-auth token, `KIMI_AUTH_TOKEN` |
| [`kimik2`](docs/providers/kimik2.md) | Kimi K2 credits | `KIMI_K2_API_KEY` |
| [`minimax`](docs/providers/minimax.md) | MiniMax coding/token-plan quota | `MINIMAX_API_KEY`/`MINIMAX_CODING_API_KEY` |
| [`mistral`](docs/providers/mistral.md) | Mistral API monthly spend | Browser cookie, `MISTRAL_COOKIE` |
| [`ollama`](docs/providers/ollama.md) | Ollama cloud session/weekly usage | Browser cookie, `OLLAMA_COOKIE` |
| [`zai`](docs/providers/zai.md) | z.ai coding-plan quota | `Z_AI_API_KEY` |
| [`grok`](docs/providers/grok.md) | Grok credit usage (gRPC-Web) | Bearer token or cookie, `GROK_TOKEN`/`GROK_COOKIE` |
| [`windsurf`](docs/providers/windsurf.md) | Windsurf daily/weekly quota (Connect proto) | Devin session token, `WINDSURF_SESSION_TOKEN` |
| [`opencode-go`](docs/providers/opencode-go.md) | OpenCode Go workspaces      | Manual session cookie |

Auth and extraction were ported from the CodexBar macOS app. Browser-cookie
providers (`cursor`, `perplexity`, `abacus`, `mistral`, `ollama`) take the
cookie/token from config rather than auto-importing it from a browser.

Per-provider setup, config keys, and troubleshooting live in
[docs/providers/](docs/providers/README.md).

## Commands

| Command | Description |
|---------|-------------|
| `list` | List providers with their resolved state (`enabled`, `disabled`, or `(auto)` from credential detection) |
| `fetch [provider] [--account <name>]` | Fetch usage. Without a provider, fetches all enabled providers concurrently; with one, fetches it (refused if explicitly disabled). `--account` restricts to a single account |
| `widget waybar [provider] [--account <name>]` | Emit single-line JSON for a Waybar custom module |
| `widget kde [provider] [--account <name>] [--pretty]` | Emit the JSON payload consumed by the KDE Plasma widget helper |
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

### Multiple accounts

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

The config file groups settings per provider, then per account:

```toml
[providers.claude.accounts.personal]
label = "Personal"
credentials_path = "~/.claude/.credentials.json"

[providers.claude.accounts.work]
credentials_path = "~/work/.claude/.credentials.json"
```

## Structure

```
usage-monitor-core/     Core library (models, providers, registry, config)
usage-monitor-cli/      Command-line interface
widgets/                KDE Plasma and Waybar integrations
docs/                   Guides + per-provider specifications
releases/               Per-version release notes
```

See [docs/architecture.md](docs/architecture.md) for a detailed breakdown.

## Tests

```bash
# All tests
cargo test

# Specific module
cargo test -p usage-monitor-core -- model::usage
cargo test -p usage-monitor-core -- provider::anthropic

# Desktop widget helpers
python -m unittest discover -s widgets -p 'test_*.py'

# Local quality gate (fmt, clippy, ruff, widget tests, qmllint, size checks)
python scripts/check_quality.py
```

## Credits

Concept, provider research, and original macOS implementation:
[steipete/CodexBar](https://github.com/steipete/CodexBar) (MIT). This project
ports the idea to Linux as a Rust library + CLI. See
[docs/credits.md](docs/credits.md).

## License

MIT — see [LICENSE](LICENSE).
