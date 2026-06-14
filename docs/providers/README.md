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
| [`opencode-go`](opencode-go.md) | OpenCode Go workspaces | Manual session cookie | Never (manual setup) |

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
[main README](../../README.md#multiple-accounts); each provider page shows a
concrete example.

How hard it is depends on the auth type:

- **API-key / cookie providers** (`anthropic`, `openai`, `opencode-go`, and most
  others) — trivial. Keys and cookies don't rotate on use, so adding an account
  is just one more `account set api_key` / `account set token`. For
  `opencode-go`, remember workspaces are per account: pass `--account <name>`
  when adding one.
- **OAuth providers** (`claude`, `codex`) — need care. Their tokens rotate and
  are session-bound, so you **cannot copy a credentials file** between accounts.
  Each account needs its own live login in its own config directory
  (`CODEX_HOME` for codex, a separate `HOME`/`~/.claude` for claude). See
  [codex → Why copying a token fails](codex.md#why-copying-a-token-fails).
