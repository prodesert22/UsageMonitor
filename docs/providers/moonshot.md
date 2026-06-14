# Moonshot provider

Tracks Moonshot / Kimi API balance using CodexBar's balance endpoint.

## Auth

```bash
export MOONSHOT_API_KEY=sk-...
# or
export MOONSHOT_KEY=sk-...

usage-monitor-cli moonshot set api_key sk-...
```

`token` is accepted as an alias for `api_key`.

## Region

Default region is international:

- `international` → `https://api.moonshot.ai/v1/users/me/balance`
- `china` / `cn` → `https://api.moonshot.cn/v1/users/me/balance`

Configure with:

```bash
export MOONSHOT_REGION=china
usage-monitor-cli moonshot set region china
```

## Config keys

| Key | Description |
|-----|-------------|
| `api_key` / `token` | Moonshot API key |
| `region` | `international` or `china`/`cn` |
| `api_url` / `base_url` | Override API base URL |

## Output mapping

- `Credits.balance` = `available_balance`.
- `Credits.bonus` = `voucher_balance`.
- `Credits.purchased` = `cash_balance`.
- Plan name shows current balance.
- Negative cash balance is surfaced as a deficit feature.

## Multiple accounts

```bash
usage-monitor-cli moonshot account add cn --label "China Moonshot"
usage-monitor-cli moonshot account set cn api_key sk-...
usage-monitor-cli moonshot account set cn region china
usage-monitor-cli fetch moonshot --account cn
```
