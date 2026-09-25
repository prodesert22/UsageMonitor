# Antigravity provider

Tracks Antigravity (Google Code Assist) quotas through the same endpoints and
CLI surface CodexBar uses.

## Auth

No credentials file is needed when the `agy` CLI is signed in: the provider
runs `agy -p /usage --output-format json` and parses the weekly quota groups
(CodexBar's `antigravity.cli-https` print-report path). Auto-enables when `agy`
is installed and has run as the current user (`~/.gemini/antigravity-cli`
exists).

```bash
# default: local `agy` report (needs `agy` signed in, no file needed)
usage-monitor-cli fetch antigravity

# point at a different `agy` binary
usage-monitor-cli antigravity set cli_path /path/to/agy

# fallback without `agy`: read an OAuth credentials file
usage-monitor-cli antigravity set credentials_path /path/to/oauth_creds.json

# or supply a short-lived access token directly
usage-monitor-cli antigravity set access_token "ya29...."

# pin the GCP project (remote path only)
usage-monitor-cli antigravity set project my-antigravity-project
```

The OAuth fallback reads `~/.codexbar/antigravity/oauth_creds.json` by default
(CodexBar format), refreshing an expired token when the OAuth client is known
(`client_id`/`client_secret` config keys, `ANTIGRAVITY_OAUTH_CLIENT_ID` /
`ANTIGRAVITY_OAUTH_CLIENT_SECRET`, or the client stored in the file).

## Plan label

When any OAuth credentials are available (Antigravity file, explicit
`access_token`, or the gemini-cli file at `~/.gemini/oauth_creds.json`), the
provider also queries `loadCodeAssist` and resolves the plan the way CodexBar
does: `paidTier.name` (e.g. `Plus`) wins, then `planInfo.planType`, then the
current tier (`standard-tier` → `Paid`, `free-tier` → `Free`,
`legacy-tier` → `Legacy`). Enrichment is best-effort and never fails the
fetch; without plan info the label stays `Antigravity`.

Note: migrated consumer accounts report no tier on `loadCodeAssist`
(`free-tier` listed as `ineligibleTiers` with `UNSUPPORTED_CLIENT`) — that is
Google's June 2026 Code Assist → Antigravity migration, not a login problem.
Usage for those accounts flows through the `agy` CLI report.

## Data source

- `agy -p /usage --output-format json` — weekly quota groups (preferred,
  no credentials file needed)
- `POST https://cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels` — per-model quota (fallback)
- `POST https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuota` — quota buckets (fallback)
- `POST https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist` — plan label only (best-effort)
- `POST https://oauth2.googleapis.com/token` — refresh

The CLI report returns `groups[]` of quota `buckets[]` (`id`, `name`,
`remaining_fraction`, `reset_time`). Gemini groups map to the primary window,
Claude/GPT groups to secondary; every bucket also lands in the named extra
windows (`antigravity-quota-summary-<bucket-id>`).

## Behavior

- The CLI source reports weekly windows (`usedPercent = 100 −
  remainingFraction × 100`, 10080-minute windows with `resets_at` from the
  report, `billing_period: weekly`).
- The remote fallback reports per-model daily quotas from
  `fetchAvailableModels`. When every model reports full (or none are
  returned), `retrieveUserQuota` is queried as the authoritative source for
  consumed quota. Models are ordered by remaining fraction: the most-consumed
  becomes the primary window, then secondary and tertiary; any extras land in
  the snapshot's named extra windows.

## Multiple accounts

```bash
usage-monitor-cli antigravity account add work --label "Work Antigravity"
usage-monitor-cli antigravity account set work credentials_path /path/to/work/oauth_creds.json
usage-monitor-cli fetch antigravity --account work
```

## Notes

Credentials are not auto-discovered from an Antigravity.app bundle on Linux —
sign in with `agy`, or supply the credentials file or an access token. If the
access token has expired and no OAuth client is configured, the refresh step
reports a clear error.
