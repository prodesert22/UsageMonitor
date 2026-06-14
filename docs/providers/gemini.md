# Google Gemini provider

Tracks Gemini Code Assist per-model daily quotas through the same Cloud Code
endpoints CodexBar uses, reusing the gemini-cli OAuth login.

## Auth

Log in once with the gemini-cli (`gemini`) so credentials exist at
`~/.gemini/oauth_creds.json`. Auto-enables when that file is present.

```bash
# default: read ~/.gemini/oauth_creds.json (refreshes when expired)
usage-monitor-cli fetch gemini

# point at a different credentials file
usage-monitor-cli gemini set credentials_path /path/to/oauth_creds.json

# or supply a short-lived access token directly (skips the creds file)
usage-monitor-cli gemini set access_token "$(gcloud auth print-access-token)"

# optionally pin the GCP project used for quota
usage-monitor-cli gemini set project gen-lang-client-0123456789
```

## Data source

- `POST https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist` — project id
- `POST https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuota` — quota buckets
- `POST https://oauth2.googleapis.com/token` — refresh (public gemini-cli client)

Each quota bucket carries `modelId`, `remainingFraction`, and `resetTime`.

## Behavior

- The access token is taken from `access_token`/`token` config when present,
  otherwise read from the credentials file. An expired token is refreshed using
  the public gemini-cli OAuth client and the stored `refresh_token`.
- Buckets are grouped by model, keeping the lowest remaining fraction per model.
- Windows are assigned by family: **Gemini Pro** (primary), **Gemini Flash**
  (secondary), **Gemini Flash Lite** (tertiary), each a 24h window where
  `usedPercent = 100 − remainingFraction × 100`.
- Reset times come straight from the bucket `resetTime`.

## Multiple accounts

```bash
usage-monitor-cli gemini account add work --label "Work Gemini"
usage-monitor-cli gemini account set work credentials_path /path/to/work/oauth_creds.json
usage-monitor-cli fetch gemini --account work
```

## Notes

Only the gemini-cli OAuth (personal) flow is supported — API-key and Vertex AI
auth are out of scope for quota reads. If no `refresh_token` is stored and the
access token has expired, re-run the gemini-cli login.
