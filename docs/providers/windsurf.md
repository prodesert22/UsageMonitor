# Windsurf provider

Tracks Windsurf daily/weekly quota through the same Connect RPC CodexBar reads
(`exa.seat_management_pb.SeatManagementService/GetPlanStatus`).

## Auth

Windsurf uses a Devin-style browser session. The session token is required; the
other Devin headers are optional but may be needed by the server:

```bash
export WINDSURF_SESSION_TOKEN="..."
# optional extra session headers
export WINDSURF_AUTH1_TOKEN="..."
export WINDSURF_ACCOUNT_ID="..."
export WINDSURF_PRIMARY_ORG_ID="..."
# or persist per account
usage-monitor-cli windsurf set session_token "..."
usage-monitor-cli windsurf set auth1_token "..."
usage-monitor-cli windsurf set account_id "..."
usage-monitor-cli windsurf set primary_org_id "..."
```

`token`/`api_key` alias `session_token`. Grab the values from a logged-in
browser at `windsurf.com` (the `devin_session_token`, `devin_auth1_token`,
`devin_account_id`, `devin_primary_org_id` storage entries).

## Data source

- `POST https://windsurf.com/_backend/exa.seat_management_pb.SeatManagementService/GetPlanStatus`
- Connect unary (`Content-Type: application/proto`, `Connect-Protocol-Version: 1`);
  the request and response are raw protobuf.

Response field numbers (from CodexBar's reverse-engineering of Windsurf's bundled
protobuf): `PlanStatus { 1: plan_info{2: plan_name}, 14: daily_remaining_percent,
15: weekly_remaining_percent, 17: daily_reset_unix, 18: weekly_reset_unix }`.

## Behavior

- Primary window = daily usage (`100 − daily_remaining_percent`, 24h), secondary
  = weekly usage (7d), each with its reset time.
- Plan name comes from the nested `plan_info`.

## Multiple accounts

```bash
usage-monitor-cli windsurf account add work --label "Work"
usage-monitor-cli windsurf account set work session_token "..."
usage-monitor-cli fetch windsurf --account work
```

## Notes

Browser session values are not auto-imported on Linux — supply them from config.
They expire with the browser session.
