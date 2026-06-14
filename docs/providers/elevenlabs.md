# ElevenLabs provider

Tracks ElevenLabs subscription usage through the same API endpoint used by
CodexBar.

## Auth

Use either environment variables or persisted account config:

```bash
export ELEVENLABS_API_KEY=xi_...
# or
export XI_API_KEY=xi_...

usage-monitor-cli elevenlabs set api_key xi_...
```

`token` is accepted as an alias for `api_key`.

## Data source

UsageMonitor requests:

`GET https://api.elevenlabs.io/v1/user/subscription`

Headers:

- `xi-api-key: <api key>`
- `Accept: application/json`

## Config keys

| Key | Description |
|-----|-------------|
| `api_key` / `token` | ElevenLabs API key |
| `api_url` / `base_url` | Override API base URL, defaults to `https://api.elevenlabs.io` |

Environment equivalents:

- `ELEVENLABS_API_KEY`
- `XI_API_KEY`
- `ELEVENLABS_API_URL`

## Output mapping

- Primary window: character credit usage (`character_count / character_limit`).
- Reset time: `next_character_count_reset_unix`, when returned.
- Extra windows: voice slot and professional voice slot usage when available.
- Plan info: tier/status plus any current overage returned by ElevenLabs.

## Multiple accounts

```bash
usage-monitor-cli elevenlabs account add work --label "Work ElevenLabs"
usage-monitor-cli elevenlabs account set work api_key xi_...
usage-monitor-cli fetch elevenlabs --account work
```
