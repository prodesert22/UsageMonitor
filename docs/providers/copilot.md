# GitHub Copilot provider

Tracks GitHub Copilot premium-interaction and chat quotas using the same
internal endpoint CodexBar reads.

## Auth

A GitHub OAuth token or Personal Access Token with Copilot access:

```bash
export COPILOT_API_TOKEN=gho_...
# GITHUB_TOKEN / GH_TOKEN are also accepted
export GITHUB_TOKEN=gho_...
# or persist per account
usage-monitor-cli copilot set token gho_...
```

`api_key` is accepted as an alias for `token`.

## Data source

- `GET https://api.github.com/copilot_internal/user`
- Headers:
  - `Authorization: token <token>`
  - `X-Github-Api-Version: 2025-04-01`
  - editor headers (`Editor-Version`, `User-Agent`)

The response contains `quota_snapshots.premium_interactions`,
`quota_snapshots.chat` (each with `entitlement`, `remaining`,
`percent_remaining`, `unlimited`), plus `copilot_plan` and
`token_based_billing`.

## Behavior

- **Primary window** = premium interactions; **secondary** = chat.
- `usedPercent = 100 − percent_remaining`; when `percent_remaining` is absent it
  is derived from `entitlement`/`remaining`.
- `unlimited` quotas render as 0% used; zero-entitlement placeholder snapshots
  (GitHub returns these for token-based billing seats) are dropped.
- Token-based-billing plans with no usable quota surface the plan only, with no
  fake usage windows.
- The plan name comes from `copilot_plan` (capitalized).

## Multiple accounts

```bash
usage-monitor-cli copilot account add work --label "Work Copilot"
usage-monitor-cli copilot account set work token gho_work-...
usage-monitor-cli fetch copilot --account work
```

## Notes

The enterprise/org budget endpoint (cookie-based) is not ported; this provider
reads the per-user quota endpoint that works with a plain OAuth/PAT token.
