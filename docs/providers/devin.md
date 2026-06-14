# Devin provider

Tracks Devin daily/weekly quota using the same quota endpoint CodexBar reads.

## Auth

A Devin Bearer token **and** an organization slug:

```bash
export DEVIN_TOKEN="ey..."
export DEVIN_ORG="my-org"
# or persist per account
usage-monitor-cli devin set token "ey..."
usage-monitor-cli devin set organization my-org
```

`api_key` aliases `token` (env: `DEVIN_TOKEN` or `DEVIN_API_TOKEN`);
`org`/`organization_id` alias `organization`. A pasted `Bearer <token>` /
`Authorization: Bearer <token>` is normalized.

## Data source

- `GET https://app.devin.ai/api/<org>/billing/quota/usage`
- Header: `Authorization: Bearer <token>`

Reads `daily_percentage`/`daily_reset_at` and `weekly_percentage`/`weekly_reset_at`
(fractions ≤ 1 are scaled to percent).

## Behavior

- Primary = daily window (24h), secondary = weekly window (7d).
- Plan name from `plan_name`/`plan` when present.

## Multiple accounts

```bash
usage-monitor-cli devin account add work --label "Work"
usage-monitor-cli devin account set work token "ey..."
usage-monitor-cli devin account set work organization work-org
usage-monitor-cli fetch devin --account work
```

## Notes

The organization is required — Devin scopes quota under the org path. Browser
session import is not ported on Linux; paste the Bearer token.
