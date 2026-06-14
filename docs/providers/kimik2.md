# Kimi K2 provider

Tracks Kimi K2 credits via the kimi-k2.ai credits endpoint (API-key auth).

## Auth

```bash
export KIMI_K2_API_KEY="..."   # KIMIK2_API_KEY also accepted
# or persist per account
usage-monitor-cli kimik2 set api_key "..."
```

`token` aliases `api_key`.

## Data source

- `GET https://kimi-k2.ai/api/user/credits`
- Header: `Authorization: Bearer <key>`

The response is parsed leniently across common shapes (`credits_remaining`,
`total_credits_consumed`, and `data`/`result`/`usage`/`credits` wrappers).

## Behavior

- `Credits.balance` = remaining credits; `used`/`total` mirror consumption.

## Multiple accounts

```bash
usage-monitor-cli kimik2 account add work --label "Work"
usage-monitor-cli kimik2 account set work api_key "..."
usage-monitor-cli fetch kimik2 --account work
```
