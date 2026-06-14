# Widgets

Usage Monitor ships a stable widget JSON contract plus reference widgets for KDE
Plasma 6 and Waybar.

## CLI contract

```bash
usage-monitor-cli widget waybar
usage-monitor-cli widget kde --pretty
```

Both commands fetch all enabled providers and emit one JSON object:

```json
{
  "text": "84%",
  "tooltip": "Claude — Work: Session 42% · Weekly 84%",
  "class": "warning",
  "percentage": 84,
  "has_errors": false,
  "providers": [],
  "updated_at": "2026-06-14T12:00:00Z"
}
```

`class` is one of `ok`, `warning`, `critical`, or `stale`.
`has_errors` is true when at least one requested provider/account failed. If all
successful providers are below warning thresholds, partial failures surface as
`class: "stale"` so bars can style the module differently.

Provider entries currently include:

- `provider_id`, `display_name`, optional `account_id` / `account_label`
- optional `plan`
- `windows[]`, with `id`, `label`, `percentage`, `status`, and optional
  `used`, `limit`, `remaining`, `resets_at`
- `max_percentage`, `status`
- optional `error`, `credits`, and `cost`

Consumers should treat unknown extra fields as additive and ignore them.

## Available widgets

- [KDE Plasma 6](kde.md) — native panel widget with settings, toggles,
  multi-account display, provider ordering, pinned target, refresh interval,
  and stale-cache fallback.
- [Waybar](waybar.md) — wrapper script for a `custom/*` module.
