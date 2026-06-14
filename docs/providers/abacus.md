# Abacus AI provider

Tracks Abacus AI organization compute points using the same dashboard endpoints
CodexBar reads. Authentication is the browser session cookie.

## Auth

```bash
export ABACUS_COOKIE="..."          # full Cookie header
# or persist per account
usage-monitor-cli abacus set cookie "..."
```

Grab the cookie from a logged-in browser at `apps.abacus.ai` (DevTools →
Application → Cookies). A `token` config key is accepted as an alias.

## Data source

- `GET https://apps.abacus.ai/api/_getOrganizationComputePoints` — `totalComputePoints`, `computePointsLeft`
- `GET https://apps.abacus.ai/api/_getBillingInfo` — `currentTier`, `nextBillingDate` (best effort)

## Behavior

- Primary window = compute points used (`total − left`) over `total`.
- `Credits.balance` = points left; `total`/`used` mirror the pool.
- Plan name comes from `currentTier`; the billing date sets the reset.

## Multiple accounts

```bash
usage-monitor-cli abacus account add work --label "Work"
usage-monitor-cli abacus account set work cookie "..."
usage-monitor-cli fetch abacus --account work
```

## Notes

Cookies are not auto-extracted from browsers on Linux — supply the cookie. It
expires with the browser session; refresh if `fetch` reports a rejected cookie.
