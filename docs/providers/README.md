# Provider docs

Per-provider setup, configuration keys, extraction details, and
troubleshooting. For the general CLI and the multi-account model, see the
[main README](../../README.md).

## Native Linux fetchers

| Provider | What it monitors | Auth | Auto-enable |
|----------|------------------|------|-------------|
| [`claude`](claude.md) | Claude Pro/Max **subscription** | Claude Code OAuth (`~/.claude/.credentials.json`) | When the credentials file exists |
| [`codex`](codex.md) | Codex on a ChatGPT **plan** | Codex CLI OAuth (`~/.codex/auth.json`) | When the auth file exists |
| [`anthropic`](anthropic.md) | Anthropic **API** (metered) | API key (Admin key for reports) | When `ANTHROPIC_API_KEY` is set |
| [`openai`](openai.md) | OpenAI **API** (metered) | API key (Admin key for reports) | When `OPENAI_API_KEY` is set |
| [`deepseek`](deepseek.md) | DeepSeek **API** balance | API key | When `DEEPSEEK_API_KEY` or `DEEPSEEK_KEY` is set |
| [`deepgram`](deepgram.md) | Deepgram usage breakdown | API key, optional project ID | When `DEEPGRAM_API_KEY` is set |
| [`elevenlabs`](elevenlabs.md) | ElevenLabs subscription credits | API key | When `ELEVENLABS_API_KEY` or `XI_API_KEY` is set |
| [`groq`](groq.md) | GroqCloud Prometheus metrics | API key | When `GROQ_API_KEY` or `GROQ_TOKEN` is set |
| [`llmproxy`](llmproxy.md) | Aggregate proxy quota stats | API key + base URL | When `LLM_PROXY_API_KEY` and `LLM_PROXY_BASE_URL` are set |
| [`moonshot`](moonshot.md) | Moonshot / Kimi API balance | API key | When `MOONSHOT_API_KEY` or `MOONSHOT_KEY` is set |
| [`openrouter`](openrouter.md) | OpenRouter credits/API-key usage | API key | When `OPENROUTER_API_KEY` is set |
| [`venice`](venice.md) | Venice DIEM/USD balance | API key | When `VENICE_API_KEY` or `VENICE_KEY` is set |
| [`cursor`](cursor.md) | Cursor plan + on-demand usage | Browser session cookie/token | When `CURSOR_SESSION_TOKEN` is set |
| [`copilot`](copilot.md) | GitHub Copilot premium/chat quota | GitHub OAuth/PAT token | When `COPILOT_API_TOKEN`/`GITHUB_TOKEN`/`GH_TOKEN` is set |
| [`perplexity`](perplexity.md) | Perplexity plan/bonus/purchased credits | Browser session cookie/token | When `PERPLEXITY_SESSION_TOKEN` is set |
| [`gemini`](gemini.md) | Gemini Code Assist daily quotas | gemini-cli Google OAuth | When `~/.gemini/oauth_creds.json` exists |
| [`antigravity`](antigravity.md) | Antigravity Code Assist daily quotas | Antigravity Google OAuth | When `~/.codexbar/antigravity/oauth_creds.json` exists |
| [`abacus`](abacus.md) | Abacus AI compute points | Browser session cookie | When `ABACUS_COOKIE` is set |
| [`devin`](devin.md) | Devin daily/weekly quota | Bearer token + organization | When `DEVIN_TOKEN` is set |
| [`kimi`](kimi.md) | Kimi coding weekly/rate-limit | kimi-auth token | When `KIMI_AUTH_TOKEN` is set |
| [`kimik2`](kimik2.md) | Kimi K2 credits | API key | When `KIMI_K2_API_KEY` is set |
| [`minimax`](minimax.md) | MiniMax coding/token-plan quota | API key | When `MINIMAX_API_KEY`/`MINIMAX_CODING_API_KEY` is set |
| [`mistral`](mistral.md) | Mistral API monthly spend | Browser session cookie | When `MISTRAL_COOKIE` is set |
| [`ollama`](ollama.md) | Ollama cloud session/weekly usage | Browser session cookie | When `OLLAMA_COOKIE` is set |
| [`zai`](zai.md) | z.ai coding-plan quota | API key | When `Z_AI_API_KEY` is set |
| [`grok`](grok.md) | Grok credit usage (gRPC-Web) | Bearer token or browser cookie | When `GROK_TOKEN`/`GROK_COOKIE` is set |
| [`windsurf`](windsurf.md) | Windsurf daily/weekly quota (Connect proto) | Devin session token | When `WINDSURF_SESSION_TOKEN` is set |
| [`opencode-go`](opencode-go.md) | OpenCode Go rolling/weekly/monthly quota | API key (auto-detected from the desktop login) | When `OPENCODE_API_KEY` is set or `~/.local/share/opencode/auth.json` holds an opencode-go key |

Every provider above ships a real Linux fetcher. Auth and extraction were ported
from the [CodexBar](https://github.com/steipete/CodexBar) macOS implementation; browser-cookie
providers take the cookie/token from config instead of auto-importing it from a
browser (which is macOS-specific).

## Subscription vs. API — pick the right one

Two pairs cover the same vendor through different doors:

- **`claude`** tracks your **claude.ai subscription** quota windows.
  **`anthropic`** tracks **API** spend/usage with an API key.
- **`codex`** tracks **ChatGPT-plan** Codex quota windows.
  **`openai`** tracks **API** spend/usage with an API key.

## Multiple accounts

Every provider supports named accounts, so the same service can be monitored
for several logins or keys. The full command reference lives in the
[commands doc](../commands.md#multiple-accounts) and the
[configuration doc](../configuration.md#accounts); each provider page shows a
concrete example.

### Auth types and their multi-account strategy

| Auth type | Providers | Strategy |
|-----------|-----------|----------|
| **API key** | `anthropic`, `openai`, `openrouter`, `deepseek`, `deepgram`, `elevenlabs`, `groq`, `llmproxy`, `moonshot`, `venice`, `kimik2`, `minimax`, `zai`, `opencode-go` | Keys don't rotate — just add another `account set api_key` (or `account set token` for `opencode-go`) |
| **Token / cookie** | `grok`, `kimi`, `copilot`, `windsurf`, `abacus`, `mistral`, `devin`, `cursor`, `perplexity`, `ollama` | Tokens/cookies don't rotate — just add another `account set token` or `account set cookie` |
| **OAuth (credentials file)** | `claude`, `codex`, `gemini`, `antigravity` | Tokens **rotate** — each account needs its own live login in an isolated directory |

### API-key and token/cookie providers (most providers)

Keys, tokens, and cookies are static credentials that don't change when used.
Adding a second account is a single command:

```bash
usage-monitor-cli openai account add work --label "Work API key"
usage-monitor-cli openai account set work api_key "sk-..."
usage-monitor-cli openai account add personal --label "Personal"
usage-monitor-cli openai account set personal api_key "sk-..."

# Fetch all accounts
usage-monitor-cli fetch openai

# Fetch just one
usage-monitor-cli fetch openai --account work
```

The same pattern works for any API-key, token, or cookie provider — just
replace `api_key` with `token` or `cookie` as needed.

### OAuth providers (Claude, Codex, Gemini, Antigravity)

OAuth tokens **refresh and rotate** — every refresh invalidates the previous
token, and logging in a second account invalidates the first one's session.
This means:

- **Never copy a credentials file** between accounts. The copied token gets
  invalidated as soon as anything refreshes it.
- **Never log two accounts into the same directory.** The second login ends
  the first account's session.
- Each account needs its **own live login** in its own config directory.

#### Codex — `CODEX_HOME` isolation

The Codex CLI respects the `CODEX_HOME` env var for its config directory.
Use separate directories, log in once per account, then point a named
usage-monitor account at each `auth.json`:

```bash
# Log in to each account in its own directory
CODEX_HOME=~/.codex-personal codex login     # Personal ChatGPT account
CODEX_HOME=~/.codex-work      codex login     # Work ChatGPT account

# Point usage-monitor accounts at each auth file
usage-monitor-cli codex account add personal --label "Personal"
usage-monitor-cli codex account set personal credentials_path ~/.codex-personal/auth.json
usage-monitor-cli codex account add work --label "Work"
usage-monitor-cli codex account set work credentials_path ~/.codex-work/auth.json

# Fetch all
usage-monitor-cli fetch codex
```

After logging in, leave each `auth.json` for usage-monitor alone — the
Codex CLI and usage-monitor both rotate tokens, so running `codex` against
the same `CODEX_HOME` would invalidate usage-monitor's cached token.

#### Claude — `HOME` isolation

Claude Code writes credentials to `~/.claude/.credentials.json` using the
real `$HOME`. There is no `CLAUDE_HOME` env var, so isolation requires
overriding `HOME` for each login:

```bash
# Log in once per account with a fake HOME
HOME=~/claude-personal claude        # log in to the Personal account
HOME=~/claude-work     claude        # log in to the Work account

# Point usage-monitor accounts at each credentials file
usage-monitor-cli claude account add personal --label "Personal"
usage-monitor-cli claude account set personal credentials_path ~/claude-personal/.claude/.credentials.json
usage-monitor-cli claude account add work --label "Work"
usage-monitor-cli claude account set work credentials_path ~/claude-work/.claude/.credentials.json

usage-monitor-cli fetch claude        # one block per account
```

The same isolation rules apply — after logging in, don't run `claude`
against a directory that usage-monitor is using, or the two will fight
over token rotation.

#### Gemini — `credentials_path`

Gemini has no dedicated HOME-style env var. Use `credentials_path` to
point each account at its own OAuth file:

```bash
usage-monitor-cli gemini account add work --label "Work Gemini"
usage-monitor-cli gemini account set work credentials_path /path/to/work/oauth_creds.json
usage-monitor-cli gemini account set work access_token "$(gcloud auth print-access-token)"
usage-monitor-cli fetch gemini
```

#### Antigravity — `credentials_path`

Same pattern as Gemini — no HOME-style env var, use `credentials_path`:

```bash
usage-monitor-cli antigravity account add work --label "Work"
usage-monitor-cli antigravity account set work credentials_path /path/to/work/oauth_creds.json
usage-monitor-cli fetch antigravity --account work
```

### Per-provider config keys

The `account set <name> <key> <value>` commands accept these provider-specific
keys. See each provider page for details.

| Key | Used by | Meaning |
|-----|---------|---------|
| `api_key` | All API-key providers | API key or secret |
| `token` | Token/cookie providers | Bearer token or session JWT |
| `cookie` | Some token providers (alias for `token`) | Browser cookie value |
| `credentials_path` | `claude`, `codex`, `gemini`, `antigravity` | Path to the OAuth credentials file |
| `access_token` | `claude`, `codex`, `gemini`, `antigravity` | Raw OAuth bearer token (no auto-refresh) |
| `base_url` | `llmproxy`, others with custom endpoints | API base URL override |
| `organization_id` | `kilo` | Team/organization scope |
| `project_id` | `gemini` | GCP project id |
| `org` | `devin` | Organization name/ID |

### The auto-detected default

When a provider has **no** configured accounts, a single implicit `default`
account is used, relying on credential auto-detection (env vars or files).
Adding a named account does **not** remove the auto-detected default — both
are fetched side by side:

```bash
usage-monitor-cli codex account add go --label "Go"
# Now fetches: [default] (auto-detected $CODEX_HOME) + [go]
```

To stop fetching the auto-detected default while keeping named accounts:

```bash
usage-monitor-cli codex account disable default
```

An explicitly configured `default` account also takes over the slot (no
auto-detection). Use bare `<provider> set`/`unset` commands to configure it.
