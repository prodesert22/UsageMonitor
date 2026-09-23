# OpenCode Go provider

Monitors [OpenCode Go](https://opencode.ai) quota: the rolling 5-hour
window, the weekly quota, and the monthly quota when the plan exposes one.
The values are the same used percents the OpenCode dashboard shows.

> **How it works.** The provider calls the official Zen usage endpoint,
> `GET https://opencode.ai/zen/go/v1/usage`, with your OpenCode Go API key
> as `Authorization: Bearer <key>`. One key covers the whole account, so
> there is nothing to pin: workspace pinning (`wrk_...`) was removed.

## Setup

The key is auto-detected from the desktop login
(`~/.local/share/opencode/auth.json`: the `opencode-go` entry, falling back
to the `opencode` entry) or from the `OPENCODE_API_KEY` environment variable. When neither is available, set it
explicitly — always wrap the value in **single quotes** so your shell does
not split or expand special characters:

```bash
usage-monitor-cli opencode-go set token '<opencode-go API key>'
usage-monitor-cli enable opencode-go
usage-monitor-cli fetch opencode-go
```

A pasted `Bearer <key>` value is accepted and stripped automatically.

The token is the auth credential for this provider. Manage it with the
provider command (the value is masked when shown):

```bash
usage-monitor-cli opencode-go set token '<key>'  # set/replace
usage-monitor-cli opencode-go show                # show (masked)
usage-monitor-cli opencode-go unset token         # remove
```

### Resulting configuration

Everything persists flat in `~/.config/usage-monitor/config.toml`:

| Key | Required | Meaning |
|-----|----------|---------|
| `token` | yes (unless auto-detected) | OpenCode Go API key, sent as `Authorization: Bearer <key>`. `api_key` works as an alias. |
| `enabled` | no | Per-account toggle; an account is enabled by default. The provider auto-enables once any account is configured or a key is detected |

`token` is a per-account key. The bare `opencode-go set token` command
writes to the implicit `default` account; use
`opencode-go account set <name> token ...` for additional logins, and
`opencode-go account remove <name>` to drop one.

```toml
[providers.opencode-go.accounts.default]
token = "<opencode-go API key>"
```

Legacy `workspaces = [...]` entries from older versions are ignored (they
still load, they just have no effect).

## Multiple accounts

Each account holds its **own** API key. Register one per OpenCode login:

```bash
usage-monitor-cli opencode-go account add personal --label "Personal"
usage-monitor-cli opencode-go account set personal token '<key>'
usage-monitor-cli opencode-go account add team --label "Team"
usage-monitor-cli opencode-go account set team token '<key>'
```

See the [main README](../../README.md#multiple-accounts) for the full
account command reference.

## How extraction works

```
GET https://opencode.ai/zen/go/v1/usage
Authorization: Bearer <key>
Accept: application/json
```

The response carries the account-wide windows:

```json
{"usage": {
  "rolling": {"status": "ok", "percent": 12.0, "resetsAt": "2026-09-23T05:00:00.000Z"},
  "weekly":  {"status": "ok", "percent": 45.0, "resetsAt": "2026-09-29T00:00:00.000Z"},
  "monthly": {"status": "ok", "percent": 5.0,  "resetsAt": "2026-10-23T00:00:00.000Z"}
}}
```

| Key | Window | Size |
|-----|--------|------|
| `rolling` | Rolling session | 5 h (300 min) |
| `weekly` | Weekly quota | 7 d (10080 min) |
| `monthly` | Monthly quota (optional) | 30 d (43200 min) |

Semantics:

- `percent` is the **used** percent (0–100), exactly as the dashboard
  shows it; the result is clamped to 0–100.
- `status` is `ok` or `rate-limited`; a rate-limited window reports as
  exhausted.
- `resetsAt` is the server-computed reset time
  (`resets_at = resetsAt`).
- `monthly` only appears on plans that expose a monthly quota.

Redirects are never followed, so the key is only ever sent to
`https://opencode.ai`.

## Snapshot mapping

- `rolling` fills the snapshot's primary window, `weekly` the secondary,
  and `monthly` the tertiary (when present).

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| `no API key configured` | No `token` set and no auto-detected key | `opencode-go set token '<key>'`, or sign in with opencode |
| `API key rejected (HTTP 401)` | Invalid or revoked key | Check the key; re-issue it if needed |
| `API key rejected (HTTP 403)` | Key valid but no Go access (e.g. `EntitlementError`: no subscription) | Check the subscription at https://opencode.ai |
| `response is missing rolling/weekly usage windows` | Endpoint payload shape changed | Open an issue; parsing needs updating |

## Limitations

- The endpoint shape is first-party and undocumented: it can change
  without notice.
- Zen credit balance is not implemented yet.
