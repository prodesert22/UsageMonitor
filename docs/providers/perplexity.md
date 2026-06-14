# Perplexity provider

Tracks Perplexity plan, bonus, and purchased credits through the same billing
endpoint CodexBar uses. Authentication is the browser session cookie.

## Auth

```bash
# session token (wrapped as __Secure-next-auth.session-token=<token>)
export PERPLEXITY_SESSION_TOKEN=...
# or persist per account
usage-monitor-cli perplexity set token <session-token>
# or pass the full cookie header verbatim
usage-monitor-cli perplexity set cookie "__Secure-next-auth.session-token=...; other=..."
```

Grab the cookie from your logged-in browser at `perplexity.ai` (DevTools →
Application → Cookies). `cookie` is sent verbatim; `token` (or `api_key`) is
wrapped as `__Secure-next-auth.session-token=<value>`.

## Data source

- `GET https://www.perplexity.ai/rest/billing/credits?version=2.18&source=default`
- Headers: `Cookie`, `Origin`, `Referer`, `Accept: application/json`

The response contains `balance_cents`, `total_usage_cents`,
`current_period_purchased_cents`, `renewal_date_ts`, and `credit_grants[]`
(`type` of `recurring`/`promotional`/`purchased`, `amount_cents`,
`expires_at_ts`).

## Behavior

- Credits are attributed in a waterfall: **recurring → purchased → promotional**
  against `total_usage_cents`.
- **Primary window** = recurring (monthly) plan credits, reset at the renewal
  date. **Secondary** = promotional bonus credits (with expiry). **Tertiary** =
  purchased on-demand credits.
- Expired promotional grants are excluded.
- Purchased credits take the larger of the top-level field and the grants total
  to avoid double counting.
- `Credits.balance` is `balance_cents / 100`; bonus/purchased/used mirror the
  pools in USD.
- The plan name is inferred from the recurring allotment (Free = 0, Pro < $50,
  Max ≥ $50).

## Multiple accounts

```bash
usage-monitor-cli perplexity account add work --label "Work Perplexity"
usage-monitor-cli perplexity account set work token <session-token>
usage-monitor-cli fetch perplexity --account work
```

## Notes

Session cookies are not auto-extracted from browsers on Linux — supply the
cookie/token explicitly. All Perplexity API timestamps are Unix seconds.
