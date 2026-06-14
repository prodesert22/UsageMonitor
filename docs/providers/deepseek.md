# DeepSeek provider

Tracks the DeepSeek API account balance using the same primary data source as
CodexBar.

## Auth

Use any of:

```bash
export DEEPSEEK_API_KEY=sk-...
# or
export DEEPSEEK_KEY=sk-...
# or persist it per account
usage-monitor-cli deepseek set api_key sk-...
```

`token` is accepted as an alias for `api_key` for account configs.

## Data source

- `GET https://api.deepseek.com/user/balance`
- Headers:
  - `Authorization: Bearer <api key>`
  - `Accept: application/json`

The response contains `is_available` and `balance_infos` entries with
`currency`, `total_balance`, `granted_balance`, and `topped_up_balance`.

## Behavior

- USD is preferred when it has a positive balance.
- If USD is present but empty and another currency has funds, the funded
  currency is shown instead.
- `Credits.balance` is the selected `total_balance`.
- `Credits.bonus` stores DeepSeek granted balance.
- `Credits.purchased` stores DeepSeek topped-up / paid balance.
- A zero or unavailable balance marks the synthetic Balance window as exhausted.

## Multiple accounts

```bash
usage-monitor-cli deepseek account add work --label "Work DeepSeek"
usage-monitor-cli deepseek account set work api_key sk-work-...
usage-monitor-cli fetch deepseek --account work
```

## Notes

DeepSeek does not expose a subscription-style session/weekly quota window via
this API. UsageMonitor currently ports the CodexBar balance endpoint; optional
per-month platform usage/cost endpoints can be added later if needed.
