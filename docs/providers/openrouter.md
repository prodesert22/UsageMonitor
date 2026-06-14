# OpenRouter provider

Tracks OpenRouter account credits and, when available, API-key quota/spend data.

## Auth

Use either an environment variable or a persisted account key:

```bash
export OPENROUTER_API_KEY=sk-or-v1-...
usage-monitor-cli openrouter set api_key sk-or-v1-...
```

`token` is accepted as an alias for `api_key` in account configs.

## Data sources

UsageMonitor follows CodexBar's OpenRouter API strategy:

1. `GET https://openrouter.ai/api/v1/credits`
   - Returns `total_credits` and `total_usage`.
   - Balance is calculated as `total_credits - total_usage`.
2. `GET https://openrouter.ai/api/v1/key`
   - Optional enrichment endpoint.
   - Provides API-key `limit`, current `usage`, `usage_daily`,
     `usage_weekly`, `usage_monthly`, and `rate_limit` when OpenRouter returns
     them.
   - If this endpoint fails, the provider still returns credits data.

## Config keys

| Key | Description |
|-----|-------------|
| `api_key` / `token` | OpenRouter API key |
| `api_url` / `base_url` | Override API base URL, defaults to `https://openrouter.ai/api/v1` |
| `http_referer` | Optional `HTTP-Referer` header |
| `x_title` | Optional `X-Title` header, defaults to `UsageMonitor` |

Environment equivalents:

- `OPENROUTER_API_KEY`
- `OPENROUTER_API_URL`
- `OPENROUTER_HTTP_REFERER`
- `OPENROUTER_X_TITLE`

## Output mapping

- `Credits.balance` = remaining OpenRouter credits.
- `Credits.total` = `total_credits`.
- `Credits.used` = `total_usage`.
- The primary rate window is API-key spend limit usage when `/key` returns both
  `limit` and `usage`.
- `Cost.total_cost` uses `usage_monthly` when present, otherwise current key
  `usage`.
- `Cost.spend_limit` stores API-key `limit`/`usage` when available.

## Multiple accounts

```bash
usage-monitor-cli openrouter account add work --label "Work OpenRouter"
usage-monitor-cli openrouter account set work api_key sk-or-v1-...
usage-monitor-cli fetch openrouter --account work
```
