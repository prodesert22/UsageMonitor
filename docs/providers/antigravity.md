# Antigravity provider

Tracks Antigravity (Google Code Assist) per-model daily quotas through the same
Cloud Code endpoints CodexBar uses, reusing Antigravity's Google OAuth login.

## Auth

Antigravity stores Google OAuth credentials at
`~/.codexbar/antigravity/oauth_creds.json`. Auto-enables when that file exists.

```bash
# default: read ~/.codexbar/antigravity/oauth_creds.json (refreshes when expired)
usage-monitor-cli fetch antigravity

# point at a different credentials file
usage-monitor-cli antigravity set credentials_path /path/to/oauth_creds.json

# or supply a short-lived access token directly
usage-monitor-cli antigravity set access_token "ya29...."

# pin the GCP project
usage-monitor-cli antigravity set project my-antigravity-project
```

Refreshing an expired token needs an OAuth client. It is taken from the
`client_id`/`client_secret` config keys, the `ANTIGRAVITY_OAUTH_CLIENT_ID` /
`ANTIGRAVITY_OAUTH_CLIENT_SECRET` environment variables, or the client stored in
the credentials file.

## Data source

- `POST https://cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels` — per-model quota
- `POST https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuota` — quota buckets (fallback)
- `POST https://oauth2.googleapis.com/token` — refresh

`fetchAvailableModels` returns a `models` map (`displayName`, `label`,
`quotaInfo.remainingFraction`, `quotaInfo.resetTime`). `retrieveUserQuota`
returns `buckets[]` (`modelId`, `remainingFraction`, `resetTime`).

## Behavior

- Per-model quotas come from `fetchAvailableModels`. When every model reports
  full (or none are returned), `retrieveUserQuota` is queried as the
  authoritative source for consumed quota.
- Models are ordered by remaining fraction: the most-consumed becomes the
  primary window, then secondary and tertiary; any extras land in the snapshot's
  named extra windows.
- Each window is a 24h window where `usedPercent = 100 − remainingFraction × 100`,
  labeled with the model display name.

## Multiple accounts

```bash
usage-monitor-cli antigravity account add work --label "Work Antigravity"
usage-monitor-cli antigravity account set work credentials_path /path/to/work/oauth_creds.json
usage-monitor-cli fetch antigravity --account work
```

## Notes

Credentials are not auto-discovered from an Antigravity.app bundle on Linux —
supply the credentials file or an access token. If the access token has expired
and no OAuth client is configured, the refresh step reports a clear error.
