# Groq provider

Tracks GroqCloud request/token/cache-hit rates from Groq's Prometheus metrics
API, following CodexBar's provider behavior.

## Auth

Use an environment variable or persisted account config:

```bash
export GROQ_API_KEY=gsk_...
usage-monitor-cli groq set api_key gsk_...
```

`GROQ_TOKEN` and account `token` are accepted as aliases.

## Data source

UsageMonitor queries:

`GET https://api.groq.com/v1/metrics/prometheus/api/v1/query`

with these Prometheus queries:

- `sum(model_project_id_status_code:requests:rate5m)`
- `sum(model_project_id:tokens_in:rate5m)`
- `sum(model_project_id:tokens_out:rate5m)`
- `sum(model_project_id:prompt_cache_hits:rate5m)`

The provider sends:

- `Authorization: Bearer <api key>`
- `Accept: application/json`

## Config keys

| Key | Description |
|-----|-------------|
| `api_key` / `token` | Groq API key |
| `api_url` / `base_url` | Override API base URL, defaults to `https://api.groq.com/v1` |

Environment equivalents:

- `GROQ_API_KEY`
- `GROQ_TOKEN`
- `GROQ_API_URL`

## Output mapping

- Primary window label shows request rate as requests/minute.
- Secondary window label shows combined input+output token rate as tokens/minute.
- Tertiary window is shown when prompt cache hit rate is positive.
- These are live 5-minute Prometheus rates, not quota limits; the bars remain at
  0% and the values are embedded in the labels.

## Multiple accounts

```bash
usage-monitor-cli groq account add work --label "Work Groq"
usage-monitor-cli groq account set work api_key gsk_...
usage-monitor-cli fetch groq --account work
```
