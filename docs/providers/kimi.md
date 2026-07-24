# Kimi provider

Tracks Kimi coding weekly quota and rate limit using the same billing RPC
CodexBar reads.

## Auth

The provider uses a `kimi-auth` JWT from a logged-in [kimi.com](https://www.kimi.com)
browser session.

### Getting the token

1. Open [kimi.com](https://www.kimi.com) in Chrome/Edge/Firefox and sign in
2. Open DevTools:
   - **Chrome/Edge**: `F12` or right-click → Inspect
   - **Firefox**: `F12` or right-click → Inspect Element
3. Go to the **Application** tab (Chrome/Edge) or **Storage** tab (Firefox)
4. In the left sidebar, expand **Cookies** and select `https://www.kimi.com`
5. Find the cookie named `kimi-auth` and copy its **Value** (a long JWT string starting with `ey...`)

### Using the token

**Via env var** (recomendado — não persiste em disco):
```bash
export KIMI_AUTH_TOKEN="ey..."
usage-monitor-cli fetch kimi
```

**Via CLI** (persiste em `~/.config/usage-monitor/config.toml`):
```bash
usage-monitor-cli enable kimi
usage-monitor-cli kimi set token "ey..."
usage-monitor-cli fetch kimi
```

**Via config direto** (`~/.config/usage-monitor/config.toml`):
```toml
[kimi]
enabled = true
token = "ey..."
```

`api_key` e `cookie` são aceitos como aliases do campo `token`.
Env vars: `KIMI_AUTH_TOKEN` ou `KIMI_API_KEY`.

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

Tokens don't rotate, so adding accounts is straightforward:

```bash
# Add a second Kimi account
usage-monitor-cli kimi account add work --label "Work"
usage-monitor-cli kimi account set work token "ey..."

usage-monitor-cli fetch kimi                    # all accounts
usage-monitor-cli fetch kimi --account work     # just one
```

Each account carries its own token — useful when you have multiple
kimi.com logins. The auto-detected default (from `KIMI_AUTH_TOKEN`) is
fetched alongside named accounts unless you disable it:

```bash
usage-monitor-cli kimi account disable default
```

## Notes

The token is the `kimi-auth` value from a logged-in browser; it expires with the
session. When it expires, re-copy the cookie from the browser and update the token.
