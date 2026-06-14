# Kimi provider

Tracks Kimi coding weekly quota and rate limit using the same billing RPC
CodexBar reads.

## Auth

The `kimi-auth` token (a JWT from a logged-in kimi.com session):

```bash
export KIMI_AUTH_TOKEN="ey..."
# or persist per account
usage-monitor-cli kimi set token "ey..."
```

`api_key`/`cookie` are accepted as aliases (env: `KIMI_AUTH_TOKEN` or
`KIMI_API_KEY`).

## Data source

- `POST https://www.kimi.com/apiv2/kimi.gateway.billing.v1.BillingService/GetUsages`
- Headers: `Authorization: Bearer <token>`, `Cookie: kimi-auth=<token>`
- Body: `{"scope":["FEATURE_CODING"]}`

The `FEATURE_CODING` usage carries a weekly `detail` (limit/used/remaining/resetTime)
and an optional rate-limit `limits[0].detail`.

## Behavior

- Primary = weekly requests window, secondary = rate-limit window (5h).
- Used is derived from `limit − remaining` when `used` is absent.

## Multiple accounts

```bash
usage-monitor-cli kimi account add work --label "Work"
usage-monitor-cli kimi account set work token "ey..."
usage-monitor-cli fetch kimi --account work
```

## Notes

The token is the `kimi-auth` value from a logged-in browser; it expires with the
session.
