# Mistral provider

Tracks Mistral API monthly spend via the admin billing endpoint CodexBar reads.
Authentication is the browser session cookie.

## Auth

```bash
export MISTRAL_COOKIE="..."           # full Cookie header
# or persist per account
usage-monitor-cli mistral set cookie "..."
# optional CSRF token if the endpoint requires it
usage-monitor-cli mistral set csrf_token "..."
```

Grab the cookie from a logged-in browser at `admin.mistral.ai`. `token` aliases
`cookie`.

## Data source

- `GET https://admin.mistral.ai/api/billing/v2/usage?month=<m>&year=<y>`
- Header: `Cookie: <session>` (plus optional `X-CSRFTOKEN`)

A per-metric `prices` index is multiplied against each category's
`input`/`output`/`cached` usage entries (`value_paid` or `value`).

## Behavior

- Produces a `CostSnapshot` with the month-to-date metered cost and the
  account currency (default EUR).

## Multiple accounts

```bash
usage-monitor-cli mistral account add work --label "Work"
usage-monitor-cli mistral account set work cookie "..."
usage-monitor-cli fetch mistral --account work
```

## Notes

Cookies are not auto-extracted from browsers on Linux — supply the cookie. It
expires with the session.
