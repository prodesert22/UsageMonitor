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

## Installing

Both widgets are embedded in the CLI binary (asset tree under
`usage-monitor-cli/assets/`) and written to disk by a single subcommand:

```bash
usage-monitor-cli widget install kde      # KDE plasmoid (via kpackagetool6)
usage-monitor-cli widget install waybar   # Waybar module + popup into ~/.local/bin
usage-monitor-cli widget install all      # both
usage-monitor-cli widget uninstall <target>
usage-monitor-cli widget doctor           # show resolved install paths
```

### Automatic upgrades

`widget install` records the installed version
(`~/.local/share/usage-monitor/<target>.version`) and drops an XDG autostart
entry (`~/.config/autostart/usage-monitor-widget-sync.desktop`) that runs
`usage-monitor-cli widget sync` at login. When you later upgrade the CLI (for
example `cargo install usage-monitor-cli` to a newer version), the next login
reinstalls any installed widget whose recorded version is older than the binary,
so the desktop widgets stay in step without a manual reinstall. `widget sync`
never installs a widget that was not already installed, and the autostart entry
is removed once the last widget is uninstalled.

### Update notice in the widgets

Beyond the silent login sync, each widget notices it is outdated itself: the
popup compares its installed version with the CLI binary (local, no network)
and shows a banner — **"Update X available (installed Y)"** — with three
actions:

- **What's new** — fetches the release notes for the new version (the
  per-version file `releases/vX.Y.Z.md` from the repo first, GitHub Releases
  API next, changelog bundled in the binary when offline; cached for a day)
  and shows them inline, rendered from markdown. The settings **Updates** tab
  fetches and shows the same notes.
- **Update now** — reinstalls the widget from the current binary, the same
  path as `usage-monitor-cli widget install <target>`. (On KDE, interface
  changes in the new version load on the next Plasma restart; helper-only
  changes apply immediately.)
- **Dismiss (×)** — hides the popup notice for that version only. The update
  stays visible in the settings **Updates** tab (versions, Update button and a
  link to the release page), which always shows a pending update regardless of
  the dismissal.

From the terminal, the same state is available without the UI:

```bash
usage-monitor-cli widget check-update            # installed vs binary, per target
usage-monitor-cli widget changelog 0.8.1         # release notes (GitHub, embedded fallback)
usage-monitor-cli widget sync kde                # reinstall one outdated widget now
```

## Available widgets

- [KDE Plasma 6](kde.md) — native panel widget with settings, toggles,
  multi-account display, provider ordering, pinned target, refresh interval,
  and stale-cache fallback.
- [Waybar](waybar.md) — wrapper script for a `custom/*` module, plus a Qt Quick
  popup (`on-click`) carrying the same interface as the KDE widget: provider
  cards, cost, and a settings window with General/Providers/Order/Theme pages.
  The popup needs PySide6 or PyQt6; the bar module does not.
