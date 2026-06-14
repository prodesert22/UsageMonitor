# LLM Proxy provider

Tracks aggregate usage from an LLM Proxy instance exposing CodexBar-compatible
quota stats.

## Auth

LLM Proxy requires both an API key and a proxy base URL:

```bash
export LLM_PROXY_API_KEY=proxy-key
export LLM_PROXY_BASE_URL=https://proxy.example.com
```

Or persist them in UsageMonitor config:

```bash
usage-monitor-cli llmproxy set api_key proxy-key
usage-monitor-cli llmproxy set base_url https://proxy.example.com
```

`token` is accepted as an alias for `api_key`; `enterprise_host` is accepted as
an alias for `base_url`.

## Data source

UsageMonitor requests:

`GET <base-url>/v1/quota-stats`

If `base_url` already ends in `/v1`, it requests `<base-url>/quota-stats`.

Headers:

- `Authorization: Bearer <api key>`
- `Accept: application/json`

## Output mapping

- Primary window: quota usage, calculated as `100 - minimum_remaining_percent`.
- Secondary window: total request count.
- Tertiary window: total token count.
- Extra windows: up to three provider summaries sorted by request volume.
- `Cost.total_cost`: approximate USD spend when returned by the proxy.
- Plan text summarizes provider count and active/total credentials.

## Multiple accounts

```bash
usage-monitor-cli llmproxy account add prod --label "Prod Proxy"
usage-monitor-cli llmproxy account set prod api_key proxy-key
usage-monitor-cli llmproxy account set prod base_url https://proxy.example.com
usage-monitor-cli fetch llmproxy --account prod
```
