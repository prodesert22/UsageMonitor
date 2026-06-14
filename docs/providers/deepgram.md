# Deepgram provider

Tracks Deepgram project usage through the Management API usage breakdown
endpoint, following CodexBar's provider behavior.

## Auth

```bash
export DEEPGRAM_API_KEY=dg_...
# Optional: pin a single project instead of discovering all projects
export DEEPGRAM_PROJECT_ID=project-...

usage-monitor-cli deepgram set api_key dg_...
usage-monitor-cli deepgram set project_id project-...
```

`token` is accepted as an alias for `api_key`.

## Data sources

If `project_id`/`DEEPGRAM_PROJECT_ID` is configured:

`GET https://api.deepgram.com/v1/projects/<project-id>/usage/breakdown`

Otherwise UsageMonitor first discovers projects:

`GET https://api.deepgram.com/v1/projects`

and then fetches usage breakdown for each project, aggregating totals.

Headers:

- `Authorization: Token <api key>`
- `Accept: application/json`

## Config keys

| Key | Description |
|-----|-------------|
| `api_key` / `token` | Deepgram API key |
| `project_id` | Optional project ID; when absent all projects are discovered |
| `api_url` / `base_url` | Override API base URL, defaults to `https://api.deepgram.com/v1` |
| `start` / `end` | Optional usage query dates |

Environment equivalents:

- `DEEPGRAM_API_KEY`
- `DEEPGRAM_PROJECT_ID`
- `DEEPGRAM_API_URL`

## Output mapping

- Primary window label: total request count.
- Secondary window label: audio hours and billable hours.
- Tertiary window label: input+output tokens and TTS characters.
- Extra window: agent hours when present.
- Plan info: selected project name/ID or aggregate project count plus period.

## Multiple accounts

```bash
usage-monitor-cli deepgram account add prod --label "Prod Deepgram"
usage-monitor-cli deepgram account set prod api_key dg_...
usage-monitor-cli deepgram account set prod project_id project-...
usage-monitor-cli fetch deepgram --account prod
```
