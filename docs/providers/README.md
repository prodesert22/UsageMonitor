# Provider docs

Per-provider setup, configuration keys, extraction details, and
troubleshooting. For the general CLI and the multi-account model, see the
[main README](../../README.md).

| Provider | What it monitors | Auth | Auto-enable |
|----------|------------------|------|-------------|
| [`claude`](claude.md) | Claude Pro/Max **subscription** | Claude Code OAuth (`~/.claude/.credentials.json`) | When the credentials file exists |
| [`codex`](codex.md) | Codex on a ChatGPT **plan** | Codex CLI OAuth (`~/.codex/auth.json`) | When the auth file exists |
| [`anthropic`](anthropic.md) | Anthropic **API** (metered) | API key (Admin key for reports) | When `ANTHROPIC_API_KEY` is set |
| [`openai`](openai.md) | OpenAI **API** (metered) | API key (Admin key for reports) | When `OPENAI_API_KEY` is set |
| [`opencode-go`](opencode-go.md) | OpenCode Go workspaces | Manual session cookie | Never (manual setup) |

## Subscription vs. API — pick the right one

Two pairs cover the same vendor through different doors:

- **`claude`** tracks your **claude.ai subscription** quota windows.
  **`anthropic`** tracks **API** spend/usage with an API key.
- **`codex`** tracks **ChatGPT-plan** Codex quota windows.
  **`openai`** tracks **API** spend/usage with an API key.

## Multiple accounts

Every provider supports named accounts, so the same service can be monitored
for several logins or keys. The full command reference lives in the
[main README](../../README.md#multiple-accounts); each provider page shows a
concrete example.

How hard it is depends on the auth type:

- **API-key / cookie providers** (`anthropic`, `openai`, `opencode-go`) —
  trivial. Keys and cookies don't rotate on use, so adding an account is just
  one more `account set api_key` / `account set token`. For `opencode-go`,
  remember workspaces are per account: pass `--account <name>` when adding one.
- **OAuth providers** (`claude`, `codex`) — need care. Their tokens rotate and
  are session-bound, so you **cannot copy a credentials file** between accounts.
  Each account needs its own live login in its own config directory
  (`CODEX_HOME` for codex, a separate `HOME`/`~/.claude` for claude). See
  [codex → Why copying a token fails](codex.md#why-copying-a-token-fails).
