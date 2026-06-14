# Cursor provider

Tracks Cursor plan usage and on-demand spend through the same web dashboard
endpoints CodexBar uses. Cursor has no public usage API key — authentication is
the browser session cookie.

## Auth

Cursor authenticates with a browser session cookie (`WorkosCursorSessionToken`).
Provide either the full cookie header or just the token:

```bash
# just the session token (wrapped as WorkosCursorSessionToken=<token>)
export CURSOR_SESSION_TOKEN=...
# or persist per account
usage-monitor-cli cursor set token <session-token>
# or pass the full cookie header verbatim
usage-monitor-cli cursor set cookie "WorkosCursorSessionToken=...; other=..."
```

Grab the cookie from your logged-in browser at `cursor.com` (DevTools →
Application → Cookies). `cookie` is sent verbatim; `token` (or `api_key`) is
wrapped as `WorkosCursorSessionToken=<value>`.

## Data source

- `GET https://cursor.com/api/usage-summary` — plan percent + on-demand spend
- `GET https://cursor.com/api/auth/me` — account id (best effort)
- `GET https://cursor.com/api/usage?user=<id>` — legacy request-based plans
  (best effort)

## Behavior

- **Primary window** is the plan percentage, using Cursor's precedence:
  `totalPercentUsed` → average of `autoPercentUsed`/`apiPercentUsed` → a single
  lane → plan cents ratio → `overall` cents ratio → team `pooled` cents ratio.
- **Legacy request plans** (`/api/usage` with `gpt-4.maxRequestUsage`) replace
  the headline window with `numRequests / maxRequestUsage`.
- **On-demand spend** surfaces as a USD credits pool (`used`, `total`,
  `balance = total − used`). Cursor reports these in cents.
- The plan name comes from `membershipType` (Pro/Hobby/Team/Enterprise).
- The billing cycle end populates the window reset time.

## Multiple accounts

```bash
usage-monitor-cli cursor account add work --label "Work Cursor"
usage-monitor-cli cursor account set work token <session-token>
usage-monitor-cli fetch cursor --account work
```

## Notes

Session cookies are not auto-extracted from browsers on Linux — supply the
cookie/token explicitly. Tokens expire when the browser session ends; refresh
the stored value if `fetch` reports "not logged in".
