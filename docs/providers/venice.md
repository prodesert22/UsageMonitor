# Venice provider

Tracks Venice API DIEM/USD balance using CodexBar's billing balance endpoint.

## Auth

```bash
export VENICE_API_KEY=ven-...
# or
export VENICE_KEY=ven-...

usage-monitor-cli venice set api_key ven-...
```

`token` is accepted as an alias for `api_key`.

## Data source

`GET https://api.venice.ai/api/v1/billing/balance`

Headers:

- `Authorization: Bearer <api key>`
- `Accept: application/json`

## Config keys

| Key | Description |
|-----|-------------|
| `api_key` / `token` | Venice API key |
| `balance_url` / `api_url` / `base_url` | Override balance endpoint |

## Output mapping

- Primary window shows active balance detail.
- USD consumption shows `$X.XX USD remaining`.
- DIEM with epoch allocation shows `DIEM remaining / allocation` and usage ratio.
- `canConsume=false` or empty balances mark the balance window exhausted.
- `Credits.balance` stores USD balance when present, otherwise DIEM balance.

## Multiple accounts

```bash
usage-monitor-cli venice account add work --label "Work Venice"
usage-monitor-cli venice account set work api_key ven-...
usage-monitor-cli fetch venice --account work
```
