# Anthropic provider

Monitors the **metered Anthropic API** (pay-as-you-go), using an API key. This
is distinct from the [`claude`](claude.md) provider, which tracks a Claude
Pro/Max subscription via Claude Code OAuth.

> **Status: needs an API key.** Auto-enables when `ANTHROPIC_API_KEY` is set in
> the environment; otherwise configure a key explicitly. Cost/usage **reports**
> additionally require an **Admin API key** (`sk-ant-admin...`).

## Setup

Use the environment variable (auto-detected):

```bash
export ANTHROPIC_API_KEY=sk-ant-...
usage-monitor-cli fetch anthropic
```

Or persist a key in the config (no env var needed):

```bash
usage-monitor-cli anthropic set api_key sk-ant-...
usage-monitor-cli anthropic show
```

Or pass it once, ad hoc:

```bash
usage-monitor-cli fetch anthropic --api-key sk-ant-...
```

## Multiple accounts / keys

> **Easy — no token juggling.** Unlike the OAuth providers
> ([`claude`](claude.md), [`codex`](codex.md)), an API key doesn't rotate or
> expire on use. Adding another account is just another `account set api_key`;
> nothing to copy, nothing to keep alive.

Track several organizations or keys with named accounts:

```bash
usage-monitor-cli anthropic account add team-a --label "Team A"
usage-monitor-cli anthropic account set team-a api_key sk-ant-admin-aaa...
usage-monitor-cli anthropic account set team-b api_key sk-ant-admin-bbb...
usage-monitor-cli fetch anthropic              # one block per account
usage-monitor-cli fetch anthropic --account team-a
usage-monitor-cli anthropic account remove team-b  # drop an account
```

See the [main README](../../README.md#multiple-accounts) for the full account
command reference.

## Configuration keys

| Key | Required | Meaning |
|-----|----------|---------|
| `api_key` | yes (unless `ANTHROPIC_API_KEY` is set) | Anthropic API key. An **Admin** key is needed for cost/usage reports |
| `enabled` | no | Per-account toggle; enabled by default |

Key resolution order: account `api_key` → `--api-key` flag → `ANTHROPIC_API_KEY`
env var.

## How extraction works

Three calls, all with `x-api-key` and `anthropic-version: 2023-06-01`:

### Usage report (Admin key)

```
GET https://api.anthropic.com/v1/organizations/usage_report/messages
    ?start_date=<7d ago>&end_date=<today>&bucket_width=1d
```

Returns per-day input/output token counts. A `401` is reported as an invalid
key.

### Cost report (Admin key)

```
GET https://api.anthropic.com/v1/organizations/cost_report
    ?start_date=<7d ago>&end_date=<today>
```

Returns per-day cost values. A non-admin key cannot read this, so any
non-success response is treated as an **empty** cost list rather than an error
(usage still works without it).

### Rate-limit probe

```
POST https://api.anthropic.com/v1/messages   (max_tokens: 1)
```

A minimal request whose **response headers** carry the current request rate
limit (`x-ratelimit-limit-requests` / `x-ratelimit-remaining-requests`), mapped
to an "RPM" window.

## Snapshot mapping

| Snapshot field | Source |
|----------------|--------|
| Primary window | RPM, from rate-limit headers on the probe |
| Cost (daily) | `cost_report` per-day values (Admin key only) |
| Token usage | `usage_report` input/output tokens per day (Admin key only) |

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| `no API key in config or ANTHROPIC_API_KEY env var` | No key resolved | `anthropic set api_key ...`, export the env var, or pass `--api-key` |
| `invalid API key` | Key rejected (HTTP 401) | Check the key value |
| No cost/usage rows, only RPM | Key is not an Admin key | Use an `sk-ant-admin...` key for reports |

## Limitations

- Cost and usage reports require an Admin API key; standard keys see only the
  rate-limit window.
- The rate-limit probe spends one minimal request per fetch.
