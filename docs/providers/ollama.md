# Ollama provider

Tracks Ollama cloud session and weekly usage by reading the settings page, the
same source CodexBar scrapes. Authentication is the browser session cookie.

## Auth

```bash
export OLLAMA_COOKIE="..."            # full Cookie header
# or persist per account
usage-monitor-cli ollama set cookie "..."
```

Grab the cookie from a logged-in browser at `ollama.com`. `token` aliases
`cookie`.

## Data source

- `GET https://ollama.com/settings` (HTML)

The "Session usage" (or "Hourly usage") and "Weekly usage" percentages and the
"Cloud Usage" plan name are parsed from the page.

## Behavior

- Primary = session window (5h), secondary = weekly window (7d).
- A signed-out page surfaces as an auth failure.

## Multiple accounts

```bash
usage-monitor-cli ollama account add work --label "Work"
usage-monitor-cli ollama account set work cookie "..."
usage-monitor-cli fetch ollama --account work
```

## Notes

Cookies are not auto-extracted from browsers on Linux — supply the cookie. The
settings page is HTML-scraped, so layout changes upstream can affect parsing.
