# z.ai provider

Tracks z.ai coding-plan token and prompt quotas via the quota API CodexBar reads
(API-key auth).

## Auth

```bash
export Z_AI_API_KEY="..."
# optional non-global host / full quota URL
export Z_AI_API_HOST="open.bigmodel.cn"
# or persist per account
usage-monitor-cli zai set api_key "..."
```

`token` aliases `api_key`; `base_url`/`host` config overrides the default
`https://api.z.ai`.

## Data source

- `GET https://api.z.ai/api/monitor/usage/quota/limit`
- Header: `Authorization: Bearer <key>`

Reads `data.limits[]` with `type` (`TOKENS_LIMIT`/`TIME_LIMIT`), `percentage`,
`unit`/`number` (window length), and `nextResetTime`.

## Behavior

- Token limits: longest window → primary, shortest → tertiary (session).
- The time/prompt limit → secondary.
- `percentage` is the used percent; the reset time comes from `nextResetTime`.
- An empty body usually means a region mismatch (Global vs BigModel CN).

## Multiple accounts

```bash
usage-monitor-cli zai account add work --label "Work"
usage-monitor-cli zai account set work api_key "..."
usage-monitor-cli fetch zai --account work
```
