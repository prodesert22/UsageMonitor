# OpenCode Go provider

Monitors [OpenCode Go](https://opencode.ai) workspace usage: the rolling
5-hour window, the weekly quota, and the monthly quota when the plan exposes
one. Multiple workspaces can be tracked with a single auth token.

> **Status: manual setup required.** OpenCode Go has no public usage API.
> Data comes from the opencode.ai web dashboard, authenticated with your
> browser session cookie. The `sk-...` API key in
> `~/.local/share/opencode/auth.json` does **not** work for this — it is a
> model-gateway key, not a dashboard session. Because of that, this provider
> is never auto-enabled: it stays `disabled (auto)` until you configure a
> token and enable it.

## Setup

1. Open https://opencode.ai and log in.
2. DevTools → Network tab → reload → click any `opencode.ai` request.
3. Copy either:
   - the full value of the `Cookie` request header, or
   - only the OpenCode `auth` cookie value, which usually starts with
     `Fe26...`.
4. Configure and enable the provider. Always wrap the token in **single
   quotes** so your shell does not split or expand characters like `;`, `$`,
   `*`, and spaces:

```bash
usage-monitor-cli opencode-go set token '<full Cookie header value or Fe26 auth value>'
usage-monitor-cli enable opencode-go
usage-monitor-cli fetch opencode-go
```

### Accepted token formats

The provider accepts all of these input formats:

```bash
# 1. Bare OpenCode auth cookie value.
# The CLI automatically sends this as: Cookie: auth=<value>
usage-monitor-cli opencode-go set token 'Fe26.2**YOUR_FULL_AUTH_VALUE'

# 2. Explicit auth cookie pair.
usage-monitor-cli opencode-go set token 'auth=Fe26.2**YOUR_FULL_AUTH_VALUE'

# 3. Full Cookie header value copied from DevTools.
usage-monitor-cli opencode-go set token 'auth=Fe26.2**YOUR_FULL_AUTH_VALUE; other_cookie=value; another=value'

# 4. Full header line copied with the Cookie: prefix.
# The CLI strips the leading "Cookie:" automatically.
usage-monitor-cli opencode-go set token 'Cookie: auth=Fe26.2**YOUR_FULL_AUTH_VALUE; other_cookie=value'
```

If `opencode-go show` prints a short value such as `(28 chars)` but your real
cookie is much longer, the token was probably pasted without quotes and your
shell truncated it. Set it again using single quotes.

The token is the auth credential for this provider. Manage it with the
provider command (the value is masked when shown):

```bash
usage-monitor-cli opencode-go set token '<Cookie header or Fe26 auth value>'  # set/replace
usage-monitor-cli opencode-go show                                           # show (masked)
usage-monitor-cli opencode-go unset token                                    # remove
```

### Pinning workspaces (optional)

When no workspaces are configured, they are auto-discovered and **all** of
them are fetched with the same token. To pin specific ones,
`opencode-go workspace add` accepts either the bare `wrk_...` id or the
dashboard URL straight from the browser:

```bash
usage-monitor-cli opencode-go workspace add https://opencode.ai/workspace/wrk_aaaa/go
usage-monitor-cli opencode-go workspace add wrk_bbbb "Production"   # optional display name
usage-monitor-cli opencode-go workspace list
usage-monitor-cli opencode-go workspace remove wrk_bbbb   # empty list → back to auto-discovery
```

### Workspace names

Window labels use the workspace **name** instead of the raw `wrk_...` id.
Names are resolved in this order:

1. Manual name from `opencode-go workspace add <id> "<name>"` (stored as `"wrk_id=Name"`).
2. Name fetched automatically from the discovery payload (each workspace
   object carries a `name` field next to its id).
3. The raw id, when neither is available.

Name enrichment for pinned ids is best-effort: if the discovery endpoint
breaks, pinned workspaces keep working with their ids.

### Resulting configuration

Everything persists flat in `~/.config/usage-monitor/config.toml`:

| Key | Required | Meaning |
|-----|----------|---------|
| `token` | yes | Full browser `Cookie` header of a logged-in opencode.ai session, `auth=<Fe26...>`, or the bare `Fe26...` auth cookie value. Bare values are automatically sent as `auth=<value>`. |
| `workspaces` | no | TOML array of `wrk_...` ids, optionally with a display name (`"wrk_id=Name"`); empty/absent → auto-discovery |
| `enabled` | no | Per-account toggle; an account is enabled by default. The provider auto-enables once any account is configured |

`token` and `workspaces` are per-account keys. The bare `opencode-go set token`
/ `opencode-go workspace add` commands write to the implicit `default` account;
use `opencode-go account set <name> token ...` and
`opencode-go workspace add ... --account <name>` for additional logins, and
`opencode-go account remove <name>` to drop one.

```toml
[providers.opencode-go.accounts.default]
token = "<Cookie header, auth=Fe26 value, or bare Fe26 value>"
workspaces = ["wrk_aaaa", "wrk_bbbb=Production"]
```

## Multiple accounts

> **Easy — no token juggling.** Unlike the OAuth providers
> ([`claude`](claude.md), [`codex`](codex.md)), the session cookie doesn't
> rotate when used. Adding another account is just another `account set token`;
> nothing to copy, nothing to keep alive (the cookie still expires on its own,
> so refresh it from the browser when it does).

Each account holds its **own** cookie (`token`) and its **own** workspace list.
Register one per opencode.ai login:

```bash
usage-monitor-cli opencode-go account add personal --label "Personal"
usage-monitor-cli opencode-go account set personal token 'Fe26.2**...'
usage-monitor-cli opencode-go account add team --label "Team"
usage-monitor-cli opencode-go account set team token 'Fe26.2**...'
```

**Workspaces are per account — pick the account when adding one.** The
`workspace` commands take `--account <name>` (default: `default`). A workspace
added without `--account` lands on the `default` account, *not* on your named
ones:

```bash
# add a workspace to the "team" account specifically
usage-monitor-cli opencode-go workspace add wrk_xxxx --account team
usage-monitor-cli opencode-go workspace list --account team
usage-monitor-cli opencode-go workspace remove wrk_xxxx --account team
```

Leave an account's workspace list empty to auto-discover every workspace its
cookie can see. See the [main README](../../README.md#multiple-accounts) for the
full account command reference.

## How extraction works

### Workspace discovery

Workspace IDs (`wrk_...`) are listed by an internal SolidStart server
function of the opencode.ai web app:

```
GET https://opencode.ai/_server?id=<server-fn-hash>
Cookie: <session cookie>
X-Server-Id: <server-fn-hash>
X-Server-Instance: server-fn:<unique-id>
Origin: https://opencode.ai
Referer: https://opencode.ai/
Accept: text/javascript, application/json;q=0.9, */*;q=0.8
User-Agent: <desktop browser UA>
```

- The `id` is a **build-specific hash** of the server function
  (`def39973159c7f0483d8793a822b8dbb10d067e12c65455fcb4608459ba0234f` at the
  time of writing). It changes when opencode.ai redeploys; if discovery
  breaks, pin `workspaces` explicitly — pinned ids skip discovery entirely.
- The response is a JS/JSON hydration payload. Any string shaped like
  `wrk_<alphanumeric>` is collected as a workspace id (deduplicated, in
  order of appearance).
- One session sees **every workspace the account belongs to** — this is
  what enables tracking multiple workspaces with the same auth token.

### Usage extraction

For each workspace, the provider fetches the dashboard page:

```
GET https://opencode.ai/workspace/<wrk_id>/go
Cookie: <session cookie>
User-Agent: <desktop browser UA>
Accept: text/html,application/xhtml+xml,...
```

The HTML embeds a hydration payload containing up to three usage windows,
extracted by locating each key and reading its `usagePercent` and
`resetInSec` fields:

| Key | Window | Size |
|-----|--------|------|
| `rollingUsage` | Rolling session | 5 h (300 min) |
| `weeklyUsage` | Weekly quota | 7 d (10080 min) |
| `monthlyUsage` | Monthly quota (optional) | 30 d (43200 min) |

Semantics:

- `usagePercent` is 0–100. Values ≤ 1.0 are treated as a ratio and scaled
  by 100 (some payload variants emit ratios); the result is clamped to
  0–100.
- `resetInSec` is seconds from now until the window resets
  (`resets_at = now + resetInSec`).
- `monthlyUsage` only appears on plans that expose a monthly quota.

### Signed-out detection

A 200 response can still be a login page. The body is treated as signed out
(→ auth error) when it contains any of: `login`, `sign in`,
`auth/authorize`, `not associated with an account`,
`actor of type "public"`. HTTP 401/403 are treated the same way.

## Snapshot mapping

All configured (or discovered) workspaces are fetched with the same token:

- The **first** workspace fills the snapshot's primary (rolling), secondary
  (weekly), and tertiary (monthly) windows.
- Each **additional** workspace contributes named extra windows labelled
  with its id (e.g. `wrk_bbbb rolling (5h)`, `wrk_bbbb weekly`).
- A workspace that fails to fetch is skipped with a warning; the fetch only
  errors when **no** workspace succeeds.

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| `no session token configured` | `token` not set | `opencode-go set token '<Cookie header or Fe26 auth value>'` |
| `session cookie rejected or expired` | Cookie expired, incomplete, or not the OpenCode `auth` cookie | Copy a fresh `Cookie` header from DevTools or the bare `Fe26...` auth value |
| `no workspace ids in discovery payload` | Server-function hash changed after a redeploy | Pin ids: `opencode-go workspace add wrk_...` |
| `workspace ... page is missing usage fields` | Dashboard payload shape changed | Open an issue; scraping needs updating |

## Limitations

- Web scraping of an internal endpoint: the payload shape and the
  `_server` function hash can change without notice.
- The session cookie must be refreshed manually when it expires.
- Zen credit balance is not implemented yet.
