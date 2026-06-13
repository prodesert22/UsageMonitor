# OpenAI provider

Monitors the **metered OpenAI API** (platform.openai.com), using an API key.
This is distinct from the [`codex`](codex.md) provider, which tracks
ChatGPT-plan Codex usage via OAuth.

> **Status: needs an API key.** Auto-enables when `OPENAI_API_KEY` is set in the
> environment; otherwise configure a key explicitly. Organization cost/usage
> endpoints additionally require an **Admin key** (`sk-admin...`).

## Setup

Use the environment variable (auto-detected):

```bash
export OPENAI_API_KEY=sk-...
usage-monitor-cli fetch openai
```

Or persist a key in the config (no env var needed):

```bash
usage-monitor-cli openai set api_key sk-...
usage-monitor-cli openai show
```

Or pass it once, ad hoc:

```bash
usage-monitor-cli fetch openai --api-key sk-...
```

## Multiple accounts / keys

> **Easy — no token juggling.** Unlike the OAuth providers
> ([`claude`](claude.md), [`codex`](codex.md)), an API key doesn't rotate or
> expire on use. Adding another account is just another `account set api_key`;
> nothing to copy, nothing to keep alive.

Track several organizations or keys with named accounts:

```bash
usage-monitor-cli openai account add prod --label "Production"
usage-monitor-cli openai account set prod api_key sk-admin-prod...
usage-monitor-cli openai account set dev  api_key sk-admin-dev...
usage-monitor-cli fetch openai             # one block per account
usage-monitor-cli fetch openai --account prod
usage-monitor-cli openai account remove dev  # drop an account
```

See the [main README](../../README.md#multiple-accounts) for the full account
command reference.

## Configuration keys

| Key | Required | Meaning |
|-----|----------|---------|
| `api_key` | yes (unless `OPENAI_API_KEY` is set) | OpenAI API key. An **Admin** key is needed for the organization cost/usage endpoints |
| `enabled` | no | Per-account toggle; enabled by default |

Key resolution order: account `api_key` → `--api-key` flag → `OPENAI_API_KEY`
env var.

## How extraction works

All calls use `Authorization: Bearer <key>`.

### Rate-limit probe

```
GET https://api.openai.com/v1/models
```

A light request whose **response headers** carry the current rate limits
(`x-ratelimit-limit-requests` / `-remaining-requests` and the token-based
`-limit-tokens` / `-remaining-tokens`). A `401` is reported as an invalid key;
a `429` is still read for its headers.

### Cost endpoint (Admin key)

```
GET https://api.openai.com/v1/organization/costs
```

Per-day spend amounts. Requires an Admin key; failures collapse to an **empty**
cost list rather than an error.

### Usage endpoint (Admin key)

```
GET https://api.openai.com/v1/organization/usage/completions
```

Per-model input/output token counts.

## Snapshot mapping

| Snapshot field | Source |
|----------------|--------|
| Primary window | Requests-per-minute, from rate-limit headers |
| Secondary window | Tokens-per-minute, from rate-limit headers (when present) |
| Cost (daily) | `organization/costs` (Admin key only) |
| Token usage | `organization/usage/completions` (Admin key only) |

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| `no API key found in config or OPENAI_API_KEY env var` | No key resolved | `openai set api_key ...`, export the env var, or pass `--api-key` |
| `invalid API key` | Key rejected (HTTP 401) | Check the key value |
| No cost/usage rows, only rate limits | Key is not an Admin key | Use an `sk-admin...` key for the organization endpoints |

## Limitations

- Cost and usage endpoints require an Admin key; standard keys see only the
  rate-limit windows.
- The rate-limit probe spends one light request per fetch.
