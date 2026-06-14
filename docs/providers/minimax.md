# MiniMax provider

Tracks MiniMax coding/token-plan quota via the API-token `remains` endpoints
CodexBar reads.

## Auth

```bash
export MINIMAX_API_KEY="..."          # or MINIMAX_CODING_API_KEY
# or persist per account
usage-monitor-cli minimax set api_key "..."
```

`token` aliases `api_key`.

## Data source

- `GET https://api.minimax.io/v1/token_plan/remains` (then
  `…/v1/api/openplatform/coding_plan/remains` as fallback)
- Header: `Authorization: Bearer <key>`

Reads `data.model_remains[]` with `current_interval_remaining_percent` and
`current_weekly_remaining_percent`. Override the host with `base_url` for the
China-mainland endpoint (`https://api.minimaxi.com`).

## Behavior

- Primary = interval window, secondary = weekly window;
  `used = 100 − remaining_percent` (the most-consumed model per window).
- A `base_resp` login error surfaces as an auth failure.

## Multiple accounts

```bash
usage-monitor-cli minimax account add work --label "Work"
usage-monitor-cli minimax account set work api_key "..."
usage-monitor-cli fetch minimax --account work
```
