# Claude provider

Monitors a **Claude Pro/Max subscription** (claude.ai), not the pay-as-you-go
API. It reuses the OAuth credentials that the
[Claude Code CLI](https://claude.ai/code) writes to disk, so no extra login is
needed once you have used `claude` on the machine.

> **Status: zero-config when Claude Code is installed.** The provider
> auto-enables as soon as `~/.claude/.credentials.json` exists. There is no API
> key to paste.

## Setup

1. Install and log in with the Claude Code CLI (`claude`). It stores OAuth
   tokens at `~/.claude/.credentials.json`.
2. That's it — the provider is auto-detected:

```bash
usage-monitor-cli fetch claude
```

If the credentials live somewhere else, point the provider at them:

```bash
usage-monitor-cli claude set credentials_path /path/to/.credentials.json
usage-monitor-cli claude show
```

## Multiple accounts

> **Do not copy `.credentials.json` between accounts.** Like Codex, Claude Code
> uses OAuth tokens that refresh and rotate; a copied credentials file carries a
> token that gets invalidated as soon as something else refreshes it or you log
> in elsewhere. Each Claude account needs its **own live login in its own config
> directory**.

Log each account in under a separate home/config directory so each keeps its
own live session, then point a named usage-monitor account at each credentials
file:

```bash
# one isolated login per account (separate HOME → separate ~/.claude)
HOME=~/claude-personal claude   # log in to the personal account
HOME=~/claude-work     claude   # log in to the work account

usage-monitor-cli claude account add personal --label "Personal"
usage-monitor-cli claude account set personal credentials_path ~/claude-personal/.claude/.credentials.json
usage-monitor-cli claude account add work --label "Work"
usage-monitor-cli claude account set work credentials_path ~/claude-work/.claude/.credentials.json

usage-monitor-cli claude show
usage-monitor-cli fetch claude                 # one block per account
usage-monitor-cli fetch claude --account work  # just one
usage-monitor-cli claude account remove work   # drop an account
```

```toml
[providers.claude.accounts.personal]
label = "Personal"
credentials_path = "~/claude-personal/.claude/.credentials.json"

[providers.claude.accounts.work]
credentials_path = "~/claude-work/.claude/.credentials.json"
```

Rules:

- Each account points at its **own** credentials file. Two accounts sharing one
  file fight over the rotating token and corrupt each other.
- After logging in, leave that credentials file for usage-monitor — don't keep
  running the live `claude` CLI against it, or both will rotate the token and
  invalidate each other.
- One account is enough? You don't need any of this — the auto-detected default
  just works (see below).

See [Codex → Why copying a token fails](codex.md#why-copying-a-token-fails) for
the full explanation of token rotation; the same applies here.

See the [main README](../../README.md#multiple-accounts) for the full account
command reference.

The auto-detected default (`~/.claude/.credentials.json`) keeps being fetched
**alongside** any named accounts you add — adding `work` does not hide it.
`claude show` lists it as `[default] (auto-detected)`. To stop fetching it while
keeping the named accounts, run `usage-monitor-cli claude account disable
default`.

## Configuration keys

| Key | Required | Meaning |
|-----|----------|---------|
| `credentials_path` | no | Path to the Claude Code credentials JSON. Defaults to `~/.claude/.credentials.json` |
| `access_token` | no | Use a raw OAuth bearer token directly instead of a credentials file (no auto-refresh). Mostly for testing |
| `subscription_type` | no | Plan label hint (`pro`, `max`, …) when using a raw `access_token` |
| `enabled` | no | Per-account toggle; enabled by default |

## How extraction works

### Credentials

`~/.claude/.credentials.json` (written by Claude Code) holds a
`claudeAiOauth` section:

```json
{
  "claudeAiOauth": {
    "accessToken": "...",
    "refreshToken": "...",
    "expiresAt": 1781358459000,
    "subscriptionType": "max"
  }
}
```

- `accessToken` is the bearer token for the usage endpoint.
- `expiresAt` is epoch **milliseconds**. When expired, the provider refreshes
  the token automatically (see below) and rewrites the file in place,
  preserving any fields it does not understand.

### Token refresh

When the access token is expired, the provider posts to
`https://platform.claude.com/v1/oauth/token` with `grant_type=refresh_token`
and the public Claude Code client id, then persists the rotated tokens back to
the credentials file. If there is no refresh token, it errors and asks you to
run `claude` again.

### Usage endpoint

```
GET https://api.anthropic.com/api/oauth/usage
Authorization: Bearer <access token>
anthropic-beta: oauth-2025-04-20
User-Agent: claude-code/<version>
```

The response carries utilization percentages and reset timestamps for several
windows:

| Field | Window |
|-------|--------|
| `five_hour` | Rolling session (5 h / 300 min) |
| `seven_day` | Weekly, all models (7 d / 10080 min) |
| `seven_day_opus` | Weekly, Opus only (extra window) |
| `seven_day_sonnet` | Weekly, Sonnet only (extra window) |
| `extra_usage` | Pay-as-you-go credit add-on, when enabled |

Semantics:

- `utilization` is 0–100 and is clamped to that range before being stored as a
  0–1 ratio.
- `resets_at` is an RFC-3339 timestamp.
- `extra_usage` maps to the snapshot's credits (monthly limit, used credits,
  remaining balance) only when `is_enabled` is true.

## Snapshot mapping

| Snapshot field | Source |
|----------------|--------|
| Primary window | `five_hour` → "Session (5h)" |
| Secondary window | `seven_day` → "Weekly (all models)" |
| Extra windows | `seven_day_opus` → "Weekly (Opus)", `seven_day_sonnet` → "Weekly (Sonnet)" |
| Credits | `extra_usage` (when enabled) |
| Plan | `subscriptionType` → Claude Pro / Max / Team / Enterprise |

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| `cannot read credentials at ...` | No Claude Code login on this machine | Run `claude`, or set `credentials_path` |
| `OAuth token rejected; run claude to re-authenticate` | Token rejected and refresh failed/absent | Re-run `claude` to refresh the login |
| `access token expired and no refresh token available` | Credentials file lacks a refresh token | Re-run `claude` |
| Rate limited (HTTP 429) | Usage endpoint throttled | Retry after the reported delay |

## Limitations

- Reads the same OAuth client used by Claude Code; a future change to that flow
  could require updates.
- Only subscription usage is reported. For the metered Anthropic **API**, use
  the [`anthropic`](anthropic.md) provider instead.
