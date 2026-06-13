# Codex provider

Monitors **OpenAI Codex on a ChatGPT plan** (Plus/Pro/Team/…), not the OpenAI
API. It reuses the OAuth credentials written by the
[Codex CLI](https://chatgpt.com/codex) to `~/.codex/auth.json`.

> **Status: zero-config when the Codex CLI is installed.** The provider
> auto-enables as soon as `~/.codex/auth.json` (or `$CODEX_HOME/auth.json`)
> exists. No API key to paste.

## Setup

1. Install and log in with the Codex CLI (`codex login`). It stores OAuth
   tokens at `~/.codex/auth.json`.
2. Fetch:

```bash
usage-monitor-cli fetch codex
```

`$CODEX_HOME` is honored: if set, credentials are read from
`$CODEX_HOME/auth.json`. To point at a specific file:

```bash
usage-monitor-cli codex set credentials_path /path/to/auth.json
usage-monitor-cli codex show
```

## Multiple accounts

> **Do not copy `auth.json` between accounts.** It will not work — see
> [Why copying a token fails](#why-copying-a-token-fails) below. Each ChatGPT
> account needs its **own live login in its own `CODEX_HOME`**.

Give each account a dedicated `CODEX_HOME` directory, log in to each
separately, then point a named usage-monitor account at each `auth.json`:

```bash
# one isolated login per account (separate dirs → separate sessions)
CODEX_HOME=~/.codex-go   codex login    # log in to the "Go" account
CODEX_HOME=~/.codex-plus codex login    # log in to the "Plus" account

# point usage-monitor accounts at the dedicated auth files
usage-monitor-cli codex account add go   --label "Go"
usage-monitor-cli codex account set go   credentials_path ~/.codex-go/auth.json
usage-monitor-cli codex account add plus --label "Plus"
usage-monitor-cli codex account set plus credentials_path ~/.codex-plus/auth.json

usage-monitor-cli fetch codex                # one block per account
usage-monitor-cli fetch codex --account plus # just one
usage-monitor-cli codex account remove go    # drop an account
```

Rules:

- **Never** log two accounts into the **same** `CODEX_HOME` — the second login
  ends the first account's session (its refresh token is invalidated).
- After logging in, **don't** run the `codex` CLI against that `CODEX_HOME`
  again. The CLI and usage-monitor would both try to refresh the same token and
  rotate it out from under each other. Leave each dedicated `auth.json` for
  usage-monitor only.
- One account is enough? You don't need any of this — the auto-detected default
  just works (see below).

See the [main README](../../README.md#multiple-accounts) for the full account
command reference.

The auto-detected default (`~/.codex/auth.json` or `$CODEX_HOME/auth.json`)
keeps being fetched **alongside** any named accounts you add. `codex show`
lists it as `[default] (auto-detected)`. To stop fetching it while keeping the
named accounts, run `usage-monitor-cli codex account disable default`.

### Why copying a token fails

OpenAI's OAuth refresh tokens are **single-use and session-bound**: every
refresh rotates the token (the old one dies), and logging in again *ends* the
previous session. So a copied `auth.json` carries a refresh token that is
invalidated as soon as anything else refreshes it or you log in elsewhere — the
fetch then fails with:

```
token refresh failed (HTTP 401 Unauthorized):
{ "code": "refresh_token_invalidated", "message": "Your session has ended. Please log in again." }
```

The fix is never to share a token: each account must own a live, exclusive
session, which is exactly what a separate `CODEX_HOME` per login gives you.

> API-key providers ([`anthropic`](anthropic.md), [`openai`](openai.md)) and
> [`opencode-go`](opencode-go.md) have none of this trouble — a key/cookie
> doesn't rotate, so adding another account is just another `account set`.

## Configuration keys

| Key | Required | Meaning |
|-----|----------|---------|
| `credentials_path` | no | Path to the Codex CLI `auth.json`. Defaults to `$CODEX_HOME/auth.json` or `~/.codex/auth.json` |
| `access_token` | no | Use a raw bearer token directly instead of a file (no auto-refresh). Mostly for testing |
| `account_id` | no | ChatGPT account id sent as `chatgpt-account-id` when using a raw `access_token` |
| `enabled` | no | Per-account toggle; enabled by default |

## How extraction works

### Credentials

`~/.codex/auth.json` (written by the Codex CLI) holds a `tokens` section:

```json
{
  "auth_mode": "chatgpt",
  "tokens": {
    "id_token": "...",
    "access_token": "...",
    "refresh_token": "...",
    "account_id": "acc-123"
  },
  "last_refresh": "2026-06-12T10:00:00.000Z"
}
```

The `OPENAI_API_KEY` field that may also live here is a metered-API key and is
**not** used by this provider — only the OAuth `tokens` are.

### Token refresh

`auth.json` has no expiry field, so the provider just tries the request. On a
`401`/`403` it posts to `https://auth.openai.com/oauth/token` with
`grant_type=refresh_token` and the public Codex client id, retries once, and
persists the rotated tokens back to the file (preserving unknown fields and
updating `last_refresh`). Without a refresh token it errors and asks you to run
`codex login`.

### Usage endpoint

```
GET https://chatgpt.com/backend-api/wham/usage
Authorization: Bearer <access token>
chatgpt-account-id: <account id, when known>
```

The response (`rate_limit`, `additional_rate_limits`, `credits`, `plan_type`)
exposes:

| Field | Window |
|-------|--------|
| `rate_limit.primary_window` | Rolling session (labelled by size, e.g. "Session (5h)") |
| `rate_limit.secondary_window` | Weekly |
| `additional_rate_limits[]` | Any extra windows, labelled by their `label`/`name` |

Semantics:

- `used_percent` is 0–100, clamped, stored as a 0–1 ratio.
- `limit_window_seconds` sets the window size; ≤ 6 h → "Session (Nh)", ≥ 6 d →
  "Weekly".
- `reset_at` is epoch **seconds**.
- `credits` becomes a credits snapshot only when `has_credits` is true; the
  `balance` arrives as a string and is parsed to a number.

## Snapshot mapping

| Snapshot field | Source |
|----------------|--------|
| Primary window | `rate_limit.primary_window` |
| Secondary window | `rate_limit.secondary_window` |
| Extra windows | `additional_rate_limits[]` |
| Credits | `credits` (when `has_credits`) |
| Plan | `plan_type` → ChatGPT Plus / Pro / Team / Business / Enterprise / Free |

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| `cannot read credentials at ...` | No Codex CLI login | Run `codex login`, or set `credentials_path` |
| `no tokens section in auth.json` | File is API-key-only, not an OAuth login | Run `codex login` |
| `OAuth token rejected; run codex login` | Token rejected and refresh failed/absent | Re-run `codex login` |
| Rate limited (HTTP 429) | Usage endpoint throttled | Retry after the reported delay |

## Limitations

- Scrapes an internal ChatGPT backend endpoint; its shape can change without
  notice.
- Reports ChatGPT-plan Codex usage only. For the metered OpenAI **API**, use
  the [`openai`](openai.md) provider.
